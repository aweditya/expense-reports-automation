# Review Submission Ledger

This repo now has a typed, versioned ledger that ties draft versions, FA review actions, submission attempts, and Stanford-site returns into one artifact.

The relevant code lives in:

- [src/ledger.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/ledger.rs:1)
- [src/ledger_regression.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/ledger_regression.rs:1)
- [src/bin/build_review_submission_ledger_from_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/build_review_submission_ledger_from_facts.rs:1)
- [src/bin/verify_ledger_regressions.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/verify_ledger_regressions.rs:1)

This is the concrete backend realization of the versioned workflow implied by the `FA Review Workbench -> submission -> site return -> correction` portion of [system-architecture.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/system-architecture.md:335).

## What it records

- machine-projected draft versions
- FA-authored review revisions
- field-level review actions and confirmations
- submission attempts against a specific draft version
- site outcomes: accepted, returned, or rejected
- site-return corrections as a new draft version instead of an in-place mutation
- feedback capture attached to returned/corrected attempts

## State model

At the ledger level, the workflow now moves through:

`automation_blocked | user_input_required | manual_review_required | ready_to_file -> submitted -> accepted | returned | rejected`

If a returned submission is corrected, the ledger creates a new `site_return_revision` draft version and can move back to `ready_to_file`.

## Commands

Build the ledger from curated fact sidecars and stop at the initial machine draft:

```bash
cargo run --bin build_review_submission_ledger_from_facts -- --fx demo --scenario initial \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Render a returned-and-corrected ledger path:

```bash
cargo run --bin build_review_submission_ledger_from_facts -- --fx demo --scenario returned \
  fixtures/curated/flight_itinerary/airline_itinerary_trip_window.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_property_labeled.md.expected.json \
  fixtures/curated/receipt/receipt_merchant_labeled.md.expected.json
```

Verify the fixture-backed ledger regressions:

```bash
cargo run --bin verify_ledger_regressions
```

Refresh the checked-in ledger goldens after an intentional workflow change:

```bash
cargo run --bin export_ledger_regressions
```

## Why it exists

The review packet and workbench are point-in-time surfaces. The ledger is the durable history.

That split matters because it lets the system:

- preserve the machine draft and every human correction
- attach site outcomes to the exact draft that was submitted
- distinguish FA fixes from site-return fixes
- feed corrected versions back into evaluation and learning without losing provenance
