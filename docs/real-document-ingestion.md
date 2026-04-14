# Real-Document Ingestion

This repo now has a real upstream ingestion path that can transcribe PDF and image documents, extract document facts, run bundle synthesis and validation, build the FA review surface, and initialize the versioned review/submission ledger.

The relevant code lives in:

- [src/vertex_gemini.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/vertex_gemini.rs:1)
- [src/ingest.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/ingest.rs:1)
- [src/bin/ingest_expense_documents.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/ingest_expense_documents.rs:1)
- [src/workspace.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/workspace.rs:1)
- [src/bin/ingest_bundle_workspace.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/ingest_bundle_workspace.rs:1)
- [src/bin/transcribe_document.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/transcribe_document.rs:1)

It supports three transcription modes:

- `builtin`: existing `pdftotext` or plain-text markdown path
- `vertex-gemini`: Gemini on Vertex AI for `.pdf`, `.png`, `.jpg`, and `.jpeg`
- `vertex-gemini-sdk`: Gemini 3 through the official Google Gen AI SDK helper path

## What the ingestion pipeline does

For each input document, the pipeline now:

1. transcribes the document
2. extracts typed document facts
3. synthesizes the canonical expense bundle
4. validates the projected draft
5. computes readiness
6. builds the review packet
7. renders the FA workbench HTML
8. initializes the review/submission ledger

The output artifact set is:

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

For a persistent upload-oriented flow, the managed workspace adds:

- `bundle_manifest.json`
- `uploads/...`
- `normalized/...`
- `runs/<run_id>/artifacts/...`

## Vertex Gemini configuration

The Vertex path uses the Vertex `generateContent` endpoint and sends the source file inline with the correct MIME type.

Environment variables:

- `VERTEX_PROJECT_ID`
- `VERTEX_LOCATION`
- `VERTEX_GEMINI_MODEL` optional, defaults to `gemini-3-flash-preview`
- `VERTEX_ACCESS_TOKEN` optional if you want to pass a short-lived bearer token directly
- `VERTEX_SERVICE_ACCOUNT_KEY` optional path to a Google Cloud service-account JSON key
- `VERTEX_ENDPOINT_OVERRIDE` optional for tests
- `VERTEX_TOKEN_ENDPOINT_OVERRIDE` optional for tests
- `VERTEX_GEMINI_SDK_PYTHON` optional Python interpreter for the SDK helper
- `VERTEX_GEMINI_SDK_SCRIPT` optional path override for the SDK helper script

For live OCR work, prefer:

- `gemini-3-flash-preview` for the default OCR path
- `gemini-3-pro-preview` when you want a slower, higher-fidelity comparison run

These Gemini 3 preview examples are intended to run in `global`.

Auth resolution order:

1. explicit `--access-token` or `VERTEX_ACCESS_TOKEN`
2. explicit `--service-account-key` or `VERTEX_SERVICE_ACCOUNT_KEY`
3. `gcloud auth print-access-token`

When a service-account key is supplied, the repo now signs a JWT locally, exchanges it for a short-lived OAuth access token, and then uses that bearer token for Vertex requests. During bundle ingestion, that token exchange is done once per bundle, not once per document.

CLI flags can override the same values:

- `--project`
- `--location`
- `--model`
- `--access-token`
- `--service-account-key`
- `--endpoint`
- `--token-endpoint`
- `--sdk-python`
- `--sdk-script`

## Commands

Run the full ingestion pipeline on a document set with the builtin transcriber:

```bash
cargo run --bin ingest_expense_documents -- \
  --output-dir /tmp/ingest_cli_output \
  --fx demo \
  /tmp/ingest_cli_input/synthetic_flight_itinerary_baseline.md \
  /tmp/ingest_cli_input/synthetic_hotel_folio_baseline.md \
  /tmp/ingest_cli_input/synthetic_receipt_baseline.md
```

Run the same pipeline with Vertex Gemini:

```bash
cargo run --bin ingest_expense_documents -- \
  --output-dir /tmp/ingest_vertex_output \
  --engine vertex-gemini \
  --project "$VERTEX_PROJECT_ID" \
  --location global \
  --model gemini-3-flash-preview \
  receipt.png hotel_folio.png itinerary.pdf
```

Run the same pipeline with the SDK-backed Gemini 3 path and a raw service-account JSON key:

```bash
cargo run --bin ingest_expense_documents -- \
  --output-dir /tmp/ingest_vertex_output \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  receipt.png hotel_folio.png itinerary.pdf
```

If the JSON key contains `project_id`, `--project` is optional.

Run the managed bundle-workspace flow so uploads and processing runs stay grouped under one `bundle_id`:

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

Inspect the persisted bundle manifest:

```bash
cargo run --bin ingest_bundle_workspace -- \
  status \
  --workspace-root /tmp/expense_workspace \
  --bundle-id live_demo \
  --format json
```

Transcribe a single document directly through Vertex Gemini:

```bash
cargo run --bin transcribe_document -- \
  --engine vertex-gemini-sdk \
  --project "$VERTEX_PROJECT_ID" \
  --location global \
  receipt.png
```

Or with a service-account key:

```bash
cargo run --bin transcribe_document -- \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  receipt.png
```

Run the synthetic OCR corpus evaluator over a small rendered packet set:

```bash
python3 scripts/evaluate_synthetic_ocr_corpus.py \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --output-dir /tmp/expense_ocr_eval \
  --packets 2
```

If you want one checked-in command that exercises isolated OCR, full ingestion, and the synthetic OCR evaluator in sequence, use:

```bash
bash scripts/run_gemini_smoke_test.sh --packets 2
```

By default that compares:

- `gemini-3-flash-preview`
- `gemini-3-pro-preview`

You can override the model set by repeating `--model`:

```bash
python3 scripts/evaluate_synthetic_ocr_corpus.py \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --output-dir /tmp/expense_ocr_eval_flash_only \
  --packets 4 \
  --model gemini-3-flash-preview
```

The evaluator writes per-model OCR reports plus a comparison summary:

- `ocr_comparison.json`
- `ocr_comparison.md`
- `ocr_evaluation_<model>.json`
- `ocr_evaluation_<model>.md`

If a requested model is not available to the current Vertex project, the evaluator records that model as unavailable instead of aborting the whole comparison run.

## Current test coverage

The new ingestion coverage includes:

- mocked Vertex HTTP transcription requests
- mocked service-account JWT exchange requests
- mocked SDK-backed OCR subprocess transcription
- mocked SDK-backed end-to-end ingestion through the review/ledger path
- service-account project inference from JSON keys
- fenced and unfenced JSON response parsing
- MIME detection for supported upload types
- builtin end-to-end ingestion on synthetic packets
- mocked Vertex end-to-end ingestion into document facts, review packet, workbench, and ledger
- mocked bundle ingestion with service-account auth and single token exchange per bundle
- artifact writing checks for the ingestion output bundle
- workspace staging of markdown/text uploads into persisted bundle directories
- bundle-local deduplication by SHA-256 content hash
- staged bundle reruns that accumulate run history without restaging documents

## Current limitation

The real ingestion stack is now complete up to the current document-fact extractors. That means the OCR/transcription layer is real, but extraction coverage is still strongest for:

- flight itineraries
- hotel folios
- restaurant-style receipts

The managed workspace is filesystem-backed, which is enough for local end-to-end development and evaluation. It is the local stand-in for the object-store + metadata-store layer described in the architecture, not a distributed service deployment.

Real Stanford summary PDFs can now flow through the ingestion CLI and produce artifacts, but they still end up `automation_blocked` unless they match one of the currently implemented extractor families.

## Gemini 3 Note

The validated Gemini 3 live OCR path in this repo is the official Google Gen AI SDK helper in [scripts/transcribe_with_google_genai.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/transcribe_with_google_genai.py:1), now exposed directly through `vertex-gemini-sdk`. In live testing, that SDK path succeeded with `gemini-3-flash-preview`, `gemini-3.1-flash-lite-preview`, and `gemini-3-pro-preview` against synthetic PNG/PDF OCR fixtures.

That helper now also applies a deterministic markdown cleanup pass after OCR generation to normalize section-heading spacing and merge wrapped pipe-delimited rows such as hotel nightly charge lines.

The older Rust `vertex-gemini` REST path is still useful for mocked tests and lower-level contract work, but it returned `404` for the tested Gemini 3 preview model ids in this project, so `vertex-gemini-sdk` is the validated path for current Gemini 3 live OCR work.

The managed workspace flow has also now been exercised live end to end with `vertex-gemini-sdk` on a rendered synthetic packet, producing persisted raw uploads, normalized page artifacts, and a stored processing run under a single `bundle_id`.
