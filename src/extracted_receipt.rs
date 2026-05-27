//! Per-receipt deserialization target.
//!
//! the per-receipt JSON the
//! extractor writes carries both schema-shaped fields AND an `extras` block.
//! The schema-shaped fields flow into `ExpenseReport`; extras live only at
//! this layer and feed reduction (e.g. FX, foreign-vs-domestic decisions).
//!
//! `ExtractedReceipt` is the wrapper Python writes per file; it deserializes
//! a single transaction line plus its extras.

use serde::{Deserialize, Serialize};

use crate::expense_report_model::ExpenseReportTransactionLinesItem;
use crate::meta::Wrapped;

/// One receipt as it comes out of the Python extractor: the schema-shaped
/// transaction line plus the extracted `extras` plus the source filename.
/// Stored one-per-file under `.scratch/spike/<name>.json` (today; production
/// path TBD).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedReceipt {
    /// Original FA-uploaded filename including extension (e.g.
    /// "mjsushi.jpeg"). Injected by the extractor post-Gemini-call from the
    /// input filename — Gemini doesn't extract this from receipt content.
    /// Reduction uses this to populate
    /// ExpenseReport.transaction_lines[].common.source_document.filename.
    #[serde(default)]
    pub source_filename: String,
    /// Schema-shaped fields. `#[serde(flatten)]` reads `common`/`meal_details`/etc.
    /// into the existing `ExpenseReportTransactionLinesItem` shape.
    ///
    /// The expense kind (meal/lodging/transport/airfare/conference) is no
    /// longer carried as a top-level field on the JSON — the per-kind
    /// extractor script (`scripts/extract_<kind>.py`, dispatched by the
    /// FA's upload-form choice) implies the kind. Structurally it's still
    /// visible: only the matching detail block (`meal_details` /
    /// `lodging_details` / …) is non-null on the line.
    #[serde(flatten)]
    pub line: ExpenseReportTransactionLinesItem,
    pub extras: Extras,
}

/// Per-receipt extras — extracted from the receipt content but NOT in
/// `ExpenseReport`. Reduction reads these to derive things a single
/// receipt can't determine in isolation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Extras {
    /// Full printed merchant address. Drives `country_of_activity` and
    /// `foreign_activity_type` in reduction.
    #[serde(default)]
    pub merchant_address: Wrapped<String>,
    /// Currency literally on the receipt (ISO 4217 if printed, else
    /// inferred from a symbol). Drives `original_currency` and FX.
    #[serde(default)]
    pub printed_currency: Wrapped<String>,
    /// Per-night rate breakdown — only emitted by the lodging extractor.
    /// Reduction averages `rate` across entries to populate
    /// `lodging_details.daily_rate` (T2-derived). Empty for non-lodging
    /// receipts and for lodging folios where the model couldn't recover
    /// the breakdown.
    ///
    /// Plain `Vec` rather than `Wrapped<Vec<…>>`: Vertex's Schema
    /// validator rejects leaf-wrapped arrays with a generic 400. The
    /// "single confidence on the whole breakdown" signal we'd have
    /// carried on the wrapper shows up implicitly downstream — reduction
    /// marks `daily_rate.meta.confidence` as `high` when entries are
    /// present, `low` (Wrapped::unknown) when they aren't.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nightly_rates: Vec<NightlyRate>,
    /// Per-flight-segment breakdown — only emitted by the airfare extractor.
    /// Reduction uses entries to derive segment count (and eventually total
    /// flight time / multi-airline detection). Empty for non-airfare
    /// receipts. Same bare-array rationale as `nightly_rates` above.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
}

/// One night of a lodging stay. The `date`, `rate`, and `taxes_and_fees`
/// are bare values (no per-leaf `_meta`) — keeping per-night cost
/// compact in the output budget (per-leaf `_meta` blocks add overhead;
/// across a multi-night folio truncated the model's output).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NightlyRate {
    /// ISO 8601 date (YYYY-MM-DD) for the night.
    pub date: String,
    /// Room rate that night, in the printed currency (currency itself
    /// lives in `extras.printed_currency`).
    pub rate: f64,
    /// Sum of all taxes/fees that night (VAT, occupancy tax, city tax).
    /// Zero if the folio doesn't break them out.
    pub taxes_and_fees: f64,
}

/// One leg of an airfare itinerary. Bare values, no per-leaf `_meta`,
/// same rationale as `NightlyRate` — keeping per-segment cost compact
/// in the output budget. A round-trip ticket emits 2 entries; a
/// multi-segment trip (e.g. SFO→ORD→FRA + return) emits 3-4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Segment {
    /// Carrier code + number (e.g. "UA1448", "AI179").
    pub flight_number: String,
    /// IATA code of the segment's origin (e.g. "SFO", "BOM").
    pub from_airport: String,
    /// IATA code of the segment's destination.
    pub to_airport: String,
    /// ISO 8601 local departure datetime (e.g. "2025-11-23T10:30").
    /// Local to the departure airport's timezone — no offset suffix.
    pub departure_datetime: String,
}

/// per-document shape for supporting conference documents
/// (program PDF, paper schedule, conference papers, etc.). NOT an
/// expense — has no `common` block, no transaction line, doesn't appear
/// in `ExpenseReport.transaction_lines`. Sibling type to
/// `ExtractedReceipt`. Reduction reads these to aggregate dates,
/// venues, papers across all supporting docs in an upload, and to feed
/// the synthesis layer.
///
/// Each field is independently optional in practice — a paper-only
/// PDF fills `papers_listed`, leaves `scheduled_dates` empty. Reduction
/// collects whatever's populated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SupportingConferenceDoc {
    /// Original FA-uploaded filename (e.g.
    /// "supporting_2026-03-25_asplos-full-program.pdf"). Injected by
    /// `extract_supporting_conference_doc.py` post-Gemini-call.
    #[serde(default)]
    pub source_filename: String,
    /// Conference name as it appears on the document
    /// (e.g. "ASPLOS 2026"). Synthesis layer canonicalizes across
    /// documents.
    pub conference_name_as_printed: Wrapped<String>,
    /// Document classification: `program` / `schedule` / `papers` /
    /// `other`. Stored as String for now (could enum-ize later if a
    /// downstream rule needs the type checking).
    pub doc_kind: Wrapped<String>,
    /// All ISO 8601 dates mentioned in the document's schedule.
    /// Reduction's min/max gives the conference date range.
    /// Bare array; same rationale as `NightlyRate` / `Segment`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scheduled_dates: Vec<String>,
    /// Venue / room / city names mentioned anywhere in the document.
    /// Reduction unions these to populate `business_purpose.where`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub venues_mentioned: Vec<String>,
    /// Papers listed in the document (titles + raw author strings).
    /// Synthesis uses author strings to infer participant_role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub papers_listed: Vec<PaperListing>,
    /// Workshops / tutorials listed in the document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workshops_listed: Vec<String>,
}

/// One paper entry within a supporting conference doc's listing.
/// Bare values, no per-entry `_meta` (same rationale as `NightlyRate`).
/// `authors_string` is the raw printed author list — synthesis layer
/// matches names within it for participant_role inference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PaperListing {
    pub title: String,
    pub authors_string: String,
}

/// T4 synthesis output. Produced by the
/// `synthesize_conference_bundle.py` orchestrator from all
/// `conference_registration_*` and `supporting_conference_doc_*`
/// per-doc JSONs in an upload. Narrow by design — only fuzzy fields
/// the LLM is uniquely good at, never things a rule could derive
/// (date range, venue list, total cost are all T2 in reduction).
///
/// Reduction reads this file (if present) and lifts its fuzzy fields
/// into `general_information.{event_name, business_purpose.{what, why,
/// who}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SynthesisConferenceBundle {
    /// Synthesis output's own filename (e.g.
    /// "synthesis_conference_bundle.json"). Injected post-call so
    /// round-trip serialization keeps the field set complete.
    #[serde(default)]
    pub source_filename: String,
    /// The canonical event name across all input docs (e.g. "ASPLOS
    /// 2026" canonicalized from "ASPLOS" / "ASPLOS '26" mentions).
    pub canonical_event_name: Wrapped<String>,
    /// `attendee` / `presenter` / `organizer` / `other`. Inferred from
    /// whether the registrant appears in any supporting doc's
    /// `papers_listed[].authors_string`.
    pub participant_role: Wrapped<String>,
    /// Composed phrase about what the trip is for (≤120 chars).
    pub business_purpose_what: Wrapped<String>,
    /// Composed sentence about the FA-relevant reason (≤200 chars).
    pub business_purpose_why: Wrapped<String>,
    /// Per-doc JSON filenames the synthesis read. For workbench
    /// citation ("synthesized from N documents: …") and FA audit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_filenames: Vec<String>,
    /// Synthesis's overall self-rating (`high` / `medium` / `low`).
    /// Drives `needs_review` defaults on lifted general_information
    /// fields.
    pub synthesis_confidence: Wrapped<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::ConfidenceLevel;

    #[test]
    fn deserializes_python_extractor_output() {
        // Trimmed mirror of .scratch/spike/mjsushi.json shape.
        let json = r#"[
            {
                "source_filename": "mjsushi.jpeg",
                "common": {
                    "date": {"value": "2026-05-02", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "line_amount_usd": {"value": 79.59, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "original_currency": {"value": null, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "original_amount": {"value": null, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "expense_type": {"value": "business_meal", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "remarks": {"value": "Dinner at MJ Sushi", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "country_of_activity": {"value": "United States", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "foreign_activity_type": {"value": null, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}}
                },
                "meal_details": {
                    "venue_name": {"value": "MJ Sushi", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "alcohol_amount": {"value": 0.0, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "tip_amount": {"value": 0.0, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                    "has_alcohol_on_receipt": {"value": true, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}}
                },
                "extras": {
                    "merchant_address": {
                        "value": "2305 El Camino Real, Palo Alto, CA 94306",
                        "_meta": {"confidence": "high", "evidence": [{"kind": "document_span", "filename": "mjsushi.jpeg", "page": 1, "quote": "2305 El Camino Real"}], "needs_review": false, "flags": []}
                    },
                    "printed_currency": {
                        "value": "USD",
                        "_meta": {"confidence": "medium", "evidence": [], "needs_review": false, "flags": []}
                    }
                }
            }
        ]"#;

        let receipts: Vec<ExtractedReceipt> = serde_json::from_str(json).expect("deserialize");
        assert_eq!(receipts.len(), 1);
        let r = &receipts[0];
        assert_eq!(r.source_filename, "mjsushi.jpeg");
        // Schema-shaped fields flow through #[serde(flatten)] into `line`.
        assert!((r.line.common.line_amount_usd.value.unwrap() - 79.59).abs() < 1e-9);
        // Extras populate alongside.
        assert_eq!(
            r.extras.merchant_address.value.as_deref(),
            Some("2305 El Camino Real, Palo Alto, CA 94306")
        );
        assert_eq!(r.extras.printed_currency.value.as_deref(), Some("USD"));
        assert_eq!(r.extras.printed_currency.meta.confidence, ConfidenceLevel::Medium);
    }

    #[test]
    fn extras_default_when_omitted() {
        // Some flows may not produce extras (e.g. tests, partial extractions).
        // Default Extras should yield Wrapped::unknown() values.
        let extras = Extras::default();
        assert!(extras.merchant_address.value.is_none());
        assert!(extras.printed_currency.value.is_none());
    }

    #[test]
    fn deserializes_supporting_conference_doc() {
        // Mirror of the per-doc JSON shape extract_supporting_conference_doc.py
        // would produce for an ASPLOS program PDF.
        let json = r#"[
            {
                "source_filename": "supporting_2026-03-25_asplos-full-program.pdf",
                "conference_name_as_printed": {"value": "ASPLOS 2026", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "doc_kind": {"value": "program", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "scheduled_dates": ["2026-03-22", "2026-03-23", "2026-03-25", "2026-03-26"],
                "venues_mentioned": ["Rivers Pre-Function", "Events Center"],
                "papers_listed": [
                    {"title": "Streaming Tensor Program", "authors_string": "Gina Sohn (Stanford), Genghan Zhang (Stanford), Nathan Sobotka (Stanford)"}
                ],
                "workshops_listed": ["GPGPU-18", "MLBench"]
            }
        ]"#;

        let docs: Vec<SupportingConferenceDoc> = serde_json::from_str(json).expect("deserialize");
        assert_eq!(docs.len(), 1);
        let d = &docs[0];
        assert_eq!(d.source_filename, "supporting_2026-03-25_asplos-full-program.pdf");
        assert_eq!(d.conference_name_as_printed.value.as_deref(), Some("ASPLOS 2026"));
        assert_eq!(d.doc_kind.value.as_deref(), Some("program"));
        assert_eq!(d.scheduled_dates.len(), 4);
        assert_eq!(d.venues_mentioned, vec!["Rivers Pre-Function", "Events Center"]);
        assert_eq!(d.papers_listed.len(), 1);
        assert_eq!(d.papers_listed[0].title, "Streaming Tensor Program");
        assert_eq!(d.workshops_listed, vec!["GPGPU-18", "MLBench"]);
    }

    #[test]
    fn deserializes_synthesis_conference_bundle() {
        // Mirror of the per-doc JSON shape synthesize_conference_bundle.py
        // would produce after seeing an ASPLOS bundle.
        let json = r#"[
            {
                "source_filename": "synthesis_conference_bundle.json",
                "canonical_event_name": {"value": "ASPLOS 2026", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "participant_role": {"value": "presenter", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "business_purpose_what": {"value": "Presenting research at ASPLOS 2026", "_meta": {"confidence": "high", "evidence": [], "needs_review": true, "flags": []}},
                "business_purpose_why": {"value": "Presenting the paper Streaming Tensor Program on dynamic parallelism.", "_meta": {"confidence": "medium", "evidence": [], "needs_review": true, "flags": []}},
                "source_filenames": ["conference_2026-01-29_asplos-main-registration.png", "supporting_2026-03-25_asplos-full-program.pdf"],
                "synthesis_confidence": {"value": "high", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}}
            }
        ]"#;

        let bundles: Vec<SynthesisConferenceBundle> = serde_json::from_str(json).expect("deserialize");
        assert_eq!(bundles.len(), 1);
        let b = &bundles[0];
        assert_eq!(b.canonical_event_name.value.as_deref(), Some("ASPLOS 2026"));
        assert_eq!(b.participant_role.value.as_deref(), Some("presenter"));
        assert_eq!(b.business_purpose_what.value.as_deref(), Some("Presenting research at ASPLOS 2026"));
        assert!(b.business_purpose_what.meta.needs_review);
        assert_eq!(b.synthesis_confidence.value.as_deref(), Some("high"));
        assert_eq!(b.source_filenames.len(), 2);
    }
}
