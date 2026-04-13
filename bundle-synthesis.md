# Bundle Synthesis

This repo now has a typed bundle-synthesis layer between document extraction and schema validation.

The relevant code lives in:

- [src/bundle_synthesis.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bundle_synthesis.rs:1)
- [src/bin/synthesize_bundle_from_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/bin/synthesize_bundle_from_facts.rs:1)

This is the architecture slice described in [system-architecture.md](/Users/adityasriram/Labs/stanford/research/expense-reports/system-architecture.md:192):

`ExtractedDocumentFacts -> CanonicalExpenseBundle -> DraftReport -> validator`

## What the bundle layer does

The bundle layer currently synthesizes:

- canonical payee
- canonical trip window, destination, and domestic/foreign region
- canonical expense lines for airfare and lodging
- heuristic meal projection for restaurant-style receipts
- review-marked scaffolds for business purpose text, event name, meal purpose, and beneficiary defaults
- generic receipt coverage records for receipts that are still not schema-projectable
- optional FX enrichment for foreign-currency lines through a pluggable rate provider
- synthesis issues for conflicts and projection gaps

It then projects the bundle into a partial evidence-bearing draft report and runs the existing validator on that draft.

## What it does not do yet

It does not yet:

- fully classify all generic receipts into meal vs. transport vs. other schema expense types
- connect to a live FX source or persist enrichment provenance beyond the current draft metadata
- infer user-owned fields like affiliation, authorization, attendee lists, or other approver context
- generate final-ready reports with zero validation errors

That is expected for this stage. The purpose of the layer is to make cross-document synthesis explicit and testable before adding more inference.

## Commands

Run the bundle-synthesis focused tests:

```bash
cargo test bundle_synthesis -- --nocapture
```

Run the fixture-backed bundle regression verifier:

```bash
cargo run --bin verify_bundle_regressions
```

Synthesize a draft report from curated expected fact JSON sidecars:

```bash
cargo run --bin synthesize_bundle_from_facts -- \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Run the same packet with demo FX enrichment enabled:

```bash
cargo run --bin synthesize_bundle_from_facts -- --fx demo \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Today, that FX-enabled packet validates with only four remaining errors, all by design:

- `general_information.payee.affiliation`
- `general_information.authorized_by`
- `general_information.student_certification` dependency on affiliation
- `transaction_lines[].meal_details.attendees`

Render the same input as canonical bundle JSON instead of draft YAML:

```bash
cargo run --bin synthesize_bundle_from_facts -- --output bundle-json \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

Render the projected draft as JSON:

```bash
cargo run --bin synthesize_bundle_from_facts -- --output draft-json \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json \
  fixtures/curated/receipt/receipt_card_dotted.md.expected.json
```

## Current test coverage

The bundle tests currently check:

- consistent payee/trip synthesis from a multi-document packet
- conflict detection for mismatched payee names
- curated sidecar compatibility
- meal classification from restaurant-like receipts
- FX enrichment and USD total projection for foreign lodging and meal lines
- partial draft projection with field metadata
- expected validation gaps for user-input and FX-dependent fields
- missing-USD-conversion warnings for foreign lodging
- review metadata on low-confidence defaulted fields
- regression snapshots for realistic curated packets with and without FX enrichment
