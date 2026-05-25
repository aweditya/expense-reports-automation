//! Evidence-and-metadata types attached to every wrapped field in a typed
//! `ExpenseReport`. The shape is mirrored by the per-receipt JSON the
//! Python extractors emit.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Document,
    DocumentSpan,
    SystemGenerated,
    UserInput,
}

// Eq not derived: `bboxes` carries `f64`. Nothing uses these as map keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceReference {
    pub kind: EvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Bounding boxes of the quote in the source document, populated by
    /// the OCR grounding pass. Each rect is `[x0, y0, x1, y1]` normalized
    /// to 0..1 with top-left origin. Multiple rects when the quote
    /// appears more than once on the page. Absent for non-`DocumentSpan`
    /// evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bboxes: Option<Vec<[f64; 4]>>,
    /// Document AI token indices Gemini returned to ground this evidence.
    /// Resolved to `bboxes` (above) via dict lookup; preserved on
    /// round-trip so the verifier can trace each bbox back to its tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_ids: Option<Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldMetadata {
    pub confidence: ConfidenceLevel,
    pub evidence: Vec<EvidenceReference>,
    pub needs_review: bool,
    pub flags: Vec<String>,
    /// One short sentence the extractor wrote justifying the confidence
    /// level. Optional so older cached extractions still deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_reason: Option<String>,
}
