# Project Instructions

## Scope boundaries

- **DO NOT** modify schema validation logic (`validator.rs`, `readiness.rs`, `schema.yaml`).
- **DO NOT** modify ingestion or OCR pipeline logic (`ingest.rs`, `vertex_gemini.rs`, `vertex_gemini_sdk.rs`, `transcribe.rs`, `document_extract.rs`, `document_facts.rs`).
- **DO NOT** modify bundle synthesis or projection logic (`bundle_synthesis.rs`, `bundle_regression.rs`).
- **DO NOT** modify draft generation or ledger logic (`draft.rs`, `ledger.rs`, `ledger_regression.rs`).
- **DO NOT** modify the data model in `review_packet.rs` (struct definitions, serialization) unless required for a UI change.
- Focus is solely on the **user-facing UI**: `review_workbench.rs`, `review_workbench.css`, the Python local app (`scripts/local_app.py`), and any new frontend assets.

## Working style

- Before making a change: explain **why** the change is needed.
- After making a change: summarize **what** changed and why.
- Maintain a running plan of action and update it as work progresses.
- Write unit tests for every change. The project uses `#[cfg(test)]` modules in each Rust source file; follow that pattern. For Python, add tests alongside the existing structure.

## Build and test

- `cargo test` runs the full Rust test suite.
- `cargo test --lib review_workbench` runs workbench-specific tests.
- The workbench regression fixtures live in `fixtures/workbench_regressions/`.

## Deployment

- Target: Google Cloud Run in the `soe-agile-agents` GCP project.
- CI/CD: Cloud Build trigger on push to `main` → run tests → build Docker image → deploy. If tests fail, deploy is blocked.
- Deployment-related changes are limited to: `scripts/local_app.py` (host/port/binary resolution), `Dockerfile`, `cloudbuild.yaml`. Do not modify Rust pipeline code for deployment purposes.
- The Dockerfile uses a two-stage build: Rust compilation in builder stage, pre-compiled binaries copied to Python slim runtime.
- Cloud Run config: `--max-instances=1`, `--timeout=600`, GCS FUSE for workspace persistence.
