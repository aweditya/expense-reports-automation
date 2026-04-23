# FA Schema Rationale Roadmap

This note captures the next schema/UI layer for the FA-facing workflow.

## Problem

The current schema is good at describing:

- data shape
- requiredness
- source tier (`T1` / `T2` / `T3`)
- allowed values
- dependencies
- extraction hints like `infer_from`

But it is missing the human-facing "why" layer that an FA needs while editing a filing packet.

Today the developer and FA audiences are mixed together:

- `infer_from` is useful for extraction/debug logic
- field descriptions are sometimes partly human-facing and partly agent-facing
- the FA surface can explain what a field is, but not yet why Stanford cares about it

## Proposed Schema Metadata

Add optional FA-facing metadata at the schema-node level:

- `fa_label`
  - short human-readable field label for the FA workbench and preview
- `policy_rationale`
  - why the field exists or is required
- `consequence`
  - what happens if the field is missing or wrong
- `condition_rationale`
  - why a conditional requirement activates
- `derivation`
  - human-readable explanation of how a computed `T2` field is derived

## UI Mapping

### FA Workbench

- `fa_label`
  - primary field label instead of raw schema-tail naming
- `policy_rationale`
  - inline help or tooltip on required fields
- `consequence`
  - urgency styling:
    - blocks payment
    - likely return
    - audit risk
    - informational
- `condition_rationale`
  - short "why this is showing up" help for activated conditional fields
- `derivation`
  - collapsed "How was this computed?" disclosure on readonly/system-generated values

### Developer Surface

Keep the existing engineering metadata visible:

- schema path
- `infer_from`
- `depends_on`
- raw validation/readiness state
- OCR/debug artifacts

The developer route can show both the FA-facing and engineering-facing metadata together.

## Highest-Priority Fields

These are the first fields that most need FA-facing rationale text:

1. `general_information.business_purpose.{who,what,when,where,why}`
2. `general_information.business_purpose.key_30char`
3. `transaction_lines[].airfare_details.class_of_ticket`
4. `general_information.student_certification.*`
5. `transaction_lines[].meal_details.{attendees,has_alcohol_on_receipt,alcohol_amount}`
6. `transaction_lines[].common.{original_currency,original_amount,exchange_rate}`
7. `transaction_lines[].per_diem_details.*`
8. `transaction_lines[].human_subject_incentive_details.irb_protocol_number`
9. `transaction_lines[].lodging_details.personal_nights_excluded`
10. `transaction_lines[].conference_details.meals_included`

## Implementation Order

1. Add optional FA-facing metadata fields to `schema.yaml`
2. Extend codegen so those fields appear in generated validation/UI metadata
3. Update the packet builder to carry `fa_label`, help text, and rationale fields
4. Render rationale in the FA workbench and preview
5. Keep developer-specific metadata on the developer route only

## Interim Rule

Until the schema is extended:

- use friendlier FA labels in the packet/workbench layer
- keep generic editing guidance minimal
- avoid showing raw schema tails or extraction jargon on the FA route
