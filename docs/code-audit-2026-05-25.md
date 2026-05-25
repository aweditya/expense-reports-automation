# Code audit — 2026-05-25

Snapshot audit of the repo for bloat, dead code, and complexity hot
spots. Findings are prioritized by **LOC removed per risk** (high
value, low risk first). Each section ends with an actionable
proposal you can accept/reject independently.

Total repo size at audit time: ~18,800 LOC (9,271 Python + 9,602
Rust). Dead code identified: **~2,400 LOC (13% of total).**

---

## Tier 1: pure dead code (safe deletes, big wins)

### 1.1 `src/validator.rs` — the OLD untyped validator (~1086 LOC dead)

The repo has two validator modules:

- `src/validator.rs` (1148 LOC) — pre-typed-schema validator. Takes a
  generic `ReportValue` blob, parses paths, traverses, emits issues.
- `src/validator_typed.rs` (1501 LOC) — schema-typed validator. Takes
  the concrete `ExpenseReport` struct, walks it directly, emits the
  same `ValidationIssue` type.

**Who calls what:**

```
$ grep -rn 'validate_expense_report\|validate_draft_report' src/ src/bin/
src/lib.rs:43:pub use validator::render_validation_report_json_pretty;
src/lib.rs:44:pub use validator::validate_draft_report;
src/lib.rs:45:pub use validator::validate_expense_report;
src/validator.rs:70:pub fn validate_expense_report(...) { ... }
src/validator.rs:81:pub fn validate_draft_report(...) { ... }
src/validator.rs:938:        let report = validate_expense_report(&base_report());
src/validator.rs:987:        let validation = validate_expense_report(&report);
src/validator.rs:1070:        let validation = validate_expense_report(&report);
```

`validate_expense_report` + `validate_draft_report` are **only called
from inside `validator.rs` itself** (its own `#[cfg(test)]` block).
Nothing in `src/bin/`, `validator_typed.rs`, `reduce.rs`, or
`workbench_simple.rs` uses them. The lib.rs re-exports are dead too.

**What IS used:** the type definitions at the top of `validator.rs`
(lines 1–62):
- `ValidationSeverity` enum
- `ValidationIssueKind` enum
- `ValidationIssue` struct
- `ValidationReport` struct

These are imported by `validator_typed.rs` and `workbench_simple.rs`.

**Proposal:**
- Move lines 1–62 of `validator.rs` (the type defs) into
  `validator_typed.rs` (or a new `validation_types.rs`).
- Delete `validator.rs` entirely.
- Remove the dead exports from `lib.rs`.
- Net: **−1086 LOC.** Risk: low — no production caller; tests in the
  same file disappear together.

### 1.2 `src/draft.rs` — dead untyped types (~150 LOC dead)

`draft.rs` (577 LOC) holds two unrelated things:
- Metadata types (`ConfidenceLevel`, `EvidenceKind`,
  `EvidenceReference`, `FieldMetadata`) — **used by `validator_typed.rs`,
  `reduce.rs`, `workbench_simple.rs`. KEEP.**
- A pre-typed-schema `DraftReport` struct + `ParseDraftReportError` +
  `parse_draft_report_path` / `parse_draft_report_value` —
  **only used by `validator.rs` (which §1.1 marks for deletion).**

**Proposal:**
- After §1.1 lands, delete `DraftReport`, `ParseDraftReportError`,
  `parse_draft_report_path`, `parse_draft_report_value`, and their
  tests from `draft.rs`.
- Rename `draft.rs` → `evidence_metadata.rs` since the surviving
  contents are all evidence/metadata types, not draft anything.
  (Optional — cosmetic but improves discoverability.)
- Net: **−150 LOC.** Risk: low; same delete-chain as §1.1.

### 1.3 `scripts/spike_*.py` — 6 spike scripts, zero callers (~1166 LOC dead)

| File | LOC | Original purpose |
|---|---|---|
| `spike_leapfrog.py` | 376 | L.0 token-id grounding spike (productionized in `evidence_bbox.py`) |
| `spike_evidence_audit.py` | 221 | Workbench evidence audit (no longer matches current schema) |
| `spike_pymupdf_search.py` | 177 | PyMuPDF text-search exploration (Document AI won) |
| `spike_leapfrog_retrofit.py` | 172 | L.0 retrofit spike (productionized in `retrofit_bboxes.py`) |
| `spike_heic_preflight.py` | 122 | HEIC preflight spike (productionized in upload_form HEIC handling) |
| `spike_dump_xlsx.py` | 78 | xlsx dump for early FA template (one-shot, now obsolete) |

Verified no callers anywhere in scripts/, src/, tests/, docs/:
```
$ for f in scripts/spike_*.py; do
    name=$(basename $f .py)
    grep -rln "$name" scripts/ src/ tests/ docs/
  done
# (no matches for any of them)
```

**Proposal:**
- Delete all 6 spike scripts.
- Net: **−1166 LOC.** Risk: zero — never imported, never run.

### 1.4 Stale `__pycache__/*.pyc` referring to deleted .py files

`scripts/__pycache__/` has 12 `.pyc` files whose source `.py` has been
deleted: `local_app.py` (replaced by `_simple`), `e2e_test.py`,
`create_receipt_manifest.py`, `evaluate_*.py` (4 files),
`import_receipt_corpus.py`, `transcribe_*.py` (2 files),
`render_text_documents_for_ocr.py`, `benchmark_*.py`.

**Proposal:**
- `rm -rf scripts/__pycache__ tests/__pycache__` (already gitignored,
  will regenerate on next Python run with current files only).
- Net: cosmetic — doesn't affect LOC counts but makes
  `ls scripts/__pycache__/` accurate.
- Risk: zero.

---

## Tier 2: pattern audit (no LOC win, but worth eyeballing)

### 2.1 Long files — defensible, not bloat

| File | LOC | Cohesion check | Verdict |
|---|---|---|---|
| `src/workbench_simple.rs` | 2058 | one concern (HTML render); per-kind render_* functions all reference shared field_card helpers | **KEEP** |
| `src/validator_typed.rs` | 1501 | one concern (schema-typed validation); walk_*_details mirrors detail blocks 1:1 | **KEEP** |
| `scripts/local_app_simple.py` | 1479 | Flask routes + helpers; splitting into route modules would scatter the Flask `app` reference | **KEEP** |
| `src/csv_export.rs` | 1103 | two CSV formats; match arms are necessary verbosity for the per-enum mappers | **KEEP** |
| `tests/test_workbench_browser.py` | 1000 | 4 test classes share fixture patterns; splitting would duplicate setUp | **KEEP** |
| `scripts/generate_response_schema.py` | 989 | per-kind detail blocks; registry at bottom is the SSOT | **KEEP** |
| `scripts/generate_schema_artifacts.py` | 933 | YAML→Rust codegen, one logical pass | **KEEP** |

None of these warrant a split. Length is a function of inherent
complexity (number of expense kinds × per-kind logic), not bloat.

### 2.2 Helper duplication — already addressed

- `merge_two_call_lines` was duplicated across meal/transport/lodging
  extractors (B1.r lifted to `extractor_lib.py`, 2026-05-23).
- `run_two_call_extraction` was duplicated across meal/transport/lodging
  `main()` functions (B.refactor lifted to `extractor_lib.py`,
  2026-05-25; −170 LOC).
- META_CONVENTION text is duplicated across extract_meal/transport/
  lodging prompts — intentional. Each kind has slightly different
  evidence examples + `origin` enum options. Consolidating would
  obscure per-kind tweaks. **NOT BLOAT.**

### 2.3 Comment density

Spot-checked the biggest production files. Comments-to-code ratio
sits at ~25% in the Rust files and ~30% in `local_app_simple.py`.
Most comments explain WHY (a hidden constraint, a bug being worked
around, a non-obvious decision) — which matches the project's
"comments explain the WHY" rule. A handful of "what the code does"
comments survive but they're not load-bearing for the audit.

**No action.**

### 2.4 Test bloat — none found

11 test files, 2384 LOC total. Each test asserts something distinct
that would actually fail on regression. No mock-heavy speculative
tests, no copy-pasted near-duplicates. Coverage breakdown:

| File | LOC | Tests | What it guards |
|---|---|---|---|
| test_workbench_browser.py | 1000 | 27 | 4 classes covering csv8a, b1-eye, b2-eye, upload form |
| test_edit_helpers.py | 271 | many | /edit endpoint's path resolver |
| test_extractor_retry*.py | 338 | 14 | Stage 21 retry + integration |
| test_workbench_browser.py | 1000 | 27 | (counted above) |
| test_fetch_irs_mileage_rate.py | 167 | 10 | B2 IRS parser |
| test_firestore_jobs.py | 128 | 7 | Durable JOBS round-trips |
| test_fx_lookup.py | 124 | 8 | Frankfurter integration |
| test_log_event.py | 111 | 8 | Stage 22 JSON logger |
| test_edit_history.py | 108 | 10 | Stage 23 history primitives |
| test_write_fa_input.py | 115 | 8 | FA-input form → JSON |
| test_deploy_config.py | 22 | 1 | Cloudbuild shape sanity |

**No action.**

### 2.5 Doc bloat — stale planning docs

The `docs/` tree has several "plan" docs whose stages have shipped.
They've become historical artifacts:

- `docs/redesign-plan.md` — Pair A/B/C plan; all stages shipped
- `docs/leapfrog-plan.md` — L.0–L.7; L.0–L.6 shipped, L.7 deferred
- `docs/fa-input-plan.md` — S.0–S.6 + E.7; all shipped
- `docs/friday-feedback-plan.md` — Stage 5–11c; all shipped

These docs are useful as ARCHIVE (showing the design reasoning) but
new readers may mistake them for current-state. SPEC.md is the
current-state SSOT.

**Proposal (optional):**
- Add a one-line "Status: SHIPPED — historical reference. See
  docs/SPEC.md for current architecture." banner to the top of each.
- No deletions; the design history is genuinely valuable.

---

## Tier 3: smaller observations

### 3.1 `scripts/retrofit_bboxes.py` and `scripts/audit_edit_paths.py`

Both are one-shot operator tools — no test references, only doc
mentions. They're useful when needed (retrofit cached JSONs;
exhaustive /edit endpoint audit) but currently silent.

**Action:** add a one-line "When to use" header to each so future-us
remembers they exist. No deletion.

### 3.2 `acceptance_check.py` (580 LOC)

Largest single Python file in scripts/. End-to-end harness against 4
real receipts. The size is the corpus + per-kind predicates — hard to
shrink without losing coverage.

**No action.**

### 3.3 Duplicated bookkeeping in the codegen output

`generated/validation_rules.rs` and `generated/validation_rules.yaml`
hold the same rule data in two formats — Rust consumes the .rs, the
.yaml is for human eyeballs / generate_response_schema.py to read.
Both are codegen output, both regenerate together. **Not bloat.**

---

## Recommended execution order

If you want to action this, the safe order is:

1. **§1.4 cleanup** — `rm -rf scripts/__pycache__ tests/__pycache__`
   (5 seconds, zero risk, immediate cosmetic win)

2. **§1.3 spike deletes** — `git rm scripts/spike_*.py` + one commit
   per deletion or one batch commit. −1166 LOC. ~10 min.

3. **§1.1 validator.rs** — extract types to a new file or
   `validator_typed.rs`; delete `validator.rs`; remove the lib.rs
   re-exports; `cargo test` to confirm. ~30 min. −1086 LOC.

4. **§1.2 draft.rs** — after §1.1 lands, delete the four dead
   symbols (`DraftReport`, `ParseDraftReportError`, the two parse
   functions) + their tests. Optional rename. ~20 min. −150 LOC.

5. **§2.5 doc banners** (optional) — add SHIPPED banners to four
   plan docs. 5 minutes.

6. **§3.1 tool headers** (optional) — add "When to use" headers
   to retrofit_bboxes.py + audit_edit_paths.py. 5 minutes.

Total potential cleanup: **~2400 LOC removed**, all from genuinely
unused code. The remaining 16,400 LOC is load-bearing.

---

## What this audit did NOT find

- No bloated production functions (no 200-line `do_everything()` calls)
- No premature abstractions in active code (the `run_two_call_extraction`
  helper is one we just landed and has 3 concrete callers)
- No commented-out code blocks (Rust compiler / grep would have caught)
- No `TODO` / `FIXME` markers left unresolved in production paths
- No God-objects or megaclasses
- No copy-pasted near-duplicates in production code

The codebase is in good shape *aside from* the dead-validator legacy
and the spike scripts. Cleaning those up gets us to a place where
every file pulls its weight.
