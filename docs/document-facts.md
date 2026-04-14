# Document Fact Extraction Design

## Purpose

The document-fact layer sits between transcription and schema projection.

Its job is to answer:

- what kind of document is this?
- what normalized facts can we safely extract from this single document?
- what evidence supports each fact?
- what extraction issues or review risks remain?

It should **not** answer bundle-level questions like:

- which receipt belongs to which trip?
- whether the travel is domestic or foreign across the whole packet
- how multiple documents should be merged into one final expense line

Those are later stages: bundle synthesis and schema projection.

## Contract

The typed Rust contract lives in [src/document_facts.rs](/Users/adityasriram/Labs/stanford/research/expense-reports/src/document_facts.rs:1).

The top-level type is:

- `ExtractedDocumentFacts`

It contains:

- `document_id`
- `filename`
- `classification`
- `extraction_status`
- `facts`
- `issues`

### Classification

Classification is evidence-bearing and explicit:

- `DocumentClassification { kind, confidence, evidence, flags }`

This is important because classification is itself an extraction step that can fail or become ambiguous.

### Evidence-bearing fact wrapper

Every normalized fact uses:

- `Observed<T> { value, confidence, evidence, flags }`

This mirrors the schema draft metadata layer, but earlier in the pipeline. The idea is:

- document extraction attaches evidence to raw normalized facts
- schema projection later decides where those facts land in the expense-report schema

### Extraction status

`ExtractionStatus` is one of:

- `Complete`
- `Partial`
- `NeedsReview`
- `Unsupported`

This lets the orchestrator distinguish:

- “parser worked and extracted what it should”
- “parser extracted something useful but not enough for auto-projection”
- “human review required before downstream use”
- “no parser available / document unsupported”

### Extraction issues

`DocumentExtractionIssue` captures parser-level concerns such as:

- low OCR quality
- ambiguous merchant name
- conflicting date ranges inside one document
- required section missing
- unsupported layout

These are document-local issues, not schema-validation issues.

## Supported document fact payloads

The payload enum is `DocumentFactsPayload`.

Current variants:

- `FlightItinerary`
- `HotelFolio`
- `Receipt`
- `ConferenceRegistration`
- `ConferenceProgram`
- `CurrencyConversion`
- `AirfarePriceComparison`
- `MissingReceiptDeclaration`
- `StanfordExpenseSummary`
- `Unknown`

Each payload is a typed struct with only fields that make sense for that document kind.

Examples:

- `FlightItineraryFacts` has traveler names, trip window, locations, and flight segments.
- `HotelFolioFacts` has guest name, stay window, nightly charges, and total paid.
- `ReceiptFacts` has merchant, date, total, tax, tip, and line items.
- `StanfordExpenseSummaryFacts` captures the structured fields visible in the summary PDFs already in this repo.

## Invariants

The layer should obey five rules:

1. A document fact object is always **single-document scoped**.
2. Every classification must include evidence.
3. Every extracted value that may drive money, dates, travel windows, or identity should be wrapped in `Observed<T>`.
4. Missing or ambiguous information should stay missing or become an issue; it should not be guessed.
5. Cross-document merging belongs later.

The contract exposes `validate_contract()` to enforce the most basic structural invariant:

- classification kind must match payload kind
- classification must carry evidence

## Relationship to the existing code

Right now the working vertical slice is:

- `transcribe_document`
- `bootstrap_extract_summary_pdf`
- `validate_draft_instance`

That bootstrap extractor currently projects directly from Stanford summary PDFs into schema-shaped draft instances.

The new document-fact layer is the abstraction we should move that logic toward:

- `PDF -> transcription artifact`
- `transcription artifact -> ExtractedDocumentFacts`
- `ExtractedDocumentFacts -> canonical graph / schema projection`

So this contract is the missing middle layer we wanted before generating fake documents.

## Why this comes before synthetic documents

Synthetic docs should be generated against a target contract, not against vague expectations.

That means the generator will eventually take inputs like:

- desired document kind
- desired extracted facts
- desired ambiguity/noise profile

and produce fake PDFs or PNGs that are meant to yield those facts.

Without the document-fact contract first, synthetic docs would be hard to evaluate because “correct extraction” would still be underspecified.

## Implemented next step

The first synthetic extraction loop now exists for:

1. `FlightItineraryFacts`
2. `HotelFolioFacts`
3. `ReceiptFacts`

See [synthetic-pipeline.md](/Users/adityasriram/Labs/stanford/research/expense-reports/docs/synthetic-pipeline.md:1) for the generator, extractor, and testing commands.
