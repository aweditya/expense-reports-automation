# Pipeline Redesign — Plan of Action

Living document. Updated as work progresses. The current step is marked.

## Goal

Replace the current multi-layer extraction pipeline (transcribe → classify →
per-kind extract → bundle synthesis → projection) with a thinner one centered
on a single Gemini call per document that returns a transaction line typed
against `schema.yaml`.

End state: file → Gemini (typed transaction line) → reduction → schema-typed
report → validation → workbench. Six modules. The schema is the spine.

## Workflow rules (locked)

- Plan first, act second. Update this doc as work progresses.
- No code bloat. No "for the future" scaffolding unless explicitly asked.
- Frequent commits, one logical change per commit.
- No inline shell scripts. Real files in `scripts/`.
- Never write to `/tmp`. Use this project directory.
- CLI testing is sanity-only. The deployed Cloud Run site is the source of truth.
- Track mistakes in `docs/redesign-regrets.md` so they don't recur.

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
- [ ] **M4. Spike: single Gemini call → typed transaction line.** New module that
  takes one image and calls Gemini with structured output typed against the
  schema's `transaction_lines` discriminated union. Sanity-test from CLI on
  `receipts/mels1.jpeg`, `mels2.jpeg`, `tamarine.png`. No reduction, no
  validator, no workbench wiring. Commit.
- [ ] **M5. Reduction step.** New module: list of typed lines →
  `general_information` block + `transaction_summary` + `per_diem_expenses`.
  Pure functions. Commit.
- [ ] **M6. Wire into workbench, deploy, validate on real receipts.** Make the
  workbench render the new typed report. Strip the parts that depend on the
  old pipeline. Deploy to Cloud Run. **Verdict from the deployed site on the 3
  receipts.** Iterate.
- [ ] **M7. Delete the old pipeline.** Once the new path works on real
  receipts via the deployed site, delete `document_extract.rs`, the keyword
  classifier, the per-kind extractors, the synthetic-corpus modules, the
  OCR-pass-comparison code, the grounded-region helpers. Update CLAUDE.md
  scope. Commit per deletion group.

## Current step

**M4 (next).** Spike a single-Gemini-call extractor against the schema's
transaction_lines discriminated union, run it from the CLI on the three real
receipts in `receipts/`. No reduction, no validator, no workbench yet — just
prove the structured-output approach returns sensible typed rows for real
documents.

## Open questions for the FA

- Approver fields in `allocation_and_approvers` — what fields exist?
- Does the Stanford portal store `status`? (We're keeping it in our schema and
  asking the FA to fill it.)
- Per-diem rates — does the portal compute these, or do we submit them?
- `lab_name` / `advisor` source — where do these come from in the FA's workflow?
