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
- `VERTEX_GEMINI_MODEL` optional, defaults to `gemini-2.5-flash`
- `VERTEX_ACCESS_TOKEN` optional if `gcloud auth print-access-token` is available
- `VERTEX_ENDPOINT_OVERRIDE` optional for tests

CLI flags can override the same values:

- `--project`
- `--location`
- `--model`
- `--access-token`
- `--endpoint`

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
  --location "$VERTEX_LOCATION" \
  --model gemini-2.5-flash \
  receipt.png hotel_folio.png itinerary.pdf
```

Transcribe a single document directly through Vertex Gemini:

```bash
cargo run --bin transcribe_document -- \
  --engine vertex-gemini \
  --project "$VERTEX_PROJECT_ID" \
  --location "$VERTEX_LOCATION" \
  receipt.png
```

## Current test coverage

The new ingestion coverage includes:

- mocked Vertex HTTP transcription requests
- fenced and unfenced JSON response parsing
- MIME detection for supported upload types
- builtin end-to-end ingestion on synthetic packets
- mocked Vertex end-to-end ingestion into document facts, review packet, workbench, and ledger
- artifact writing checks for the ingestion output bundle

## Current limitation

The real ingestion stack is now complete up to the current document-fact extractors. That means the OCR/transcription layer is real, but extraction coverage is still strongest for:

- flight itineraries
- hotel folios
- restaurant-style receipts

Real Stanford summary PDFs can now flow through the ingestion CLI and produce artifacts, but they still end up `automation_blocked` unless they match one of the currently implemented extractor families.
