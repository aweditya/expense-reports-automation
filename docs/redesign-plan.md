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

## Per-receipt JSON shape — Architecture B (locked 2026-05-03)

The per-receipt JSON the extractor writes has TWO compartments:

1. **Schema-shaped fields.** `expense_kind`, `common`, and the matching
   detail block (`meal_details` for now). These map 1:1 to
   `ExpenseReportTransactionLinesItem` and flow into the final report
   unchanged. T3 fields are `Wrapped<T>` (carry _meta provenance from
   the document); T1/T2 fields stay bare (FA-input or system-derived,
   no document quote needed).

2. **`extras` block.** Things the model reads off the receipt that are
   useful to reduction or validation but DON'T appear in `ExpenseReport`.
   Initial fields: `merchant_address`, `printed_currency`. More added
   as reduction needs them. Same `_meta` discipline (provenance still
   matters); they just live at the per-receipt layer only.

The schema (`schema.yaml`) is NOT polluted with these extras. It stays
focused on what the FA portal stores. Two shapes, one source of truth
for the report; the per-receipt JSON is a richer thing that the
extractor produces and reduction consumes.

Why we did this:
- We never get to re-read a receipt without another Gemini call. Capture
  what's there now; let reduction decide what to use.
- Reduction has real work that needs the extras (FX conversion needs
  printed_currency, foreign/domestic decision needs merchant_address).
- The schema's discipline matters — it mirrors the portal. The day
  someone says "is `merchant_address` required for submission?" we want
  the answer to be obvious from the schema, not "well it depends on
  what the extractor happened to grab."

## Decisions locked 2026-05-03 (drive M6.2 work)

1. **Money is `f64`.** The YAML says `type: number`. The codegen
   currently maps `number` → `DecimalAmount(String)` (string-based,
   chosen to dodge float precision). Reverting to `f64`: this domain
   doesn't have the magnitudes where IEEE 754 precision bites. M6.2
   reverts the codegen and the response_schema in lockstep.

2. **Drop `source_documents` and `attendees` from the per-receipt
   response_schema.** `source_documents` is filename context the system
   provided (no value re-emitting). `attendees` is T1 (FA fills later).
   Both still exist in `ExpenseReportTransactionLinesItem` so the full
   report can carry them; the per-receipt JSON just doesn't emit them.
   With `#[serde(default)]` on those fields, deserialization works.

3. **Codegen wraps by source tier, not blindly by leaf-ness.**
   - T3 fields → `Wrapped<T>` (extracted from documents; provenance
     matters)
   - T1 fields → bare `T` or `Option<T>` per required-ness (FA input;
     "the FA typed it" is the provenance, no document quote needed)
   - T2 fields → bare `T` or `Option<T>` (system-derived; provenance
     is "computed by step X")

   This dissolves the deeply-nested-wrapping problem with
   source_documents items naturally.

4. **`extras` block is per-receipt only.** Lives in the Python output's
   JSON, not in the schema. Rust side: the per-receipt deserialization
   target is a wrapper struct, e.g. `ExtractedReceipt { line:
   ExpenseReportTransactionLinesItem, extras: Extras }`. Reduction reads
   both. The final `ExpenseReport` only carries `line` (extras don't
   flow downstream past reduction).

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

  **M6.2 — Per-receipt cleanup + reduction function (Architecture B).**

  Sub-steps, one commit each:

  - **M6.2.a Revert money to `f64`.** Codegen `number` → `f64` (drop
    DecimalAmount string-newtype). Response_schema money fields back to
    `number` (drop the string description). Acceptance-harness
    expectations back to numeric literals.
  - **M6.2.b Codegen wraps by source tier.** T3 leaves → `Wrapped<T>`;
    T1/T2 leaves → bare `T` or `Option<T>` per required-ness. Eliminates
    the deeply-nested wrapping problem (source_documents/attendees items
    now have bare fields and Gemini accepts that shape natively).
  - **M6.2.c Drop source_documents and attendees from the per-receipt
    response_schema.** They stay in the schema's full report struct, but
    the extractor doesn't emit them. `serde(default)` handles the absence
    on deserialize.
  - **M6.2.d Add the `extras` block to the per-receipt JSON.** Initial
    fields: `merchant_address`, `printed_currency`. Hand-written
    response_schema additions; same `_meta` discipline. Add a Rust
    `Extras` struct + `ExtractedReceipt { line: ExpenseReportTransactionLinesItem,
    extras: Extras }` wrapper for deserialization. Update the unit test
    in src/meta.rs to deserialize the new shape.
  - **M6.2.e Reduction library.** `src/reduce.rs` with named reductions
    (sum, earliest, foreign_presence_from_extras, etc.). Aggregates a
    `Vec<ExtractedReceipt>` into a complete `ExpenseReport`. Per-diem
    expansion deferred. Unit-tested.
  - **M6.2.f Reduction binary.** `src/bin/reduce_extractions.rs` reads
    `.scratch/spike/*.json`, calls reduce, writes one `ExpenseReport` to
    `.scratch/reduced/report.json`. Acceptance harness extended with a
    new `--end-to-end` mode that runs extract → reduce → asserts the
    aggregated report has 4 lines, total = sum, etc.
- [x] **M6.5 — Hardening pass.** Done as one focused round-trip check
  (`src/bin/roundtrip_check.rs`) plus the fixes it surfaced.
  - Surfaced: Rust was *adding* fields on serialize that Python omitted
    (Option::None → null, empty Vec → []). ~50 mismatches per file.
  - Fixed by emitting `skip_serializing_if = "Option::is_none"` /
    `"Vec::is_empty"` from the codegen, and on `EvidenceReference`'s 5
    pre-codegen Option fields in `src/draft.rs`.
  - Acceptance harness now runs the round-trip check by default (no
    Gemini cost). 4/4 PASS.
  - Skipped explicitly: M6.5.b (response_schema validator — Gemini
    fails loudly enough at runtime); M6.5.c (recorded responses —
    flake bound by predicate tolerance); M6.5.d (_meta codegen —
    hand-written is fine).
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

**M7 (next).** Wire the new pipeline into the workbench, deploy to
Cloud Run, validate on the four real receipts via the deployed site.

The redesigned pipeline is end-to-end working at the CLI:
  receipts/*.{jpeg,png} → spike_extract.py (Gemini structured output)
  → .scratch/spike/*.json → reduce_extractions binary
  → .scratch/reduced/report.json (typed ExpenseReport)

What M7 needs:
- An adapter from the new ExpenseReport into whatever the workbench
  renderer consumes today (likely DraftReport — needs a From impl, or
  the workbench reader gets rewritten to take ExpenseReport directly).
- The local app's job runner (scripts/local_app_job_runner.py) needs to
  invoke the new spike_extract.py + reduce_extractions binary instead
  of the old transcribe → ingest path.
- Deploy via Cloud Build push to main.
- Cloud Run verdict on the real receipts.

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
