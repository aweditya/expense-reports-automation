# Review Workbench

This repo now has a deterministic FA-facing HTML workbench layered on top of the typed review packet.

The relevant code lives in:

- [src/review_workbench.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_workbench.rs:1)
- [src/review_workbench.css](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_workbench.css:1)
- [src/workbench_regression.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/workbench_regression.rs:1)
- [src/bin/build_review_workbench_from_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/build_review_workbench_from_facts.rs:1)
- [src/bin/verify_workbench_regressions.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/verify_workbench_regressions.rs:1)

The workbench is the first concrete backend realization of the `FA Review Workbench` stage from [system-architecture.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/system-architecture.md:297). It is still static HTML, but it now behaves like a lightweight review workstation rather than a passive report page.

## What it renders

- packet summary with filing readiness and confidence
- issue queue with jump links into the copy surface
- Oracle-ordered field cards with inline editable controls
- missing required fields rendered as blank inputs instead of literal `[missing]`
- machine-filled values editable in place, with copy buttons reading the current control value
- inline expandable evidence per field, including raw-document links
- in-page document preview modal for uploaded source files
- attachment checklist per projected transaction line

## Commands

Build the workbench HTML from curated fact sidecars:

```bash
cargo run --bin build_review_workbench_from_facts -- --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Verify the fixture-backed regression corpus for the workbench interface:

```bash
cargo run --bin verify_workbench_regressions
```

Refresh the checked-in HTML goldens after an intentional renderer change:

```bash
cargo run --bin export_workbench_regressions
```

## Why it exists

The typed review packet is the contract. The workbench is the deterministic presentation layer on top of that contract.

That split lets us:

- regression-test the packet as structured data
- regression-test the FA surface as stable HTML
- change styling and layout without changing synthesis or validation logic
- eventually swap the static renderer for a richer UI without losing the packet boundary
