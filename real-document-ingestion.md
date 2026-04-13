# Real-Document Ingestion

This repo now has a real upstream ingestion path that can transcribe PDF and image documents, extract document facts, run bundle synthesis and validation, build the FA review surface, and initialize the versioned review/submission ledger.

The relevant code lives in:

- [src/vertex_gemini.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/vertex_gemini.rs:1)
- [src/ingest.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/ingest.rs:1)
- [src/bin/ingest_expense_documents.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/ingest_expense_documents.rs:1)
- [src/bin/transcribe_document.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/transcribe_document.rs:1)

It supports two transcription modes:

- `builtin`: existing `pdftotext` or plain-text markdown path
- `vertex-gemini`: Gemini on Vertex AI for `.pdf`, `.png`, `.jpg`, and `.jpeg`

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

## Vertex Gemini configuration

The Vertex path uses the Vertex `generateContent` endpoint and sends the source file inline with the correct MIME type.

Environment variables:

- `VERTEX_PROJECT_ID`
- `VERTEX_LOCATION`
- `VERTEX_GEMINI_MODEL` optional, defaults to `gemini-3.1-flash-lite-preview`
- `VERTEX_ACCESS_TOKEN` optional if you want to pass a short-lived bearer token directly
- `VERTEX_SERVICE_ACCOUNT_KEY` optional path to a Google Cloud service-account JSON key
- `VERTEX_ENDPOINT_OVERRIDE` optional for tests
- `VERTEX_TOKEN_ENDPOINT_OVERRIDE` optional for tests

For live OCR work, prefer:

- `gemini-3.1-flash-lite-preview` for the default OCR path
- `gemini-3.1-pro-preview` when you want a slower, higher-fidelity comparison run

Live OCR validation in this repo was also successfully exercised with `gemini-3-flash-preview`.

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
  --model gemini-3.1-flash-lite-preview \
  receipt.png hotel_folio.png itinerary.pdf
```

Run the same pipeline with a raw service-account JSON key instead of a pre-minted access token:

```bash
cargo run --bin ingest_expense_documents -- \
  --output-dir /tmp/ingest_vertex_output \
  --engine vertex-gemini \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  receipt.png hotel_folio.png itinerary.pdf
```

If the JSON key contains `project_id`, `--project` is optional.

Transcribe a single document directly through Vertex Gemini:

```bash
cargo run --bin transcribe_document -- \
  --engine vertex-gemini \
  --project "$VERTEX_PROJECT_ID" \
  --location global \
  receipt.png
```

Or with a service-account key:

```bash
cargo run --bin transcribe_document -- \
  --engine vertex-gemini \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  receipt.png
```

## Current test coverage

The new ingestion coverage includes:

- mocked Vertex HTTP transcription requests
- mocked service-account JWT exchange requests
- service-account project inference from JSON keys
- fenced and unfenced JSON response parsing
- MIME detection for supported upload types
- builtin end-to-end ingestion on synthetic packets
- mocked Vertex end-to-end ingestion into document facts, review packet, workbench, and ledger
- mocked bundle ingestion with service-account auth and single token exchange per bundle
- artifact writing checks for the ingestion output bundle

## Current limitation

The real ingestion stack is now complete up to the current document-fact extractors. That means the OCR/transcription layer is real, but extraction coverage is still strongest for:

- flight itineraries
- hotel folios
- restaurant-style receipts

Real Stanford summary PDFs can now flow through the ingestion CLI and produce artifacts, but they still end up `automation_blocked` unless they match one of the currently implemented extractor families.

## Gemini 3 Note

The live Gemini 3 OCR probes in this repo currently use the official Google Gen AI SDK helper in [scripts/transcribe_with_google_genai.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/transcribe_with_google_genai.py:1). In live testing, that SDK path succeeded with `gemini-3-flash-preview`, `gemini-3.1-flash-lite-preview`, and `gemini-3.1-pro-preview` against synthetic PNG/PDF OCR fixtures.

The older Rust `vertex-gemini` REST path is still useful for mocked tests and lower-level contract work, but it returned `404` for the tested Gemini 3 preview model ids in this project, so the SDK helper is the validated path for current Gemini 3 live OCR work.
