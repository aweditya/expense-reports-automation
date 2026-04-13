# Review Packet

This repo now has a typed FA handoff artifact between bundle validation and any future UI.

The relevant code lives in:

- [src/review_packet.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/review_packet.rs:1)
- [src/readiness.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/readiness.rs:1)
- [src/bin/build_review_packet_from_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/build_review_packet_from_facts.rs:1)
- [src/bin/verify_review_regressions.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/verify_review_regressions.rs:1)

The review packet is the typed backend artifact for the `FA Review Workbench` handoff from [system-architecture.md](/Users/adityasriram/Labs/stanford/research/expense-reports/system-architecture.md:297).

The deterministic HTML presentation built on top of it now lives in [review-workbench.md](/Users/adityasriram/Labs/stanford/research/expense-reports/review-workbench.md:1).

## What it contains

- packet summary
- issue queue derived from validation plus readiness classification
- Oracle-ordered copy sections derived from [`generated/ui_field_map.yaml`](./generated/ui_field_map.yaml)
- attachment checklist per projected transaction line

## Why it exists

The validator answers "is this draft schema-complete?" The review packet answers:

- what can the FA copy right now
- what is blocked on automation
- what is blocked on user input
- what still needs manual review even if it is populated

That is the operational boundary the HTML workbench and any future richer UI should consume.

## Commands

Build a markdown review packet from curated fact sidecars:

```bash
cargo run --bin build_review_packet_from_facts -- --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Render the same packet as JSON:

```bash
cargo run --bin build_review_packet_from_facts -- --output json --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Verify the fixture-backed regression corpus for the review packet interface:

```bash
cargo run --bin verify_review_regressions
```

## Current behavior

For the curated FX-enabled packet, the review packet currently reports:

- `0` automation gaps
- `4` user-input gaps
- `3` manual-review items

Those remaining user-input gaps are intentional:

- `general_information.payee.affiliation`
- `general_information.authorized_by`
- `general_information.student_certification` dependency on affiliation
- `transaction_lines[].meal_details.attendees`
