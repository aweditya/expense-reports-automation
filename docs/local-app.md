# Local App

This repo now includes a tiny browser-based local app for FA-facing intake and review:

- [scripts/local_app.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/local_app.py:1)

It sits on top of the managed workspace flow rather than replacing it. The app is intentionally small: it stages uploaded documents into a `bundle_id`, runs the existing ingestion pipeline, and redirects to the generated review workbench.

## What it does

- renders an upload form for PDF, PNG, JPG, markdown, and text inputs
- stores documents in the managed bundle workspace
- runs `ingest_bundle_workspace stage-and-run`
- shows recent bundles and their current stages
- opens the latest generated `review_workbench.html` inside a bundle page
- exposes the bundle manifest JSON for inspection

## Run it

```bash
python3 scripts/local_app.py \
  --workspace-root /tmp/expense_local_app_workspace \
  --host 127.0.0.1 \
  --port 8765
```

Then open:

```text
http://127.0.0.1:8765
```

The app supports:

- `builtin` transcription for markdown/text and native PDF text extraction
- `vertex-gemini-sdk` for live Gemini 3 OCR

## Vertex-backed usage

To use live Gemini OCR from the app, fill in:

- `Engine`: `vertex-gemini-sdk`
- `Project`: optional if the service-account JSON already contains `project_id`
- `Location`: usually `global`
- `Model`: for example `gemini-3-flash-preview`
- `Service Account Key`: absolute path to the Vertex service-account JSON key
- `SDK Python`: optional path to the Python interpreter that has `google-genai`

The app passes those values through to:

```bash
cargo run --bin ingest_bundle_workspace -- stage-and-run ...
```

## Bundle layout

The app writes into the same persisted bundle structure described in [ingestion-workspace.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/ingestion-workspace.md:1):

- `bundles/<bundle_id>/uploads/`
- `bundles/<bundle_id>/normalized/`
- `bundles/<bundle_id>/runs/<run_id>/artifacts/`
- `bundles/<bundle_id>/bundle_manifest.json`

## Test coverage

The local app is covered by Python tests in [tests/test_local_app.py](/Users/adityasriram/Labs/stanford/research/expense-reports/tests/test_local_app.py:1), including:

- multipart form parsing
- duplicate filename handling
- ingestion command construction for `builtin` and `vertex-gemini-sdk`
- bundle listing and workbench lookup
- upload submission handling
- handler-level GET/POST routing via socket-pair HTTP simulation

The handler tests avoid external ports so they stay stable in sandboxed environments while still exercising the real request code.
