#!/usr/bin/env python3
"""One-time + on-demand cleanup of test/dummy expense reports from
Firestore + GCS.

Dummy data accumulates because every prod E2E test uploads a real
receipt — each one creates a reports/{upload_id} doc + GCS
artifacts. The 90-day TTL eventually reaps them, but until then
they pollute the dashboard.

Patterns we consider test data (any one match is enough to flag):
  - upload_id begins with `test_` (TestStage2cRehydrate uploads)
  - fa_input.payee_sunet matches one of the test fixtures
    (fmtest, fxtest, e2etest, scope_*, hist*)
  - filed_by_sunet matches one of those values

Default is a dry-run — prints what would be deleted without
touching anything. Pass --commit to actually delete.

Usage:
    ./.venv/bin/python scripts/cleanup_test_reports.py
    ./.venv/bin/python scripts/cleanup_test_reports.py --commit
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "scripts"))


TEST_SUNET_PATTERNS = (
    re.compile(r"^fmtest$"),
    re.compile(r"^fxtest$"),
    re.compile(r"^e2etest$"),
    re.compile(r"^scope_[0-9a-f]+$"),
    re.compile(r"^hist[0-9a-f]+$"),
)


def _is_test_sunet(s: str | None) -> bool:
    if not s:
        return False
    return any(p.match(s) for p in TEST_SUNET_PATTERNS)


def _is_test_doc(upload_id: str, data: dict) -> bool:
    if upload_id.startswith("test_"):
        return True
    fa = data.get("fa_input") or {}
    if _is_test_sunet(fa.get("payee_sunet")):
        return True
    if _is_test_sunet(data.get("filed_by_sunet")):
        return True
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", action="store_true",
                        help="actually delete (default: dry-run)")
    parser.add_argument("--include-gcs", action="store_true", default=True,
                        help="also delete matching GCS artifacts (default on)")
    args = parser.parse_args()

    from firestore_reports import _get_client as fs_client, delete_report
    from gcs_artifacts import delete_artifacts

    coll = fs_client().collection("reports")
    matches: list[str] = []
    for snap in coll.stream():
        data = snap.to_dict() or {}
        if _is_test_doc(snap.id, data):
            sunet = (data.get("fa_input") or {}).get("payee_sunet") or "—"
            filer = data.get("filed_by_sunet") or "—"
            matches.append(snap.id)
            print(f"  match  {snap.id}  payee={sunet}  filer={filer}")

    print(f"\n{'WOULD DELETE' if not args.commit else 'DELETING'} "
          f"{len(matches)} doc(s)")

    if args.commit:
        for upload_id in matches:
            try:
                delete_report(upload_id)
                if args.include_gcs:
                    delete_artifacts(upload_id)
            except Exception as err:  # noqa: BLE001
                print(f"  ERROR deleting {upload_id}: {err!r}",
                      file=sys.stderr)
        print("done.")
    else:
        print("(re-run with --commit to actually delete)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
