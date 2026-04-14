# Feedback Capture

This repo now has a typed feedback artifact for comparing a machine-generated draft against a corrected draft and recording Stanford-site outcomes.

The relevant code lives in:

- [src/feedback.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/feedback.rs:1)
- [src/feedback_regression.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/feedback_regression.rs:1)
- [src/bin/capture_feedback_from_drafts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/capture_feedback_from_drafts.rs:1)
- [src/bin/verify_feedback_regressions.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/verify_feedback_regressions.rs:1)

This is the current backend implementation of the `Feedback capture and continuous improvement` stage from [system-architecture.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/system-architecture.md:338).

For the versioned workflow wrapper around those corrections and site outcomes, see [review-submission-ledger.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/review-submission-ledger.md:1).

## What it captures

- original machine value
- corrected human value
- operation type: added, updated, or cleared
- schema source tier and machine confidence
- machine evidence and corrected evidence
- whether the field had a preexisting readiness issue
- suggested taxonomy for the correction
- optional confirmed correction reason and note
- optional Stanford-site submission outcome and returned-field messages

## Commands

Run the small standalone example as markdown:

```bash
cargo run --bin capture_feedback_from_drafts -- \
  --original examples/feedback_original.yaml \
  --corrected examples/feedback_corrected.yaml \
  --annotations examples/feedback_annotations.yaml \
  --site-feedback examples/feedback_submission.yaml \
  --validation examples/feedback_validation.json
```

Render the same artifact as JSON:

```bash
cargo run --bin capture_feedback_from_drafts -- \
  --original examples/feedback_original.yaml \
  --corrected examples/feedback_corrected.yaml \
  --annotations examples/feedback_annotations.yaml \
  --site-feedback examples/feedback_submission.yaml \
  --validation examples/feedback_validation.json \
  --output json
```

Verify the fixture-backed regression suite:

```bash
cargo run --bin verify_feedback_regressions
```

Refresh the checked-in feedback goldens after an intentional artifact change:

```bash
cargo run --bin export_feedback_regressions
```

## Why it exists

The workbench tells the FA what to review and copy. The feedback artifact tells the system what the machine got wrong after that review happened.

That gives the pipeline a structured learning signal for:

- extraction failures
- schema projection failures
- deterministic derivation mistakes
- Stanford policy mismatches
- Stanford site workflow mismatches
