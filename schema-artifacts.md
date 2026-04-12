# Schema-Derived Artifacts

This project now generates three concrete artifacts from [`schema.yaml`](./schema.yaml):

- [`generated/expense_report_model.py`](./generated/expense_report_model.py)
- [`generated/validation_rules.yaml`](./generated/validation_rules.yaml)
- [`generated/ui_field_map.yaml`](./generated/ui_field_map.yaml)

Generation command:

```bash
python3 scripts/generate_schema_artifacts.py
```

## What "typed model" means

A typed model is a programming-language representation of the schema shape.

In this repo, the typed model is the generated Python module:

- nested `TypedDict` classes for objects
- `Literal[...]` aliases for enums
- `Decimal` for money-like numeric fields
- `date` for date fields
- `Required[...]` and `NotRequired[...]` markers for always-required vs conditionally/optionally present fields

Example:

```python
class ExpenseReportTransactionSummary(TypedDict, total=False):
    transaction_type: Required[ExpenseReportTransactionSummaryTransactionTypeEnum]
    transaction_number: NotRequired[str]
    transaction_date: Required[date]
    total_usd: Required[Decimal]
```

Why this matters:

- editors and static analyzers can catch shape mismatches early
- downstream code has one canonical object structure
- code can distinguish strings, dates, enums, and money amounts instead of treating everything as untyped YAML

## What "validation rule set" means

The validation rule set is the runtime contract extracted from the schema into a normalized, machine-readable format.

In this repo, that is [`generated/validation_rules.yaml`](./generated/validation_rules.yaml). It flattens the schema into rules such as:

- field path
- schema type
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
