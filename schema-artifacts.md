# Schema-Derived Artifacts

This project now generates three concrete artifacts from [`schema.yaml`](./schema.yaml):

- [`generated/expense_report_model.rs`](./generated/expense_report_model.rs)
- [`generated/validation_rules.yaml`](./generated/validation_rules.yaml)
- [`generated/ui_field_map.yaml`](./generated/ui_field_map.yaml)

It also generates a Rust-native validation artifact:

- [`generated/validation_rules.rs`](./generated/validation_rules.rs)

The Rust crate entrypoint is [`src/lib.rs`](./src/lib.rs).
The runtime validator built on top of these artifacts is in [`src/validator.rs`](./src/validator.rs).
The file loader is in [`src/parse.rs`](./src/parse.rs), and the CLI entrypoint is [`src/bin/validate_report.rs`](./src/bin/validate_report.rs).
The draft-instance metadata parser is in [`src/draft.rs`](./src/draft.rs), with a dedicated CLI in [`src/bin/validate_draft_instance.rs`](./src/bin/validate_draft_instance.rs).

Generation command:

```bash
python3 scripts/generate_schema_artifacts.py
```

## What "typed model" means

A typed model is a programming-language representation of the schema shape.

In this repo, the typed model is the generated Rust module:

- nested `struct`s for objects
- `enum`s for constrained values
- `Option<T>` for conditional or optional fields
- `Vec<T>` for repeated fields
- lightweight wrapper types `IsoDate` and `DecimalAmount` so dates and amounts are not just plain strings

Example:

```rust
pub struct ExpenseReportTransactionSummary {
    pub transaction_type: ExpenseReportTransactionSummaryTransactionTypeEnum,
    pub transaction_number: Option<String>,
    pub transaction_date: IsoDate,
    pub total_usd: DecimalAmount,
}
```

Why this matters:

- editors and static analyzers can catch shape mismatches early
- downstream code has one canonical object structure
- code can distinguish strings, dates, enums, and money amounts instead of treating everything as untyped YAML
- the crate can be compiled directly with `cargo check`

## What "validation rule set" means

The validation rule set is the runtime contract extracted from the schema into a normalized, machine-readable format.

In this repo, that contract is produced in two forms:

- [`generated/validation_rules.yaml`](./generated/validation_rules.yaml) for human inspection and debugging
- [`generated/validation_rules.rs`](./generated/validation_rules.rs) for Rust code to consume directly

They flatten the schema into rules such as:

- field path
- schema type
- Rust type
- effective source (`T1`, `T2`, `T3`)
- always-required status
- conditional required expressions
- enum allowed values
- dependency references
- free-form validation expressions

Example rule:

```yaml
- path: expense_report.general_information.rush_processing
  schema_type: enum
  rust_type: ExpenseReportGeneralInformationRushProcessingEnum
  required: true
  allowed_values: ['yes', 'no']
  source: T1
```

Why this matters:

- the validator can iterate over rules instead of hardcoding field checks
- conditional requirements can be enforced consistently
- the review UI can surface the exact reason a field is blocked or flagged
- future schema changes regenerate the rule set instead of requiring hand-maintained validator code

The current validator uses the generated Rust rule tables to check:

- required fields
- conditional requiredness
- basic type compatibility
- enum membership
- dependency presence

You can validate a draft file directly with:

```bash
cargo run --bin validate_report -- examples/minimal_report.yaml
```

You can validate an evidence-bearing extracted draft instance with:

```bash
cargo run --bin validate_draft_instance -- examples/draft_instance.yaml
```

The parser accepts both:

- a top-level wrapped document with `expense_report: ...`
- an unwrapped document whose root is the report object itself

For draft instances, leaf fields can use the schema’s wrapped form:

```yaml
class_of_ticket:
  value: coach
  _meta:
    confidence: high
    source_document: flight_confirmation.pdf
    needs_review: false
    flags: []
```

## What the UI field map is for

[`generated/ui_field_map.yaml`](./generated/ui_field_map.yaml) is the starting point for the FA review surface. It organizes leaf fields by section and includes:

- label
- control type
- source-aware entry mode
- requiredness
- review priority

This is the bridge from schema structure to a practical review/copy workflow.

## Why the generator is still Python

The typed model is now Rust, but the code generator remains Python because the repo already has local YAML parsing available without introducing new network-fetched Rust dependencies. The generator is a build-time tool; the generated schema contract that downstream code uses is Rust.
