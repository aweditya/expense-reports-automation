//! Per-receipt deserialization target.
//!
//! Architecture B (see docs/redesign-plan.md): the per-receipt JSON the
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
    /// ExpenseReport.transaction_lines[].common.source_documents[].filename.
    #[serde(default)]
    pub source_filename: String,
    pub expense_kind: String,
    /// Schema-shaped fields. `#[serde(flatten)]` reads `common`/`meal_details`/etc.
    /// into the existing `ExpenseReportTransactionLinesItem` shape.
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
                "expense_kind": "meal",
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
        assert_eq!(r.expense_kind, "meal");
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
}
