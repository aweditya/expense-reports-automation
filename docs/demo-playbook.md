# Demo Playbook

This guide maps each major completed deliverable to a short, runnable demo.

For each section:

- run the command or steps exactly as written
- use the “what to show” note to know what part of the output to highlight
- use the “what it proves” line as your verbal framing

## Before Any Demo

Run these once:

```bash
cargo test
python3 -m unittest discover -s tests
```

What to say:

- “The Rust core pipeline and the Python harnesses are both covered by automated tests.”

## 1. Schema Model And Validator

What it proves:

- the repo has a typed schema contract and a working runtime validator

Run:

```bash
cargo run --bin validate_report -- \
  examples/minimal_report.yaml
```

Then:

```bash
cargo run --bin validate_draft_instance -- \
  examples/draft_instance.yaml
```

What to show:

- the plain schema-shaped report validates
- the evidence-bearing draft instance also validates
- mention that the second form includes per-field confidence and evidence metadata

## 2. Schema-Derived Artifact Generation

What it proves:

- `schema.yaml` is the single source of truth and code/artifacts are generated from it

Run:

```bash
python3 scripts/generate_schema_artifacts.py
```

What to show:

- [generated/expense_report_model.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/generated/expense_report_model.rs:1)
- [generated/validation_rules.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/generated/validation_rules.rs:1)
- [generated/ui_field_map.yaml](/Users/adityasriram/Labs/stanford/research/expense-reports/generated/ui_field_map.yaml:1)

Suggested talking point:

- “The schema is not just documentation; it compiles into runtime contracts.”

## 3. Per-Document Fact Extraction

What it proves:

- the system can parse OCR/markdown output into typed document facts

Run one receipt example:

```bash
cargo run --bin extract_document_facts -- \
  fixtures/curated/receipt/receipt_card_dotted.md
```

Optionally run a hotel folio too:

```bash
cargo run --bin extract_document_facts -- \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md
```

What to show:

- the extracted JSON has a document kind
- individual fact values are typed and carry confidence/evidence

Deep proof:

```bash
cargo run --bin verify_curated_corpus
```

Use this if someone asks whether the extractors are regression-tested.

## 4. Bundle Synthesis And Schema Projection

What it proves:

- multiple documents get merged into one canonical expense bundle and then projected into a schema draft

Run:

```bash
cargo run --bin synthesize_bundle_from_facts -- --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

What to show:

- the readiness summary
- the remaining blocked fields are the human-input ones, not random parser failures

If you want the raw canonical bundle:

```bash
cargo run --bin synthesize_bundle_from_facts -- --output bundle-json \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

If you want the projected draft as JSON:

```bash
cargo run --bin synthesize_bundle_from_facts -- --fx demo --output draft-json \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

## 5. Readiness Classification

What it proves:

- the system distinguishes automation failures from expected user input and manual review

Run:

```bash
cargo run --bin synthesize_bundle_from_facts -- --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

What to show:

- `automation gap(s)`
- `user input gap(s)`
- `manual review item(s)`

Suggested talking point:

- “Readiness is how we keep the FA from treating every issue as the same kind of problem.”

## 6. FA Review Packet And Workbench

What it proves:

- the pipeline produces a copy-friendly FA-facing surface instead of only raw JSON/YAML

Generate the workbench HTML:

```bash
cargo run --bin build_review_workbench_from_facts -- --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json \
  > /tmp/review_workbench_demo.html
```

What to show:

- open `/tmp/review_workbench_demo.html`
- summary cards
- issue queue
- copy buttons
- evidence panel
- attachment checklist

Regression proof:

```bash
cargo run --bin verify_workbench_regressions
```

## 7. Feedback Capture

What it proves:

- the system can compare machine output with corrected output and store the delta as structured feedback

Run:

```bash
cargo run --bin capture_feedback_from_drafts -- \
  --original examples/feedback_original.yaml \
  --corrected examples/feedback_corrected.yaml \
  --annotations examples/feedback_annotations.yaml \
  --site-feedback examples/feedback_submission.yaml \
  --validation examples/feedback_validation.json
```

What to show:

- changed fields are labeled as added, updated, or cleared
- correction reasons and site-return context are preserved

Regression proof:

```bash
cargo run --bin verify_feedback_regressions
```

## 8. Review/Submission Ledger

What it proves:

- the system has a durable versioned history of machine drafts, FA revisions, submissions, and returns

Initial workflow demo:

```bash
cargo run --bin build_review_submission_ledger_from_facts -- --fx demo --scenario initial \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Returned-and-corrected workflow demo:

```bash
cargo run --bin build_review_submission_ledger_from_facts -- --fx demo --scenario returned \
  fixtures/curated/flight_itinerary/airline_itinerary_trip_window.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_property_labeled.md.expected.json \
  fixtures/curated/receipt/receipt_merchant_labeled.md.expected.json
```

What to show:

- there are explicit draft versions
- submissions are tied to versions
- returned cases create a new correction path instead of mutating history in place

Regression proof:

```bash
cargo run --bin verify_ledger_regressions
```

## 9. Managed Ingestion Workspace

What it proves:

- uploads, normalization artifacts, and run history are persisted under one bundle id

Run:

```bash
cargo run --bin ingest_bundle_workspace -- \
  stage-and-run \
  --workspace-root /tmp/expense_workspace_demo \
  --bundle-id demo_bundle \
  --user-id presenter \
  --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md \
  fixtures/curated/receipt/receipt_card_dotted.md
```

Then inspect:

```bash
cargo run --bin ingest_bundle_workspace -- \
  status \
  --workspace-root /tmp/expense_workspace_demo \
  --bundle-id demo_bundle \
  --format json
```

What to show:

- `uploads/`
- `normalized/`
- `runs/<run_id>/artifacts/`
- `bundle_manifest.json`

## 10. Local FA Upload App

What it proves:

- the pipeline can be driven through a browser workflow, not only through CLIs

Run:

```bash
python3 scripts/local_app.py \
  --workspace-root /tmp/expense_local_app_workspace \
  --port 8765
```

In the browser:

1. open `http://127.0.0.1:8765`
2. set `Engine` to `builtin`
3. set `FX Mode` to `demo`
4. leave `Run ID` blank
5. upload:
   - `fixtures/curated/flight_itinerary/airline_itinerary_classic.md`
   - `fixtures/curated/hotel_folio/hotel_folio_guest_bill.md`
   - `fixtures/curated/receipt/receipt_card_dotted.md`
6. click `Run Intake Pipeline`

What to show:

- redirect to `/bundle/<bundle_id>`
- embedded review workbench
- pending-upload accumulator in the form
- re-uploading to the same bundle does not overwrite previous runs

## 11. Gemini OCR Integration

What it proves:

- the live OCR boundary works against Vertex Gemini 3 and feeds the downstream pipeline

Fastest reliable demo:

```bash
bash scripts/run_gemini_smoke_test.sh --packets 2
```

What to show:

- the script emits:
  - single-document OCR JSON
  - end-to-end ingestion workbench
  - OCR evaluation report
- the evaluation summary shows document counts and exact/content matches

If you want only isolated OCR on one rendered document:

```bash
python3 scripts/render_text_documents_for_ocr.py \
  --output-dir /tmp/gemini_single_doc \
  --format png \
  fixtures/curated/receipt/receipt_card_dotted.md

cargo run --bin transcribe_document -- \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --model gemini-3-flash-preview \
  --sdk-python /tmp/expense_report_genai_venv/bin/python \
  --format json \
  /tmp/gemini_single_doc/receipt_card_dotted.png
```

## 12. Corpus And Stress Testing

What it proves:

- the system is not only demoable on one packet; it has regression and scale harnesses

Synthetic OCR evaluator:

```bash
python3 scripts/evaluate_synthetic_ocr_corpus.py \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --sdk-python /tmp/expense_report_genai_venv/bin/python \
  --output-dir /tmp/expense_ocr_eval_demo \
  --packets 4 \
  --model gemini-3-flash-preview
```

Workspace stress harness:

```bash
python3 scripts/evaluate_workspace_pipeline.py \
  --output-dir /tmp/workspace_stress_demo \
  --engine builtin \
  --packets 16
```

What to show:

- generated reports under the output directory
- packet counts
- match counts
- filing-state counts
- zero-failure summaries when the run is clean

## Presentation Shortcut

If you need the shortest live sequence that shows the whole story:

1. `cargo test`
2. `python3 scripts/local_app.py --workspace-root /tmp/expense_local_app_workspace --port 8765`
3. show the browser upload flow with the curated markdown packet
4. `bash scripts/run_gemini_smoke_test.sh --packets 2`

That covers:

- typed schema and validation
- extraction
- bundle synthesis
- readiness
- FA workbench
- Gemini OCR
- evaluation
