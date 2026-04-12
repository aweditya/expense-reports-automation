# Schema-Derived Artifacts

This project now generates three concrete artifacts from [`schema.yaml`](./schema.yaml):

- [`generated/expense_report_model.rs`](./generated/expense_report_model.rs)
- [`generated/validation_rules.yaml`](./generated/validation_rules.yaml)
- [`generated/ui_field_map.yaml`](./generated/ui_field_map.yaml)

The Rust crate entrypoint is [`src/lib.rs`](./src/lib.rs).

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

In this repo, that is [`generated/validation_rules.yaml`](./generated/validation_rules.yaml). It flattens the schema into rules such as:

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
