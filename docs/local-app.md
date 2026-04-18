# Local App

This repo now includes a tiny browser-based local app for FA-facing intake and review:

- [scripts/local_app.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/local_app.py:1)

It sits on top of the managed workspace flow rather than replacing it. The app is intentionally small: it stages uploaded documents into a `bundle_id`, runs the existing ingestion pipeline, and redirects to the generated review workbench.

## What it does

- renders an upload form for PDF, PNG, JPG, markdown, and text inputs
- accumulates file selections across repeated file-picker opens before submit
- stores documents in the managed bundle workspace
- runs `ingest_bundle_workspace stage-and-run`
- shows recent bundles and their current stages
- opens bundles directly into the editable review workbench by default
- keeps a separate bundle overview page for exports, metadata, and uploaded document inventory
- lets the FA save edits back into a typed reviewed draft version
- persists latest reviewed artifacts and versioned review snapshots inside the bundle run
- exposes the bundle manifest JSON for inspection
- exposes current draft, packet, ledger, and review-session exports
- serves raw uploaded bundle documents so workbench evidence links can open them

## Run it

```bash
python3 scripts/local_app.py \
  --workspace-root /tmp/expense_local_app_workspace \
  --default-engine vertex-gemini-sdk \
  --default-service-account-key /abs/path/to/service-account.json \
  --default-sdk-python ./.venv/bin/python \
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

Notes:

- You can also reopen the file picker multiple times before submit; the pending upload list will accumulate those selections.
- By default, technical ingestion settings are hidden from the upload form and supplied server-side through the app configuration.
- If you want those overrides visible for engineering/debugging, start the app with `--show-advanced-config`.
- If you leave `Run Label` blank in the advanced section, the workspace generates a unique run id automatically.

## Vertex-backed usage

For an FA-facing deployment, the recommended setup is to configure Gemini server-side when you start the app:

```bash
python3 scripts/local_app.py \
  --workspace-root /tmp/expense_local_app_workspace \
  --default-engine vertex-gemini-sdk \
  --default-service-account-key /abs/path/to/service-account.json \
  --default-sdk-python ./.venv/bin/python \
  --default-location global \
  --default-model gemini-3-flash-preview
```

That gives the FA a simpler upload form with no engine/model/credential fields.

If you explicitly enable `--show-advanced-config`, the form can still override:

- `Engine`
- `Project`
- `Location`
- `Model`
- `Service Account Key`
- `SDK Python`
- `FX Mode`
- `Run Label`

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

The current tests also cover:

- review-save command construction
- review-session JSON export
- reviewed-artifact export routes
- JSON error responses for failed review saves
