# Synthetic Corpus Evaluation

This repo now has a deterministic large-corpus generator and evaluator for the full data/validation/review/feedback spine.

The relevant code lives in:

- [src/synthetic_corpus.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/synthetic_corpus.rs:1)
- [src/corpus_eval.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/corpus_eval.rs:1)
- [src/bin/generate_synthetic_corpus.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/generate_synthetic_corpus.rs:1)
- [src/bin/evaluate_synthetic_corpus.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/evaluate_synthetic_corpus.rs:1)

It exercises the pipeline slice:

`synthetic markdown docs -> extraction -> bundle synthesis -> validation/readiness -> review packet -> workbench -> review/submission ledger`

## What it covers

The corpus generator produces travel packets with:

- flight itinerary, hotel folio, and meal-style receipt documents
- baseline and noisy markdown styles
- multiple destinations and currencies
- explicit and inferred trip windows
- explicit and inferred hotel totals
- explicit and inferred receipt totals
- accepted and returned-and-corrected submission scenarios
- alcohol and non-alcohol meal receipts

The evaluator checks that:

- extracted facts exactly match the synthetic ground truth
- extracted facts still satisfy the document-fact contract
- bundle projection succeeds with demo FX support
- review packets build
- workbench HTML renders
- a synthetic FA revision moves each packet to `ready_to_file`
- accepted and returned/corrected ledger scenarios both complete cleanly

## Commands

Generate a filesystem corpus you can inspect manually:

```bash
cargo run --bin generate_synthetic_corpus -- --output-dir /tmp/expense_corpus_large --packets 128
```

Run a large end-to-end evaluation and print a readable summary:

```bash
cargo run --bin evaluate_synthetic_corpus -- --packets 512 --output markdown
```

Render the same evaluation as JSON:

```bash
cargo run --bin evaluate_synthetic_corpus -- --packets 512 --output json
```

Run the focused unit tests for the corpus layer:

```bash
cargo test corpus_eval -- --nocapture
```

## Current result

The current implementation runs clean on a 512-packet corpus:

- 512 packets
- 1,536 documents
- 1,536 exact fact matches
- 512 review packets
- 512 workbench renders
- 512 ledger initializations
- 512 successful FA-ready revisions
- 256 accepted submissions
- 256 returned-and-corrected submissions
- 0 failures

## Why it exists

Small curated fixtures are good for stable regressions. They are not enough to shake out interaction bugs across extraction, inference, review, and submission states.

The large synthetic corpus is the stress harness for that middle-to-downstream spine. It gives the project a reproducible way to harden the interfaces before real OCR and real Stanford-site integration are introduced.
