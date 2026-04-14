# Ingestion Workspace

This repo now has a managed filesystem-backed ingestion workspace for real document bundles.

The core code lives in:

- [src/workspace.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/workspace.rs:1)
- [src/bin/ingest_bundle_workspace.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/ingest_bundle_workspace.rs:1)

The goal is to make uploads durable and rerunnable instead of treating ingestion as a one-shot CLI that only writes a single output directory.

## What the workspace stores

Each `bundle_id` gets a stable directory under:

```text
<workspace_root>/bundles/<bundle_id>/
```

Inside that bundle directory, the workspace persists:

- `uploads/...`: immutable raw uploaded files
- `normalized/...`: preprocessing artifacts per document
- `runs/<run_id>/artifacts/...`: OCR, extraction, review, and ledger outputs for a processing run
- `bundle_manifest.json`: bundle metadata, deduped document records, and run history

The document-level normalization artifacts currently include:

- `document_manifest.json`
- `pages/page_0001.png` and later pages when applicable
- `native_text.json` for formats with deterministic text extraction, such as PDFs and markdown/text fixtures

## Supported workflow

Stage uploads into the workspace without running OCR:

```bash
cargo run --bin ingest_bundle_workspace -- \
  stage \
  --workspace-root /tmp/expense_workspace \
  --bundle-id demo_bundle \
  --user-id aditya \
  itinerary.pdf hotel_folio.pdf receipt.png
```

Run OCR/extraction/review initialization on an already staged bundle:

```bash
cargo run --bin ingest_bundle_workspace -- \
  run \
  --workspace-root /tmp/expense_workspace \
  --bundle-id demo_bundle \
  --run-id gemini_flash \
  --fx demo \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global
```

Do both in one command:

```bash
cargo run --bin ingest_bundle_workspace -- \
  stage-and-run \
  --workspace-root /tmp/expense_workspace \
  --bundle-id demo_bundle \
  --user-id aditya \
  --run-id gemini_flash \
  --fx demo \
  --engine vertex-gemini-sdk \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  itinerary.png hotel_folio.pdf receipt.png
```

Inspect bundle status:

```bash
cargo run --bin ingest_bundle_workspace -- \
  status \
  --workspace-root /tmp/expense_workspace \
  --bundle-id demo_bundle \
  --format json
```

## Current behavior

- identical files inside the same bundle are deduplicated by SHA-256
- original filenames are preserved in the manifest
- each run gets its own `run_id` and artifact directory
- the bundle manifest tracks the current stage using the downstream filing/ledger state
- the current validated live OCR path is `vertex-gemini-sdk`

## Validation status

The workspace layer is covered by Rust tests for:

- markdown/text bundle staging
- within-bundle file deduplication by content hash
- staged-bundle execution into persisted artifacts
- repeated reruns that reuse staged documents while accumulating run history

It has also been exercised live with Gemini OCR on a rendered synthetic packet, producing:

- raw uploads under `uploads/`
- normalized page-level artifacts under `normalized/`
- a persisted OCR/extraction/review run under `runs/<run_id>/artifacts/`

## Corpus stress harness

The repo now also has a managed-workspace corpus evaluator in [scripts/evaluate_workspace_pipeline.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/evaluate_workspace_pipeline.py:1).

It can stress test:

- the persistent workspace bundle layout
- repeated `stage-and-run` execution over many synthetic packets
- OCR markdown fidelity for live Gemini runs
- final filing and ledger state consistency

Large local builtin stress run:

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

The evaluator writes:

- `workspace_evaluation.json`
- `workspace_evaluation.md`
- `source_corpus/`
- `rendered/` when OCR-style inputs are used
- `workspace/` with the full persisted bundle tree
