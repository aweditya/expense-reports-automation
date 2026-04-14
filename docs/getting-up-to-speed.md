# Getting Up To Speed

This guide is for someone who has not worked on this repo before and wants to get oriented quickly without reading everything in random order.

The right mental model is:

`documents -> OCR/transcription -> per-document facts -> canonical bundle -> schema draft -> validation/readiness -> FA workbench -> feedback/ledger`

If you follow the steps below in order, you will understand both the architecture and the code paths that implement it.

## 1. Set Up The Environment

Install or confirm the following:

- Rust and `cargo`
- Python 3
- `pdftotext` for builtin PDF text extraction
- for live Gemini OCR: a Google Cloud service-account JSON key and a Python env with `google-genai`, `google-auth`, and `pillow`

Recommended Gemini SDK env:

```bash
python3 -m venv /tmp/expense_report_genai_venv
/tmp/expense_report_genai_venv/bin/pip install google-genai google-auth pillow
```

## 2. Read The Repo Top Down

Read these files in order.

1. [README.md](/Users/adityasriram/Labs/stanford/research/expense-reports/README.md:1)
   This tells you what the repo does and which commands are considered primary.

2. [docs/system-architecture.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/system-architecture.md:1)
   This is the system-level blueprint. Read it before reading implementation files.

3. [schema.yaml](/Users/adityasriram/Labs/stanford/research/expense-reports/schema.yaml:1)
   This is the single source of truth for the final expense-report shape.

4. [docs/schema-artifacts.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/schema-artifacts.md:1)
   This explains how `schema.yaml` becomes typed Rust artifacts and validation metadata.

5. [docs/document-facts.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/document-facts.md:1)
   This introduces the per-document extraction boundary.

6. [docs/bundle-synthesis.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/bundle-synthesis.md:1)
   This explains how per-document facts become a single expense-report bundle and draft.

7. [docs/review-workbench.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/review-workbench.md:1)
   This explains the FA-facing surface.

8. [docs/review-submission-ledger.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/review-submission-ledger.md:1)
   This explains how the pipeline continues after human review.

After those eight, the rest of the docs are workflow-specific rather than foundational.

## 3. Read The Core Code In Pipeline Order

Once the docs above make sense, read the code in the order data flows through it.

1. [src/draft.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/draft.rs:1)
   This defines `DraftReport`, field metadata, and evidence references.

2. [src/document_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/document_facts.rs:1)
   This defines the typed per-document fact contract.

3. [src/document_extract.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/document_extract.rs:1)
   This is where markdown/text gets parsed into typed document facts.

4. [src/bundle_synthesis.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bundle_synthesis.rs:1)
   This synthesizes a `CanonicalExpenseBundle` and projects it into a schema-shaped draft.

5. [generated/validation_rules.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/generated/validation_rules.rs:1)
   This is the generated runtime schema contract.

6. [src/validator.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/validator.rs:1)
   This turns the validation metadata into concrete validation issues.

7. [src/readiness.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/readiness.rs:1)
   This classifies validation issues into automation gaps, user input gaps, and manual review items.

8. [src/review_packet.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_packet.rs:1)
   This produces the typed FA handoff artifact.

9. [src/review_workbench.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_workbench.rs:1)
   This renders the static HTML workbench.

10. [src/feedback.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/feedback.rs:1)
    This captures FA corrections and site outcomes.

11. [src/ledger.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/ledger.rs:1)
    This stores versioned review/submission state.

12. [src/workspace.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/workspace.rs:1)
    This is the local persistent storage/orchestration layer for bundles and runs.

13. [src/ingest.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/ingest.rs:1)
    This is the end-to-end orchestration path for one-shot ingestion.

14. [src/transcribe.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/transcribe.rs:1) and [src/vertex_gemini_sdk.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/vertex_gemini_sdk.rs:1)
    These are the OCR/transcription entry points.

15. [src/lib.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/lib.rs:1)
    Read this last as the crate-level export map.

## 4. Learn The Main Terms

These terms recur everywhere in the repo.

- `Document`: one uploaded file, such as a receipt, hotel folio, or itinerary
- `Transcription`: OCR/native-text output, usually represented as markdown-like page text
- `ExtractedDocumentFacts`: typed facts for one document
- `Bundle`: the set of documents for one payee’s expense-report packet
- `CanonicalExpenseBundle`: the merged cross-document state before schema projection
- `DraftReport`: a schema-shaped report plus per-field metadata/evidence
- `ValidationReport`: raw schema/rule violations
- `ReadinessReport`: interpretation of validation issues into workflow classes
- `ReviewPacket`: typed FA-facing handoff artifact
- `Review Workbench`: deterministic HTML surface for copy/paste filing
- `Ledger`: versioned history of review, submission, and returns

## 5. Run The Fastest Confidence Checks

Run these commands in order.

1. Full Rust test suite:

```bash
cargo test
```

2. Python test suite:

```bash
python3 -m unittest discover -s tests
```

3. Builtin local app flow:

```bash
python3 scripts/local_app.py \
  --workspace-root /tmp/expense_local_app_workspace \
  --port 8765
```

Then open `http://127.0.0.1:8765` and upload:

- `fixtures/curated/flight_itinerary/airline_itinerary_classic.md`
- `fixtures/curated/hotel_folio/hotel_folio_guest_bill.md`
- `fixtures/curated/receipt/receipt_card_dotted.md`

Use:

- `Engine`: `builtin`
- `FX Mode`: `demo`
- leave `Run ID` blank

4. Gemini smoke test:

```bash
bash scripts/run_gemini_smoke_test.sh --packets 2
```

This is the quickest way to verify the live Gemini path without manually stitching commands together.

## 6. Know Which Entry Points Matter

These are the main executable entry points.

- [scripts/local_app.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/local_app.py:1)
  Browser-based local intake and review flow

- [src/bin/transcribe_document.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/transcribe_document.rs:1)
  Single-document OCR/transcription debugging

- [src/bin/ingest_expense_documents.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/ingest_expense_documents.rs:1)
  One-shot end-to-end ingestion

- [src/bin/ingest_bundle_workspace.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/ingest_bundle_workspace.rs:1)
  Persistent bundle-oriented ingestion

- [scripts/evaluate_synthetic_ocr_corpus.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/evaluate_synthetic_ocr_corpus.py:1)
  Live OCR regression and model comparison

- [scripts/evaluate_workspace_pipeline.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/evaluate_workspace_pipeline.py:1)
  Workspace-oriented stress testing

## 7. Know Where Outputs Go

For one-shot ingestion, outputs are written into one directory:

- `transcriptions/`
- `facts/`
- `bundle.json`
- `draft.yaml`
- `validation.json`
- `readiness.json`
- `review_packet.json`
- `review_workbench.html`
- `ledger.json`
- `manifest.json`

For persistent bundle ingestion, look under:

```text
<workspace_root>/bundles/<bundle_id>/
```

Important paths:

- `uploads/`
- `normalized/`
- `runs/<run_id>/artifacts/`
- `bundle_manifest.json`

## 8. If You Need To Change A Specific Layer

Use this routing table.

- Changing the final schema:
  Start with [schema.yaml](/Users/adityasriram/Labs/stanford/research/expense-reports/schema.yaml:1), then [docs/schema-artifacts.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/schema-artifacts.md:1)

- Adding support for a new document type:
  Start with [src/document_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/document_facts.rs:1), [src/document_extract.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/document_extract.rs:1), and synthetic fixtures under [fixtures](/Users/adityasriram/Labs/stanford/research/expense-reports/fixtures:1)

- Changing cross-document projection:
  Start with [src/bundle_synthesis.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bundle_synthesis.rs:1)

- Changing validation or readiness behavior:
  Start with [generated/validation_rules.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/generated/validation_rules.rs:1), [src/validator.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/validator.rs:1), and [src/readiness.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/readiness.rs:1)

- Changing the FA experience:
  Start with [src/review_packet.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_packet.rs:1), [src/review_workbench.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_workbench.rs:1), [src/review_workbench.css](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_workbench.css:1), and [scripts/local_app.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/local_app.py:1)

- Changing OCR behavior:
  Start with [src/vertex_gemini_sdk.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/vertex_gemini_sdk.rs:1), [scripts/transcribe_with_google_genai.py](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/transcribe_with_google_genai.py:1), and [scripts/run_gemini_smoke_test.sh](/Users/adityasriram/Labs/stanford/research/expense-reports/scripts/run_gemini_smoke_test.sh:1)

## 9. Good First Checks Before You Touch Code

Before making changes, answer these questions:

1. Which layer am I changing: OCR, extraction, synthesis, validation, review, or ledger?
2. Is this change local to one document type or does it affect all bundles?
3. Do I need a new synthetic fixture or regression case?
4. What artifact should look different if my change is correct?
5. Which test or evaluator should fail before the change and pass after it?

If you can answer those five clearly, you are usually starting in the right place.
