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
- [ ] **M5. Harden the extractor (Python).** Address the prompt + schema
  enforcement issues from the M4 review now, before designing Rust against
  imperfect inputs. Four sub-steps, one commit each:
  - **M5.1 Schema generator.** `scripts/generate_response_schema.py` reads
    `schema.yaml` and emits `generated/response_schema_meal.json` — the
    SDK-ready response schema for a list of meal transaction lines.
    Meal-only because that's all our corpus covers; other kinds get added
    when receipts of those kinds appear.
  - **M5.2 Wire `response_schema` into the extractor.** Modify
    `scripts/spike_extract.py` to load the generated schema, pass it as
    `response_schema` in the SDK call, and drop the inline schema
    description from the prompt. Verify against the three real receipts:
    structural fixes (always-array wrapping, all required fields) should
    land on first run. If the SDK rejects our schema or returns errors,
    fall back to keeping `response_mime_type` only and document in the
    regrets log.
  - **M5.3 Tighten the prompt.** With shape guaranteed, the prompt
    focuses on reasoning rules: when to use `group_*` variants, where to
    look for `tip_amount`, evidence-kind discipline for null values,
    confidence calibration. Re-run on three receipts; each issue from
    the M4 review either fixed or accepted with rationale.
  - **M5.4 Acceptance harness.** `scripts/spike_acceptance_check.py`
    runs the extractor on the three receipts and asserts the ~6-8
    fields-that-matter per receipt. Manual-run script (no Cloud Build
    integration). Pass/fail per receipt, summary line.
- [ ] **M6. Reduction step (Rust).** New module: list of typed lines (read
  from the JSON files Python wrote) → `general_information` block +
  `transaction_summary` + `per_diem_expenses`. Pure functions. Commit.
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

**M5.1 (next).** Schema generator: `scripts/generate_response_schema.py`.
Reads `schema.yaml`, emits `generated/response_schema_meal.json` containing
a JSON-Schema-shaped object the Google Gen AI SDK can use as its
`response_schema`. Meal-only. Verifies the output parses as valid JSON.
No Gemini call yet.

After M5.1: M5.2 wires the generated schema into the extractor and re-runs
on the three receipts to confirm structural fixes land on the first try.

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
