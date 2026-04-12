# Synthetic Document Pipeline

This repo now supports a full synthetic document-fact extraction loop for three document kinds:

- flight itinerary
- hotel folio
- receipt

The relevant code lives in:

- [src/synthetic_documents.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/synthetic_documents.rs:1)
- [src/document_extract.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/document_extract.rs:1)
- [src/document_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/document_facts.rs:1)

## What gets generated

`generate_synthetic_documents` writes markdown documents plus their expected extracted fact payloads:

- `<document>.md`
- `<document>.md.expected.json`
- `manifest.json`

The markdown files are meant to stand in for OCR-transcribed markdown.

This repo also includes a checked-in curated corpus of more realistic markdown transcriptions under `fixtures/curated/`:

- `fixtures/curated/flight_itinerary/`
- `fixtures/curated/hotel_folio/`
- `fixtures/curated/receipt/`

Those fixtures are paired with expected `ExtractedDocumentFacts` values in [src/curated_corpus.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/curated_corpus.rs:1), and they are meant to pin the interface against layouts that look closer to real OCR output than the generated smoke fixtures.

## Commands

Run the full Rust test suite:

```bash
cargo test
```

Generate a baseline synthetic packet:

```bash
cargo run --bin generate_synthetic_documents -- --output-dir /tmp/expense_synth_baseline --variant baseline
```

Generate a noisier packet with heading/casing/bullet variation:

```bash
cargo run --bin generate_synthetic_documents -- --output-dir /tmp/expense_synth_noisy --variant noisy
```

Generate a single document kind:

```bash
cargo run --bin generate_synthetic_documents -- --output-dir /tmp/expense_synth_receipt --variant baseline --kind receipt
```

Extract document facts from a generated markdown document:

```bash
cargo run --bin extract_document_facts -- /tmp/expense_synth_baseline/synthetic_receipt_baseline.md
```

Verify the checked-in curated corpus:

```bash
cargo run --bin verify_curated_corpus
```

Transcribe a markdown document through the same transcription layer:

```bash
cargo run --bin transcribe_document -- /tmp/expense_synth_baseline/synthetic_receipt_baseline.md
```

## What is tested

The current tests cover:

- baseline round-trip extraction for all three synthetic document kinds
- noisy round-trip extraction for all three synthetic document kinds
- curated realistic corpus regression checks for flights, hotel folios, and receipts
- deterministic inference when a flight trip window is missing
- deterministic inference when hotel and receipt totals are missing
- error reporting when a non-derivable required field is missing
- label parsing regressions for hyphenated and slashed field names
- receipt row parsing regressions for dotted leaders, inline currency codes, and uppercase tax lines

## Why this matters

These fixtures give the document-fact layer a stable contract and a repeatable evaluation set before real receipts and folios are available. The generated packet is useful for quick smoke coverage, while the curated corpus is the stricter regression suite that should fail whenever interface behavior changes unexpectedly.
