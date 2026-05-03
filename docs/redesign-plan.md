# Pipeline Redesign — Plan of Action

Living document. Updated as work progresses. The current step is marked.

## Goal

Replace the current multi-layer extraction pipeline (transcribe → classify →
per-kind extract → bundle synthesis → projection) with a thinner one centered
on a single Gemini call per document that returns a transaction line typed
against `schema.yaml`.

End state: file → Gemini (typed transaction line) → reduction → schema-typed
report → validation → workbench. Six modules. The schema is the spine.

## Language split (decided 2026-05-02)

- **Python owns extraction.** One module that calls the Google Gen AI SDK
  with structured output typed against the schema, returns typed JSON. The
  SDK is Python-only; no REST hand-rolling, no Rust subprocess wrapper.
- **Rust owns everything downstream.** Schema-typed model, reduction,
  validation, ledger, workbench rendering. This is where the type system
  pays for itself — discriminated-union per-line types, generated
  `FIELD_RULES` and `CONDITIONAL_RULES`, bit-stable validation.
- **Boundary: typed JSON file on disk.** Python writes it, Rust reads it.
  If the extractor ever swaps (Claude, GPT-4o, local VLM), only Python
  changes.

## Workflow rules (locked)

- Plan first, act second. Update this doc as work progresses.
- No code bloat. No "for the future" scaffolding unless explicitly asked.
- Frequent commits, one logical change per commit.
- No inline shell scripts. Real files in `scripts/`.
- Never write to `/tmp`. Use this project directory.
- CLI testing is sanity-only. The deployed Cloud Run site is the source of truth.
- Track mistakes in `docs/redesign-regrets.md` so they don't recur.

## Post-M4 decisions (locked 2026-05-02)

- **Prompt-tier fixes belong in M6**, not blocking M5: array-vs-object
  bracketing, weak expense_type inference, missed tips. The JSON shape is
  stable enough to design the reduction against now.
- **`country_of_activity` over-fill for domestic is fine.** Pass 1
  (Gemini) reads what's on the receipt. Pass 2 (Rust `CONDITIONAL_RULES`)
  decides whether the value was required given `category`. Don't fight
  Pass 1 about Pass 2's job.
- **Keep thinking tokens enabled.** A future ReAct-style agentic loop
  (extract → validate → revise based on validation feedback → repeat) is
  the likely direction, and thinking is what makes that work. Budget
  generously (32k+).

### Architecture flag for M5/M6 design

The one-shot extractor (M4) writes one JSON file and exits. An agentic
extractor needs to read back validation issues from Rust and re-call
Gemini with that context. Boundary design choice we don't need to make
yet but should not lock out: Python emits one final JSON OR Python
imports/calls Rust validation between iterations. Cleanest is probably
"Python emits one final JSON, but the loop happens inside Python by
calling a Rust binary that returns validation issues as JSON." Defer
the decision until M6.

## Schema decisions (made; pending edits in M2)

| Decision | Status |
|---|---|
| `payment_method` — keep, no change | done |
| `transaction_type` — keep, FA fills manually (mirrors category but separate field) | edit |
| `transaction_date` — date of the expense, not filing date | edit |
| `status` — keep in schema, FA fills | edit (no change but clarify) |
| `mileage_expenses` — keep empty placeholder | done |
| `_meta` — promote to spec'd structure, ordinal confidence (high/medium/low), enumerated flags vocab | edit |
| `business_purpose.{who,what,when,where,why,key_30char}` — FA enters manually (T1) | edit |
| `source_documents` — change source tier T1 → T3 | edit |
| Approver fields | deferred — pending FA conversation |
| `lab_name` / `advisor` references | deferred — business_purpose is manual now |

## Milestones

- [x] **M0. Merge `feature/async-ocr-jobs` to main.** Done — fast-forward, both
  test suites passed locally before push.
- [x] **M1. Workflow scaffolding.** This document + regrets doc, committed on
  `redesign/single-call-extraction`.
- [x] **M2. Schema YAML edits.** Seven edits applied: business_purpose +
  key_30char → T1, transaction_type → T1, transaction_date description
  clarified, status → T1, source_documents → T3, `_meta` promoted to a
  top-level `_meta_convention` block (sibling of `expense_report`, ignored by
  the generator). YAML validated.
- [x] **M3. Regenerate model + rules.** Regenerated; cargo green at 201 lib
  tests + ancillary, Python green at 182. Two commits: (a) regenerated
  artifacts + readiness cleanup (dropped the obsolete deferred-derived
  special cases for key_30char/transaction_type, those are now plain T1
  user-input fields), (b) refreshed ledger/review/workbench regression
  fixtures via the existing export binaries.
- [x] **M4. Spike: single Gemini call → typed transaction line.** Done.
  `scripts/spike_extract.py` calls Gemini with the schema vocabulary
  inlined in a plain-text prompt (no `response_schema`). All three real
  receipts in `receipts/` produced valid, schema-shaped JSON in
  `.scratch/spike/`. Verdict in `.scratch/spike/REVIEW.md`: proceed to
  M5. Issues to address before production: array-vs-object wrapping
  inconsistency, occasional weak `expense_type` inference (mels1 picked
  group_travel from a "GST" tax label), `tip_amount` missed on one,
  `country_of_activity` filled when it should be null for domestic, and
  `gemini-3-flash-preview` thinking tokens count toward
  `max_output_tokens` (had to raise to 32768).
- [x] **M5. Harden the extractor (Python).** Done in four commits:
  - **M5.1 (`048faf9`)** — `scripts/generate_response_schema.py` emits
    `generated/response_schema_meal.json` from the meal slice of
    `schema.yaml`. Hand-built (not a generic converter), only the
    expense_type enum is pulled in from the YAML.
  - **M5.2 (`641d8a5`)** — Spike extractor wired up to the generated
    schema. Inline schema description dropped from the prompt; structural
    fixes landed on first try (always-array wrapping, all required fields
    present, every leaf wrapped). One regression introduced (model started
    filling `original_amount` with the USD line value), addressed in M5.3.
  - **M5.3 (`441ec0f`)** — Prompt rewritten as reasoning rules:
    `business_meal` vs `business_meal_with_alcohol` routing, where to find
    tip, when to null `original_currency`/`original_amount`, evidence-kind
    discipline for null values, confidence calibration. Re-ran on three
    receipts: original_amount/currency now correctly null with
    `not_applicable_for_domestic` origin; tamarine still nails everything;
    mels1 expense_type stable on `business_meal_with_alcohol`.
  - **M5.4** — `scripts/spike_acceptance_check.py` runs by default against
    the existing JSON outputs (no Gemini call) and asserts ~7 fields per
    receipt: date, total, original_currency/amount nullness, expense_type
    (with tolerance for the alcohol/non-alcohol variant on borderline
    cases), venue (substring), has_alcohol_on_receipt. With `--run` it
    re-invokes the extractor first (3 Gemini calls). All three receipts
    PASS.
- [ ] **M6. Reduction step (Rust).** Two phases:

  **M6.1 — Wrap leaves in `Wrapped<T>` so Python's JSON deserializes
  directly into the generated Rust types.** Removes the duplication risk
  of hand-writing parallel `ExtractedTransactionLine` types. The schema's
  `_meta_convention` exists precisely for this; the codegen has been
  ignoring it. Provenance flows end-to-end after this lands. Sub-commits:
    - **M6.1.a** Add `Wrapped<T>` and `Meta` types in a new `src/meta.rs`.
      Hand-written, ~80 lines. Unit tests for round-trip and default-meta.
    - **M6.1.b** Modify `scripts/generate_schema_artifacts.py` to wrap
      every leaf field's Rust type as `Wrapped<T>`. Regenerate. `cargo
      build` should pass; many tests will then fail to compile.
    - **M6.1.c** Fix every compiler error across the Rust codebase. Two
      patterns: reads add `.value`, constructions wrap with
      `Wrapped::known()`. Touches `validator.rs`, `bundle_synthesis.rs`,
      `draft.rs`, all tests, etc. If the diff balloons past ~20 files,
      split into per-module sub-commits. End state: `cargo test` and
      `python3 -m unittest discover -s tests` both green.
    - **M6.1.d** Refresh ledger/review/workbench regression fixtures via
      the export binaries (same machinery as M3).
    - **M6.1.e** Verify Python's spike output (`.scratch/spike/*.json`)
      deserializes into the regenerated transaction-line type. This is
      the proof that the duplication gap closed — same Rust type for
      both extracted-from-Python and report-state.

  **M6.2 — Reduction function over `Vec<ExpenseReportTransactionLine>`.**
  A small library of named reductions (sum, earliest, foreign-presence)
  in a single `src/reduce.rs`. Aggregates per-bundle into a complete
  `ExpenseReport`. Per-diem expansion deferred. Plus a binary
  `src/bin/reduce_extractions.rs` that reads a directory of JSON files
  and writes one report. Acceptance harness extended to exercise the
  end-to-end Python -> Rust path on the four real receipts.
- [ ] **M7. Wire into workbench, deploy, validate on real receipts.** Make the
  workbench render the new typed report. Strip the parts that depend on the
  old pipeline. Deploy to Cloud Run. **Verdict from the deployed site on the 3
  receipts.** Iterate.
- [ ] **M8. Delete the old pipeline.** Once the new path works on real
  receipts via the deployed site, delete:
  - `src/vertex_gemini.rs` (REST client) and `src/vertex_gemini_sdk.rs`
    (Rust→Python subprocess wrapper) — both made obsolete by the
    Python-owned extraction.
  - `IngestionTranscriber` enum and its branches in `ingest.rs`.
  - `OcrPassKind`, `OcrPreprocessVariant`, `OcrGeometrySource`,
    `OcrRegionKind` types in `transcribe.rs` (and likely most of
    `transcribe.rs`).
  - `src/document_extract.rs`, the keyword classifier, the per-kind
    extractors.
  - `src/ocr_compare.rs`, `src/ocr_grounding.rs`, `src/ocr_inspection.rs`.
  - The synthetic-corpus modules (`synthetic_corpus.rs`,
    `synthetic_documents.rs`, `corpus_eval.rs`) and their export/eval
    binaries.
  - The OCR-pass-comparison and image-preprocessing machinery in
    `scripts/transcribe_with_google_genai.py` (the file collapses to
    a thin wrapper, ~200-300 lines).
  - Update CLAUDE.md scope. Commit per deletion group.

## Current step

**M6.1.a (next).** Add `src/meta.rs` with `Wrapped<T>`, `Meta`,
`Confidence`, `EvidencePtr`, `EvidenceKind`. Hand-written, serde-derived,
defaults on `Meta` so no-meta JSON still deserializes. Unit tests for
round-trip + default-meta + `Wrapped::known()` constructor. No other
code touched yet.

After M6.1.a: M6.1.b modifies the codegen to wrap leaves; expect a
medium-sized regenerated `expense_report_model.rs` diff and a much
larger downstream "fix all callers" sub-commit (M6.1.c) where the
compiler enumerates the work.

## Mistakes I'm watching for during M5

- Generating schema for expense kinds we don't have receipts of. We can't
  validate them. Build only meal for now.
- Building a generic JSON-Schema → SDK-Schema converter when one shape is
  all we need. Hand-write the conversion.
- Adding "for the future" prompt rules about expense kinds we haven't seen.
- Re-piping `cargo test` through `tee` and reading `$?` (M3 mistake).
- Writing anything to `/tmp` (M3 mistake).

## Open questions for the FA

- Approver fields in `allocation_and_approvers` — what fields exist?
- Does the Stanford portal store `status`? (We're keeping it in our schema and
  asking the FA to fill it.)
- Per-diem rates — does the portal compute these, or do we submit them?
- `lab_name` / `advisor` source — where do these come from in the FA's workflow?
