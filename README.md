# Expense Reports Automation

This repo builds the data spine for a robust expense-report automation system:

- OCR / transcription into markdown
- typed document-fact extraction
- bundle synthesis into a canonical report
- schema projection and validation
- FA review packet and workbench generation
- feedback capture and versioned review/submission ledger

The current validated live OCR path is `Gemini 3` on Vertex AI through the official Google Gen AI SDK, exposed via the `vertex-gemini-sdk` ingestion engine.

That OCR path now applies a small deterministic markdown normalization pass after model output so downstream extraction does not churn on heading spacing or wrapped pipe-row continuations.

## Repo Map

- [system-architecture.md](/Users/adityasriram/Labs/stanford/research/expense-reports/system-architecture.md:1): full system design
- [real-document-ingestion.md](/Users/adityasriram/Labs/stanford/research/expense-reports/real-document-ingestion.md:1): OCR and ingestion CLI details
- [ingestion-workspace.md](/Users/adityasriram/Labs/stanford/research/expense-reports/ingestion-workspace.md:1): managed bundle workspace and rerunnable upload flow
- [document-facts.md](/Users/adityasriram/Labs/stanford/research/expense-reports/document-facts.md:1): typed per-document extraction contract
- [bundle-synthesis.md](/Users/adityasriram/Labs/stanford/research/expense-reports/bundle-synthesis.md:1): cross-document synthesis and draft projection
- [review-workbench.md](/Users/adityasriram/Labs/stanford/research/expense-reports/review-workbench.md:1): FA-facing output surface
- [review-submission-ledger.md](/Users/adityasriram/Labs/stanford/research/expense-reports/review-submission-ledger.md:1): versioned review and submission tracking

## Prerequisites

- Rust with `cargo`
- Python 3
- `pdftotext` for builtin PDF transcription
- A Google Cloud service-account JSON key with Vertex AI access for live Gemini OCR

For live Gemini OCR, the helper script uses the Google Gen AI SDK. A local venv like this is the simplest setup:

```bash
python3 -m venv /tmp/expense_report_genai_venv
/tmp/expense_report_genai_venv/bin/pip install google-genai google-auth pillow
```

The code automatically uses `/tmp/expense_report_genai_venv/bin/python` if it exists. You can override that with `--sdk-python` or `VERTEX_GEMINI_SDK_PYTHON`.

## Quick Start

Run the Rust test suite:

```bash
cargo test
```

Run the Python evaluator tests:

```bash
python3 -m unittest discover -s tests
```

Generate a synthetic corpus for inspection:

```bash
cargo run --bin generate_synthetic_corpus -- --output-dir /tmp/expense_corpus --packets 8
```

Validate a minimal schema-shaped report:

```bash
cargo run --bin validate_report -- \
  examples/minimal_report.yaml
```

For realistic end-to-end document ingestion, use synthetic documents or OCR-rendered files as shown below.

## OCR and Ingestion

Transcribe a single PNG or PDF through Gemini 3:

```bash
cargo run --bin transcribe_document -- \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --model gemini-3-flash-preview \
  receipt.png
```

Run end-to-end ingestion on a packet of rendered source documents:

```bash
cargo run --bin ingest_expense_documents -- \
  --output-dir /tmp/expense_ingest_live \
  --fx demo \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --model gemini-3-flash-preview \
  itinerary.png hotel_folio.pdf receipt.png
```

This writes:

- `transcriptions/*.transcribed.json`
- `facts/*.facts.json`
- `bundle.json`
- `draft.yaml`
- `validation.json`
- `readiness.json`
- `review_packet.json`
- `review_workbench.html`
- `ledger.json`
- `manifest.json`

Run the managed bundle-workspace flow so raw uploads, normalized artifacts, and processing runs are stored together:

```bash
cargo run --bin ingest_bundle_workspace -- \
  stage-and-run \
  --workspace-root /tmp/expense_workspace \
  --bundle-id live_demo \
  --user-id aditya \
  --run-id gemini_flash \
  --fx demo \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  itinerary.png hotel_folio.pdf receipt.png
```

That creates a stable bundle directory with:

- `uploads/`
- `normalized/`
- `runs/<run_id>/artifacts/`
- `bundle_manifest.json`

## Synthetic OCR Evaluation

The main regression harness for live OCR is [scripts/evaluate_synthetic_ocr_corpus.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/evaluate_synthetic_ocr_corpus.py:1). It:

- generates a synthetic packet corpus
- renders markdown into OCR-style PNG/PDF documents
- runs end-to-end ingestion with Gemini 3
- compares OCR markdown against the source markdown
- reports filing/readiness outcomes per model

By default it compares:

- `gemini-3-flash-preview`
- `gemini-3-pro-preview`

If your project lacks access to one of the requested models, the evaluator does not abort. It records that model as unavailable in the comparison output and still keeps the successful model results.

Example:

```bash
python3 scripts/evaluate_synthetic_ocr_corpus.py \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --output-dir /tmp/expense_ocr_eval \
  --packets 4
```

Outputs include:

- `ocr_comparison.json`
- `ocr_comparison.md`
- `ocr_evaluation_gemini_3_flash_preview.json`
- `ocr_evaluation_gemini_3_flash_preview.md`
- `ocr_evaluation_gemini_3_pro_preview.json`
- `ocr_evaluation_gemini_3_pro_preview.md`

If you only pass one `--model`, the script also writes:

- `ocr_evaluation.json`
- `ocr_evaluation.md`

For managed-workspace stress testing, use [scripts/evaluate_workspace_pipeline.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/evaluate_workspace_pipeline.py:1). It exercises the persistent `bundle_id` workflow instead of the one-shot ingestion CLI.

Large builtin workspace stress run:

```bash
python3 scripts/evaluate_workspace_pipeline.py \
  --output-dir /tmp/workspace_builtin_stress \
  --engine builtin \
  --packets 128
```

Live Gemini workspace stress run:

```bash
python3 scripts/evaluate_workspace_pipeline.py \
  --output-dir /tmp/workspace_gemini_stress \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --model gemini-3-flash-preview \
  --packets 12
```

## Testing Surface

Current automated coverage includes:

- Rust unit and regression tests for schema generation, validation, extraction, bundle synthesis, review packet/workbench, feedback, ledger, and ingestion
- mocked Vertex REST tests
- mocked SDK-backed OCR ingestion tests
- live synthetic OCR evaluation against Gemini 3
- Python unit tests for the OCR comparison harness

## Current Limits

- The current synthetic live OCR evaluation is clean at larger scale: Gemini 3 Flash matched `96/96` documents exactly on a 32-packet synthetic stress test.
- Document extraction coverage is strongest for:
  - flight itineraries
  - hotel folios
  - restaurant-style receipts
- Stanford accepted expense report PDFs in this repo are reference material, not the intended OCR input set for extractor evaluation.
