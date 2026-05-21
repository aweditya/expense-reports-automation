"""Exhaustive audit of /uploads/<id>/edit for a rendered workbench.

Reads every `data-path` attribute from the workbench HTML (that's
the universe of paths the FA can click), then for each path:

  1. Reads the existing value + type from reduced/report.json
  2. Generates a plausible new value of the right type
  3. POSTs to /uploads/<id>/edit
  4. Records HTTP status + response body

At the end, prints a summary broken down by outcome category:
  PASS    : 200, render succeeded
  WALK    : path did not resolve (walker bug)
  COERCE  : type coercion error (heuristic miss)
  ENUM    : enum validation rejected (Python enum table out of sync)
  RENDER  : render binary failed after mutation (schema mismatch)
  OTHER   : anything else

Non-destructive: backs up reduced/report.json + workbench.html +
lines.csv before testing; restores them at the end so the FA's
upload state is unchanged when the audit completes (even if it
crashes — uses a try/finally).

Usage:
  scripts/audit_edit_paths.py <upload_id> [--port 8765]

Exit code: 0 if all paths PASS, 1 if any failures (so the script
can run as a regression test).
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
UPLOADS_ROOT = REPO_ROOT / ".scratch" / "uploads"

# Parser for data-path="..." attrs. Skips attributes that wrap binary
# noise; the renderer always emits ASCII paths.
DATA_PATH_RE = re.compile(r'data-path="([^"]+)"')


def parse_paths(workbench_html: str) -> list[str]:
    """Pull every data-path attribute out of the rendered workbench
    HTML. Deduplicates; preserves first-seen order so the audit
    output is stable across runs."""
    seen: set[str] = set()
    out: list[str] = []
    for m in DATA_PATH_RE.finditer(workbench_html):
        path = m.group(1)
        if path not in seen:
            seen.add(path)
            out.append(path)
    return out


def get_at_path(report: dict, path: str):
    """Return the existing value at `path`, mirroring _walk_to_leaf
    in local_app_simple.py (kept independent so this audit doesn't
    import the Flask module). Returns (existing_value, found_bool)."""
    if path.startswith("expense_report."):
        path = path[len("expense_report."):]
    parts: list[tuple[str, str | int]] = []
    for chunk in path.split("."):
        m = re.match(r"^([^\[]+)((?:\[\d+\])*)$", chunk)
        if not m:
            return (None, False)
        parts.append(("key", m.group(1)))
        for idx_match in re.finditer(r"\[(\d+)\]", m.group(2) or ""):
            parts.append(("idx", int(idx_match.group(1))))
    cursor: object = report
    for kind, val in parts:
        if kind == "key":
            if not isinstance(cursor, dict) or val not in cursor:
                return (None, False)
            cursor = cursor[val]
        else:
            if not isinstance(cursor, list) or val >= len(cursor):
                return (None, False)
            cursor = cursor[val]
    # Wrapped<T>: pull .value out
    if isinstance(cursor, dict) and "value" in cursor:
        return (cursor["value"], True)
    return (cursor, True)


# Load codegen-emitted SSOTs. Both keyed by path template (indices
# stripped to `[]`). Audit uses these to generate a type-appropriate
# new value per path — so a null-existing numeric field gets a
# number, not a string that'd fail Rust deserialize.
_ENUM_JSON = REPO_ROOT / "generated" / "enum_values.json"
_FIELD_TYPES_JSON = REPO_ROOT / "generated" / "field_types.json"
SAFE_ENUM_VALUES_BY_PATH: dict[str, str] = {}
FIELD_TYPES_BY_PATH: dict[str, dict] = {}
if _ENUM_JSON.exists():
    _data = json.loads(_ENUM_JSON.read_text())
    for tpl, values in _data.get("enums", {}).items():
        if values:
            SAFE_ENUM_VALUES_BY_PATH[tpl] = values[0]
if _FIELD_TYPES_JSON.exists():
    FIELD_TYPES_BY_PATH = dict(
        json.loads(_FIELD_TYPES_JSON.read_text()).get("fields", {})
    )

_INDEX_RE = re.compile(r"\[\d+\]")


def _normalize_path_template(path: str) -> str:
    """Match the Flask validator's path normalization so the audit's
    enum/type lookups hit the same entries the server uses."""
    return _INDEX_RE.sub("[]", path)


def plausible_new_value(path: str, existing) -> str:
    """Generate a plausible FA edit for a given existing value. Pick
    a value of the right runtime type so the audit doesn't trip on
    type-coercion errors when we're actually testing the walker.

    Decision order:
      1. Enum path (codegen SSOT)  → first valid value
      2. Existing runtime type     → number+1, bool flip, str+suffix
      3. Schema type from codegen  → type-appropriate value for null
                                     fields where runtime type signal
                                     isn't available
      4. Fallback                  → "audit value"
    """
    template = _normalize_path_template(path)
    if template in SAFE_ENUM_VALUES_BY_PATH:
        return SAFE_ENUM_VALUES_BY_PATH[template]
    if isinstance(existing, bool):
        return "yes" if not existing else "no"
    if isinstance(existing, (int, float)):
        return str(round(float(existing) + 1.0, 2))
    if isinstance(existing, str) and existing:
        if re.match(r"^\d{4}-\d{2}-\d{2}$", existing):
            return "2026-06-15"
        return existing + " (audit)"
    # Existing is None/empty: consult the schema type.
    field_meta = FIELD_TYPES_BY_PATH.get(template) or {}
    schema_type = field_meta.get("type")
    if schema_type == "number":
        return "42.50"
    if schema_type == "boolean":
        return "yes"
    if schema_type == "date":
        return "2026-06-15"
    return "audit value"


def post_edit(upload_id: str, port: int, path: str, value: str) -> tuple[int, str]:
    """POST /uploads/<id>/edit and return (status, body_excerpt)."""
    url = f"http://127.0.0.1:{port}/uploads/{upload_id}/edit"
    payload = json.dumps({"path": path, "value": value}).encode("utf-8")
    req = urllib.request.Request(
        url, data=payload, method="POST",
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return (resp.getcode(), resp.read().decode("utf-8", "replace")[:400])
    except urllib.error.HTTPError as err:
        return (err.code, err.read().decode("utf-8", "replace")[:400])
    except urllib.error.URLError as err:
        return (-1, f"URLError: {err.reason}")


def categorize(status: int, body: str) -> str:
    """Bucket the result so the audit summary is actionable."""
    if status == 200:
        return "PASS"
    if status == 400:
        if "did not resolve" in body:
            return "WALK"
        if "must be a number" in body or "must be yes or no" in body:
            return "COERCE"
        if "must be one of" in body:
            return "ENUM"
        return "OTHER"
    if status == 500:
        if "render failed" in body:
            return "RENDER"
        return "OTHER"
    if status < 0:
        return "OTHER"  # network error
    return "OTHER"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upload_id")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--verbose", action="store_true",
                        help="Print each path's result as it runs")
    args = parser.parse_args()

    upload_dir = UPLOADS_ROOT / args.upload_id
    if not upload_dir.is_dir():
        print(f"error: {upload_dir} does not exist", file=sys.stderr)
        return 2

    workbench = upload_dir / "workbench.html"
    report = upload_dir / "reduced" / "report.json"
    csv_domestic = upload_dir / "lines-domestic.csv"
    csv_foreign = upload_dir / "lines-foreign.csv"
    if not workbench.exists() or not report.exists():
        print(f"error: workbench.html or report.json missing under {upload_dir}", file=sys.stderr)
        return 2

    paths = parse_paths(workbench.read_text())
    print(f"# audit: {len(paths)} editable paths in workbench.html")

    report_data = json.loads(report.read_text())

    # Backup files we're about to mutate (each POST re-writes
    # report.json + workbench.html + both portal CSVs). Restore in finally.
    backup_dir = upload_dir / "_audit_backup"
    backup_dir.mkdir(exist_ok=True)
    backups = {
        "report.json": (report, backup_dir / "report.json"),
        "workbench.html": (workbench, backup_dir / "workbench.html"),
        "lines-domestic.csv": (csv_domestic, backup_dir / "lines-domestic.csv"),
        "lines-foreign.csv": (csv_foreign, backup_dir / "lines-foreign.csv"),
    }
    for src, dst in backups.values():
        if src.exists():
            shutil.copy2(src, dst)

    results: list[tuple[str, str, int, str]] = []  # (path, category, status, body_excerpt)
    try:
        for i, path in enumerate(paths, 1):
            # Look up existing value to pick a type-appropriate test
            # value. Missing-on-disk paths (None, False) still get
            # POSTed — the server's walker auto-creates skip-
            # serialized Wrappeds, and plausible_new_value falls
            # through to the schema-type lookup when existing is
            # None. So a "miss" here isn't a fatal pre-check anymore.
            existing, _found = get_at_path(report_data, path)
            new_value = plausible_new_value(path, existing)
            status, body = post_edit(args.upload_id, args.port, path, new_value)
            category = categorize(status, body)
            results.append((path, category, status, body))
            if args.verbose:
                print(f"  [{i}/{len(paths)}] {category:7} {path}")
            # After each POST, re-read report so subsequent paths use
            # the latest state (recompute_summary may have changed
            # transaction_summary.total_usd).
            report_data = json.loads(report.read_text())
    finally:
        # Restore the originals so the FA's upload state survives the audit.
        for src, dst in backups.values():
            if dst.exists():
                shutil.copy2(dst, src)
                dst.unlink()
        backup_dir.rmdir()

    # Summary
    by_cat = Counter(r[1] for r in results)
    print()
    print("# results")
    for cat in ("PASS", "WALK", "COERCE", "ENUM", "RENDER", "OTHER"):
        if by_cat.get(cat):
            print(f"  {cat:7} {by_cat[cat]:>4}")

    failures_by_cat: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for path, category, _status, body in results:
        if category != "PASS":
            failures_by_cat[category].append((path, body))
    if failures_by_cat:
        print()
        print("# failures (first 8 per category)")
        for cat, items in failures_by_cat.items():
            print(f"## {cat}")
            for path, body in items[:8]:
                # Strip trailing whitespace and limit body length so the output
                # stays readable on a terminal.
                snip = " ".join(body.split())[:140]
                print(f"  {path}")
                print(f"    → {snip}")
            if len(items) > 8:
                print(f"  … {len(items) - 8} more")

    return 0 if by_cat.get("PASS", 0) == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
