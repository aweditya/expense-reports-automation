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
- [ ] **M7. Build the new minimal workbench, deploy, validate on real
  receipts.** Re-planned 2026-05-03 around a much simpler architecture
  than the old workbench. The old workbench is *moved aside* (not
  deleted) — the new one is built fresh with no inherited complexity.

  **Locked decisions for the first deploy:**
  - Read-only display. Editable comes later — FA reviews extracted data
    on the page, copies into Stanford portal manually for now. (Eventual
    target: in-place editing of missing/incorrectly-extracted fields,
    plus a "save PDF" option for the final printable form.)
  - Synchronous request flow. ~30s/receipt × 4 = ~2 min upload-to-page.
    Browser blocks. Cloud Run's 600s timeout is plenty. (Eventual target:
    polling-based async with a status page.)
  - Render only: transaction summary, transaction lines, source documents.
    Plus general info as mostly-empty (T1 fields show as "needs FA
    input"). Skip per-diem and mileage entirely until they're populated
    by reduction.

  **What we keep from the old workbench's visual language:**
  hero header with summary cards, eyebrow/badge/panel/card hierarchy,
  per-section panels, field cards with confidence pills + evidence
  quotes + needs-review tags, issues queue with jump-to-field links.

  **What we drop:** OCR inspection pages, grounding overlays, async job
  queue UI, ledger/version/draft-revision concepts, the editable filing
  surface JavaScript, OCR comparison artifacts.

  **Sub-steps, one commit each:**

  - **M7.a — Move the old workbench aside.** Create an `old/`
    directory at the repo root. Move `src/review_workbench.rs`,
    `src/review_packet.rs`, `src/review_fa_workbench.rs`,
    `src/review_preview.rs`, `src/review_session.rs`,
    `src/review_workbench.css`, the workbench/review/ledger/feedback
    export+verify binaries, and `fixtures/workbench_regressions/` into
    it. Update `src/lib.rs` to drop the references. Confirm `cargo
    test` still passes (the moved tests no longer run; that's expected).

  - **M7.b — `src/workbench_simple.rs` — the new HTML renderer.**
    Walks the typed `ExpenseReport`, emits HTML using the existing
    visual vocabulary (eyebrow/panel/card classes). Inline CSS in the
    HTML (single self-contained file output). No JavaScript. Unit-tested
    against a sample `ExpenseReport`. ~400-600 lines including CSS.

  - **M7.c (done, commit `d8c83c2`)** — Renamed
    `src/bin/render_workbench_preview.rs` → `render_workbench_from_report.rs`
    via git mv (preserves history) and rewrote with production CLI
    flags + sensible defaults. Output moved from `.scratch/workbench_preview/`
    to `.scratch/workbench/`.

  - **M7.d.1 (done, commit `2f40b02`)** — Typed validator
    (`src/validator_typed.rs`) that walks the typed `ExpenseReport`
    directly, looks up FIELD_RULES + CONDITIONAL_RULES per path, emits
    issues. Wired into the render binary so the workbench shows real
    validation issues. Plus the workbench layout split (left rail for
    issues, sticky-positioned, hidden when no issues; center column
    for the form), bumped base font to 15px, JS-driven active-issue
    highlight on the field card with in-view-no-scroll.

  - **M7.d.2 — `scripts/local_app_simple.py` — the new HTTP server.**
    Two endpoints:
      GET  /          → upload form (one file input that accepts multiple)
      POST /upload    → save uploads → spike_extract per file (sequential)
                        → reduce_extractions → render_workbench → return HTML
    Synchronous. No job queue, no polling, no session state. ~200 lines.
    UI-validation step: run the server locally, upload the four real
    receipts via the browser, see the workbench render, click the
    issue-jump links, expand/collapse the transaction lines.

  - **M7.e — Dockerfile + cloudbuild updates.** Copy
    `scripts/spike_extract.py`, `scripts/local_app_simple.py`, the
    `generated/response_schema_meal.json`, and the new Rust binaries.
    Switch the entrypoint to `local_app_simple.py`. cloudbuild.yaml
    likely needs no changes (still cargo test + python tests + build +
    deploy).

  - **M7.f — Deploy and verify.** Push to main → Cloud Build → Cloud
    Run. Open the deployed site, upload the four real receipts via the
    actual UI, see what happens. Iterate on whatever surfaces.

  **Eventual follow-ups (NOT M7):**
  - Editable in-place fields with save endpoint
  - Poll-based async upload with status page
  - "Save as PDF" button for the FA's records
  - Per-diem rendering once the reduction populates it
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

**M8 (done): old-pipeline removal.** The redesign is the system now.
Big delete in 5 stages, each one push → trigger auto-deploys → eyeball
the live workbench:
- M8.1 (`778ca5f`): 31 obsolete binaries from `src/bin/`.
- M8.2 (`3b81a05`): 7 obsolete tests + 16 obsolete scripts.
- M8.3 (`8818467`): 29 old modules + slim `src/lib.rs` (28k-line delete).
- M8.4 (`77689de`): `old/` directory + drop `COPY old/` from Dockerfile.
- M8.5 (this commit): `fixtures/` orphans + drop `COPY fixtures/` from
  Dockerfile + rewrite the CLAUDE.md preamble to describe the system
  as it stands (no more "redesign in progress" framing).

End state: 11 Rust modules, 3 binaries, 6 scripts, 1 Python test.
Down from 32 modules, 34 binaries, 21 scripts, 9 tests. Pipeline
identical end-to-end on the deployed Cloud Run URL throughout — every
M8 stage was verified live before the next one started.

**M7.f (done): the deploy story.** Cloud Build trigger `deploy-on-push`
in us-west1 fires on push to `^main$`. (Spent half an M7.f session
building manual `scripts/deploy.sh` infra before noticing the trigger
existed — see regrets.) `scripts/deploy.sh` kept as a manual escape
hatch. Verified live on all four real receipts.

**M7.e.4 (done): cleanup before deploy gate.** Deleted
`tests/test_transcribe_with_google_genai.py` and
`tests/test_transcribe_with_document_ai.py` (M8 will delete the scripts
they cover). Added `scripts/deploy.sh`. Added three regret entries
covering the deploy-flow mistakes.

**M7.e.2/.3 (done, `3356407`):** Dockerfile rewrite: only the new
binaries; dropped perl / poppler-utils / curl / openssl; switched to
`gunicorn local_app_simple:app`; `RUST_BIN_DIR=/usr/local/bin` env
var so the script uses prebuilt binaries in-container, falls back to
`cargo run` locally. Verified end-to-end against `mjsushi.jpeg` in a
local container with mounted ADC.

**M7.e.1 (done, `1b19867`):** Dropped --service-account-key and the
on-disk JSON entirely. spike_extract.py / local_app_simple.py /
spike_acceptance_check.py all use ADC now (Cloud Run metadata server in
prod, `gcloud auth application-default login` locally). Same commit
fixes a round-trip bug exposed by M6.2.b T2-wrapping: `Wrapped::is_unknown`
+ codegen `skip_serializing_if` so Python-omitted leaves don't get
re-emitted as `{value:null,_meta:{default}}`.

**M7.d.1 (done, `2f40b02`):** typed validator + workbench layout split.
**M7.d.2 (done, `1ebce88`):** Flask HTTP server + Download JSON link.
**M7.d.3 (done):** PipelineError + @app.errorhandler renders a friendly
error page when extract/reduce/render fail.
**Codegen wraps T2 (done, `52d7fd3`):** total_usd is Wrapped now.
**Provenance text for derived fields (done, `0699db8`):** Trip Date /
Total USD / Category show human-readable provenance under the value.
**Relative-URL unification (done, `23932e0`):** renderer is
backend-agnostic; Flask serves the per-upload dir as static files;
POST/Redirect/GET means the URL is bookmarkable.

After M7.e.3: M7.f pushes to main, watches Cloud Build, and verifies
the deployed Cloud Run URL with all four real receipts (the verdict).

**Locked principles for this phase:**

1. **UI-driven validation.** Every backend change is validated by
   rendering it in the workbench and looking at it — not by cargo
   tests alone. Workbench is the local verdict; Cloud Run is the
   deploy verdict.
2. **Keep things simple.** No async jobs, no editable inputs yet,
   no "save as PDF." The full feature set comes in subsequent
   milestones.
3. **Slow down on visual iteration.** When changing UI: re-read the
   diff, ask "did I leave any stale state that could trigger the old
   behavior?", trace through what the user is about to see, before
   sending a refresh-and-look message. Codified after M7.d.1's UI
   mistakes (see regrets).

**M7.a (done, commit `a5566f2`):** Old workbench moved to `old/`.
**M7.b (done, commit `a035907`):** New HTML renderer + CSS.
**M7.c (done, commit `d8c83c2`):** render_workbench_from_report binary.
**M7.d.1 (done, commit `2f40b02`):** Typed validator + layout split +
active-issue highlight.

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
