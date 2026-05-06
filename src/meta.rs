//! Per-field metadata wrapping that mirrors the `_meta_convention` block in
//! `schema.yaml`.
//!
//! Every leaf in a generated `ExpenseReport*` struct is a `Wrapped<T>` that
//! carries the value plus the schema's `_meta` block (confidence, evidence,
//! needs_review, flags). This is what lets Python's Gemini output deserialize
//! directly into the Rust types and lets provenance flow end-to-end into the
//! workbench.
//!
//! `FieldMetadata`, `EvidenceReference`, `EvidenceKind`, and `ConfidenceLevel`
//! already exist in `crate::draft` (they pre-date this module). They are
//! re-exported here so callers and the codegen have one canonical import.
//! When the rest of the codebase finishes migrating to leaf-wrapped types,
//! the originals can move into this module and the re-export becomes the
//! definition.

use serde::{Deserialize, Serialize};

pub use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference, FieldMetadata};

/// A leaf value paired with its `_meta` provenance block.
///
/// The JSON shape is `{"value": <T or null>, "_meta": {...}}`. The `_meta`
/// field is `#[serde(default)]` so JSON written without it (e.g., test
/// fixtures, hand-edited values) still deserializes — defaults yield a
/// `FieldMetadata` with `Low` confidence, no evidence, `needs_review: false`,
/// no flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wrapped<T> {
    pub value: Option<T>,
    #[serde(default, rename = "_meta")]
    pub meta: FieldMetadata,
}

impl<T> Wrapped<T> {
    /// Construct a wrapped value with default `_meta`. Useful for tests and
    /// for code that constructs values without genuine provenance.
    pub fn known(value: T) -> Self {
        Self {
            value: Some(value),
            meta: FieldMetadata::default(),
        }
    }

    /// Construct a wrapped null with default `_meta`.
    pub fn unknown() -> Self {
        Self {
            value: None,
            meta: FieldMetadata::default(),
        }
    }

    /// True when this wraps a missing value with no meaningful provenance.
    /// Codegen uses this for `skip_serializing_if` so fields Python omits
    /// don't get re-emitted as `{value: null, _meta: {default}}` on the
    /// way back out — keeps the round-trip lossless.
    pub fn is_unknown(&self) -> bool {
        self.value.is_none() && self.meta == FieldMetadata::default()
    }
}

impl<T> Default for Wrapped<T> {
    fn default() -> Self {
        Self::unknown()
    }
}

// `FieldMetadata` (defined in draft.rs) doesn't currently impl `Default`
// because `ConfidenceLevel` doesn't either. The codegen will require both —
// every leaf needs to round-trip through Default for `Wrapped::unknown()`
// to compile. Add the Default impls here so we don't have to touch draft.rs
// unrelatedly.

impl Default for ConfidenceLevel {
    fn default() -> Self {
        Self::Low
    }
}

impl Default for FieldMetadata {
    fn default() -> Self {
        Self {
            confidence: ConfidenceLevel::default(),
            evidence: Vec::new(),
            needs_review: false,
            flags: Vec::new(),
            confidence_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_round_trips_through_json() {
        let wrapped = Wrapped {
            value: Some("2026-05-02".to_owned()),
            meta: FieldMetadata {
                confidence: ConfidenceLevel::High,
                evidence: vec![EvidenceReference {
                    kind: EvidenceKind::DocumentSpan,
                    document_id: Some("mjsushi".to_owned()),
                    filename: Some("mjsushi.jpeg".to_owned()),
                    page: Some(1),
                    quote: Some("05/02/26".to_owned()),
                    origin: None,
                }],
                needs_review: false,
                flags: Vec::new(),
                confidence_reason: None,
            },
        };

        let json = serde_json::to_string(&wrapped).expect("serialize");
        let parsed: Wrapped<String> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, wrapped);
    }

    #[test]
    fn wrapped_uses_underscore_prefixed_meta_in_json() {
        let wrapped = Wrapped::known("hello".to_owned());
        let json = serde_json::to_string(&wrapped).expect("serialize");
        assert!(json.contains("\"_meta\""), "expected '_meta' key, got: {json}");
        assert!(!json.contains("\"meta\":"), "should not contain bare 'meta' key");
    }

    #[test]
    fn wrapped_deserializes_without_meta_using_defaults() {
        let json = r#"{"value": "x"}"#;
        let parsed: Wrapped<String> = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.value.as_deref(), Some("x"));
        assert_eq!(parsed.meta.confidence, ConfidenceLevel::Low);
        assert!(parsed.meta.evidence.is_empty());
        assert!(!parsed.meta.needs_review);
    }

    #[test]
    fn wrapped_deserializes_python_extractor_output_shape() {
        // Sample shape Python's per-kind extractor writes for a leaf.
        let json = r#"{
            "value": 79.59,
            "_meta": {
                "confidence": "high",
                "evidence": [
                    {
                        "kind": "document_span",
                        "filename": "mjsushi.jpeg",
                        "page": 1,
                        "quote": "Total $79.59"
                    }
                ],
                "needs_review": false,
                "flags": []
            }
        }"#;
        let parsed: Wrapped<f64> = serde_json::from_str(json).expect("deserialize");
        assert!((parsed.value.unwrap() - 79.59).abs() < 1e-9);
        assert_eq!(parsed.meta.confidence, ConfidenceLevel::High);
        assert_eq!(parsed.meta.evidence.len(), 1);
        assert_eq!(parsed.meta.evidence[0].quote.as_deref(), Some("Total $79.59"));
    }

    #[test]
    fn known_constructor_uses_default_meta() {
        let w: Wrapped<String> = Wrapped::known("foo".to_owned());
        assert_eq!(w.value.as_deref(), Some("foo"));
        assert_eq!(w.meta.confidence, ConfidenceLevel::Low);
        assert!(!w.meta.needs_review);
    }

    #[test]
    fn unknown_constructor_yields_null_value() {
        let w: Wrapped<String> = Wrapped::unknown();
        assert!(w.value.is_none());
    }

    /// M6.1.e: prove the Python extractor's JSON deserializes directly into
    /// the regenerated `ExpenseReportTransactionLinesItem` struct. This is
    /// what closes the duplication-risk gap — there is no parallel hand-written
    /// `ExtractedTransactionLine` type, just the generated schema model.
    #[test]
    fn extractor_output_deserializes_into_transaction_line_item() {
        use crate::expense_report_model::ExpenseReportTransactionLinesItem;

        // Inline JSON mirroring what scripts/extract_meal.py writes for one
        // line. Embedded literally so the test runs without depending on the
        // gitignored .scratch/ outputs.
        let json = r#"{
            "common": {
                "date": {"value": "2026-05-02", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "line_amount_usd": {"value": 79.59, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "original_currency": {"value": null, "_meta": {"confidence": "high", "evidence": [{"kind": "system_generated", "origin": "not_applicable_for_domestic"}], "needs_review": false, "flags": []}},
                "original_amount": {"value": null, "_meta": {"confidence": "high", "evidence": [{"kind": "system_generated", "origin": "not_applicable_for_domestic"}], "needs_review": false, "flags": []}},
                "expense_type": {"value": "business_meal", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "remarks": {"value": "Dinner at MJ Sushi", "_meta": {"confidence": "medium", "evidence": [], "needs_review": true, "flags": []}},
                "country_of_activity": {"value": "United States", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "foreign_activity_type": {"value": null, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}}
            },
            "meal_details": {
                "venue_name": {"value": "MJ Sushi", "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "alcohol_amount": {"value": 0.0, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "tip_amount": {"value": 0.0, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}},
                "has_alcohol_on_receipt": {"value": true, "_meta": {"confidence": "high", "evidence": [], "needs_review": false, "flags": []}}
            }
        }"#;

        let parsed: ExpenseReportTransactionLinesItem =
            serde_json::from_str(json).expect("deserialize into transaction line item");

        // Spot-check the fields we care about flowed through correctly.
        assert_eq!(
            parsed.common.date.value.as_ref().map(|d| d.0.as_str()),
            Some("2026-05-02")
        );
        assert!(
            (parsed.common.line_amount_usd.value.unwrap() - 79.59).abs() < 1e-9
        );
        assert_eq!(parsed.common.original_currency.value, None);
        assert_eq!(parsed.common.original_amount.value, None);
        assert_eq!(
            parsed.common.country_of_activity.value.as_deref(),
            Some("United States")
        );
        assert_eq!(parsed.common.country_of_activity.meta.confidence, ConfidenceLevel::High);

        let meal = parsed.meal_details.expect("meal_details present");
        assert_eq!(meal.venue_name.value.as_deref(), Some("MJ Sushi"));
        assert_eq!(meal.has_alcohol_on_receipt.value, Some(true));
        assert_eq!(meal.tip_amount.value, Some(0.0));

        // Provenance flows: the country_of_activity high-confidence flag is
        // available on the Rust side without a sidecar lookup.
        assert!(!parsed.common.country_of_activity.meta.needs_review);
    }
}
