use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::document_extract::extract_document_facts;
use crate::document_facts::{
    DocumentFactsPayload, DocumentKind, ExtractedDocumentFacts, ExtractionStatus,
};
use crate::draft::ConfidenceLevel;
use crate::transcribe::{
    parse_transcribed_document_json_path, OcrPassKind, OcrPreprocessVariant, TranscribedDocument,
};

#[derive(Debug)]
pub enum OcrComparisonError {
    NoPasses,
    MismatchedDocumentIdentity,
    Io(Box<dyn std::error::Error>),
}

impl fmt::Display for OcrComparisonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPasses => write!(f, "at least one OCR pass artifact is required"),
            Self::MismatchedDocumentIdentity => {
                write!(f, "OCR pass artifacts must refer to the same document")
            }
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for OcrComparisonError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrComparisonStatus {
    Consensus,
    PartialConsensus,
    Divergent,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrPassSummary {
    pub pass_id: String,
    pub pass_kind: OcrPassKind,
    pub preprocess_variant: OcrPreprocessVariant,
    pub classification_kind: DocumentKind,
    pub classification_confidence: ConfidenceLevel,
    pub extraction_status: ExtractionStatus,
    pub merchant_name: Option<String>,
    pub transaction_date: Option<String>,
    pub total_paid: Option<String>,
    pub total_paid_currency: Option<String>,
    pub line_item_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrFieldCandidate {
    pub pass_id: String,
    pub value: Option<String>,
    pub extractor_confidence: Option<ConfidenceLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrFieldComparison {
    pub field: String,
    pub status: OcrComparisonStatus,
    pub confidence: ConfidenceLevel,
    pub consensus_value: Option<String>,
    pub disagreement_reason: Option<String>,
    pub candidates: Vec<OcrFieldCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrComparisonResult {
    pub document_id: String,
    pub filename: String,
    pub passes: Vec<OcrPassSummary>,
    pub fields: Vec<OcrFieldComparison>,
    pub disagreement_count: usize,
    pub overall_confidence: ConfidenceLevel,
}

pub fn compare_ocr_passes(
    documents: &[TranscribedDocument],
) -> Result<OcrComparisonResult, OcrComparisonError> {
    let first = documents.first().ok_or(OcrComparisonError::NoPasses)?;
    if documents.iter().any(|document| {
        document.document_id != first.document_id || document.filename != first.filename
    }) {
        return Err(OcrComparisonError::MismatchedDocumentIdentity);
    }

    let extracted = documents
        .iter()
        .map(|document| (document, extract_document_facts(document)))
        .collect::<Vec<_>>();

    let mut fields = vec![compare_field(
        "classification_kind",
        extracted
            .iter()
            .map(|(document, facts)| OcrFieldCandidateInput {
                pass_id: document.metadata.pass_id.as_str(),
                value: Some(facts.classification.kind.as_str().to_owned()),
                confidence: Some(facts.classification.confidence),
            })
            .collect(),
        |value| value.to_owned(),
    )];

    if extracted
        .iter()
        .all(|(_, facts)| facts.classification.kind == DocumentKind::Receipt)
    {
        fields.extend(compare_receipt_fields(&extracted));
    }

    let disagreement_count = fields
        .iter()
        .filter(|field| field.status == OcrComparisonStatus::Divergent)
        .count();

    Ok(OcrComparisonResult {
        document_id: first.document_id.clone(),
        filename: first.filename.clone(),
        passes: extracted
            .iter()
            .map(|(document, facts)| pass_summary(document, facts))
            .collect(),
        overall_confidence: overall_confidence(&fields),
        disagreement_count,
        fields,
    })
}

pub fn compare_ocr_passes_json_paths(
    paths: &[impl AsRef<Path>],
) -> Result<OcrComparisonResult, OcrComparisonError> {
    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        let document =
            parse_transcribed_document_json_path(path).map_err(OcrComparisonError::Io)?;
        documents.push(document);
    }
    compare_ocr_passes(&documents)
}

pub fn render_ocr_comparison_json_pretty(
    comparison: &OcrComparisonResult,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(comparison)
}

pub fn render_ocr_comparison_markdown(comparison: &OcrComparisonResult) -> String {
    let mut lines = vec![
        "# OCR Pass Comparison".to_owned(),
        String::new(),
        format!("- document_id: {}", comparison.document_id),
        format!("- filename: {}", comparison.filename),
        format!(
            "- overall_confidence: {}",
            confidence_label(comparison.overall_confidence)
        ),
        format!("- disagreement_count: {}", comparison.disagreement_count),
        String::new(),
        "## Passes".to_owned(),
    ];

    for pass in &comparison.passes {
        lines.push(format!(
            "- `{}` [{} / {}] kind=`{}` status=`{:?}` merchant=`{}` date=`{}` total=`{}` currency=`{}` line_items={}",
            pass.pass_id,
            pass.pass_kind.as_str(),
            pass.preprocess_variant.as_str(),
            pass.classification_kind.as_str(),
            pass.extraction_status,
            pass.merchant_name.as_deref().unwrap_or("[missing]"),
            pass.transaction_date.as_deref().unwrap_or("[missing]"),
            pass.total_paid.as_deref().unwrap_or("[missing]"),
            pass.total_paid_currency.as_deref().unwrap_or("[missing]"),
            pass.line_item_count,
        ));
    }

    lines.push(String::new());
    lines.push("## Field Comparison".to_owned());
    for field in &comparison.fields {
        lines.push(format!(
            "- `{}`: status=`{:?}` confidence=`{}` consensus=`{}`",
            field.field,
            field.status,
            confidence_label(field.confidence),
            field.consensus_value.as_deref().unwrap_or("[missing]")
        ));
        if let Some(reason) = field.disagreement_reason.as_deref() {
            lines.push(format!("  reason: {reason}"));
        }
        for candidate in &field.candidates {
            lines.push(format!(
                "  - {} -> {} ({})",
                candidate.pass_id,
                candidate.value.as_deref().unwrap_or("[missing]"),
                candidate
                    .extractor_confidence
                    .map(confidence_label)
                    .unwrap_or("unknown"),
            ));
        }
    }

    lines.join("\n")
}

fn compare_receipt_fields(
    extracted: &[(&TranscribedDocument, ExtractedDocumentFacts)],
) -> Vec<OcrFieldComparison> {
    vec![
        compare_field(
            "merchant_name",
            extracted
                .iter()
                .map(|(document, facts)| {
                    let receipt = receipt_facts(facts);
                    OcrFieldCandidateInput {
                        pass_id: document.metadata.pass_id.as_str(),
                        value: receipt
                            .and_then(|receipt| receipt.merchant_name.as_ref())
                            .map(|value| value.value.clone()),
                        confidence: receipt
                            .and_then(|receipt| receipt.merchant_name.as_ref())
                            .map(|value| value.confidence),
                    }
                })
                .collect(),
            normalize_merchant_name,
        ),
        compare_field(
            "transaction_date",
            extracted
                .iter()
                .map(|(document, facts)| {
                    let receipt = receipt_facts(facts);
                    OcrFieldCandidateInput {
                        pass_id: document.metadata.pass_id.as_str(),
                        value: receipt
                            .and_then(|receipt| receipt.transaction_date.as_ref())
                            .map(|value| value.value.clone()),
                        confidence: receipt
                            .and_then(|receipt| receipt.transaction_date.as_ref())
                            .map(|value| value.confidence),
                    }
                })
                .collect(),
            normalize_date_value,
        ),
        compare_field(
            "total_paid",
            extracted
                .iter()
                .map(|(document, facts)| {
                    let receipt = receipt_facts(facts);
                    OcrFieldCandidateInput {
                        pass_id: document.metadata.pass_id.as_str(),
                        value: receipt
                            .and_then(|receipt| receipt.total_paid.as_ref())
                            .map(|value| value.value.amount.clone()),
                        confidence: receipt
                            .and_then(|receipt| receipt.total_paid.as_ref())
                            .map(|value| value.confidence),
                    }
                })
                .collect(),
            normalize_generic_value,
        ),
        compare_field(
            "total_paid_currency",
            extracted
                .iter()
                .map(|(document, facts)| {
                    let receipt = receipt_facts(facts);
                    OcrFieldCandidateInput {
                        pass_id: document.metadata.pass_id.as_str(),
                        value: receipt
                            .and_then(|receipt| receipt.total_paid.as_ref())
                            .and_then(|value| value.value.currency.clone()),
                        confidence: receipt
                            .and_then(|receipt| receipt.total_paid.as_ref())
                            .map(|value| value.confidence),
                    }
                })
                .collect(),
            normalize_generic_value,
        ),
        compare_field(
            "line_item_count",
            extracted
                .iter()
                .map(|(document, facts)| {
                    let receipt = receipt_facts(facts);
                    OcrFieldCandidateInput {
                        pass_id: document.metadata.pass_id.as_str(),
                        value: receipt.map(|receipt| receipt.line_items.len().to_string()),
                        confidence: Some(classification_floor_confidence(facts)),
                    }
                })
                .collect(),
            normalize_generic_value,
        ),
    ]
}

fn pass_summary(document: &TranscribedDocument, facts: &ExtractedDocumentFacts) -> OcrPassSummary {
    let receipt = receipt_facts(facts);
    OcrPassSummary {
        pass_id: document.metadata.pass_id.clone(),
        pass_kind: document.metadata.pass_kind,
        preprocess_variant: document.metadata.preprocess_variant,
        classification_kind: facts.classification.kind,
        classification_confidence: facts.classification.confidence,
        extraction_status: facts.extraction_status,
        merchant_name: receipt
            .and_then(|receipt| receipt.merchant_name.as_ref())
            .map(|value| value.value.clone()),
        transaction_date: receipt
            .and_then(|receipt| receipt.transaction_date.as_ref())
            .map(|value| value.value.clone()),
        total_paid: receipt
            .and_then(|receipt| receipt.total_paid.as_ref())
            .map(|value| value.value.amount.clone()),
        total_paid_currency: receipt
            .and_then(|receipt| receipt.total_paid.as_ref())
            .and_then(|value| value.value.currency.clone()),
        line_item_count: receipt.map_or(0, |receipt| receipt.line_items.len()),
    }
}

fn receipt_facts(facts: &ExtractedDocumentFacts) -> Option<&crate::document_facts::ReceiptFacts> {
    match &facts.facts {
        DocumentFactsPayload::Receipt(receipt) => Some(receipt),
        _ => None,
    }
}

fn classification_floor_confidence(facts: &ExtractedDocumentFacts) -> ConfidenceLevel {
    if facts.extraction_status == ExtractionStatus::NeedsReview {
        ConfidenceLevel::Low
    } else {
        facts.classification.confidence
    }
}

#[derive(Clone)]
struct OcrFieldCandidateInput<'a> {
    pass_id: &'a str,
    value: Option<String>,
    confidence: Option<ConfidenceLevel>,
}

fn compare_field(
    field_name: &str,
    candidates: Vec<OcrFieldCandidateInput<'_>>,
    normalize: fn(&str) -> String,
) -> OcrFieldComparison {
    let mut normalized_groups = BTreeMap::<String, usize>::new();
    let mut display_by_normalized = BTreeMap::<String, String>::new();
    let mut non_missing_count = 0usize;
    let mut has_missing = false;
    let mut all_high = true;

    let rendered_candidates = candidates
        .iter()
        .map(|candidate| {
            if let Some(confidence) = candidate.confidence {
                all_high &= confidence == ConfidenceLevel::High;
            } else {
                all_high = false;
            }
            if let Some(value) = candidate.value.as_deref() {
                let normalized = normalize(value);
                *normalized_groups.entry(normalized.clone()).or_default() += 1;
                display_by_normalized
                    .entry(normalized)
                    .or_insert_with(|| value.to_owned());
                non_missing_count += 1;
            } else {
                has_missing = true;
                all_high = false;
            }
            OcrFieldCandidate {
                pass_id: candidate.pass_id.to_owned(),
                value: candidate.value.clone(),
                extractor_confidence: candidate.confidence,
            }
        })
        .collect::<Vec<_>>();

    let (status, confidence, consensus_value, disagreement_reason) = if non_missing_count == 0 {
        (
            OcrComparisonStatus::Missing,
            ConfidenceLevel::Low,
            None,
            Some("all OCR passes left this field empty".to_owned()),
        )
    } else if normalized_groups.len() == 1 {
        let (normalized_value, _) = normalized_groups.into_iter().next().unwrap();
        let consensus_value = display_by_normalized.get(&normalized_value).cloned();
        if has_missing {
            (
                OcrComparisonStatus::PartialConsensus,
                ConfidenceLevel::Medium,
                consensus_value,
                Some("some OCR passes recovered this field while others left it empty".to_owned()),
            )
        } else {
            (
                OcrComparisonStatus::Consensus,
                if all_high {
                    ConfidenceLevel::High
                } else {
                    ConfidenceLevel::Medium
                },
                consensus_value,
                None,
            )
        }
    } else {
        let rendered_values = display_by_normalized
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join(" vs ");
        (
            OcrComparisonStatus::Divergent,
            ConfidenceLevel::Low,
            None,
            Some(format!("OCR passes disagreed: {rendered_values}")),
        )
    };

    OcrFieldComparison {
        field: field_name.to_owned(),
        status,
        confidence,
        consensus_value,
        disagreement_reason,
        candidates: rendered_candidates,
    }
}

fn overall_confidence(fields: &[OcrFieldComparison]) -> ConfidenceLevel {
    if fields
        .iter()
        .any(|field| field.status == OcrComparisonStatus::Divergent)
    {
        return ConfidenceLevel::Low;
    }
    if fields
        .iter()
        .any(|field| field.status == OcrComparisonStatus::PartialConsensus)
        || fields
            .iter()
            .any(|field| field.status == OcrComparisonStatus::Missing)
    {
        return ConfidenceLevel::Medium;
    }
    if fields
        .iter()
        .all(|field| field.confidence == ConfidenceLevel::High)
    {
        ConfidenceLevel::High
    } else {
        ConfidenceLevel::Medium
    }
}

fn normalize_generic_value(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn normalize_merchant_name(value: &str) -> String {
    let normalized = value
        .to_ascii_lowercase()
        .replace('.', " ")
        .replace(',', " ")
        .replace('&', " and ");
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_date_value(value: &str) -> String {
    let normalized = value.to_ascii_lowercase();
    for token in normalized.split_whitespace() {
        let token = token.trim_matches(|ch: char| ",;()".contains(ch));
        if looks_like_date_token(token) {
            return token.to_owned();
        }
    }
    normalized
}

fn looks_like_date_token(value: &str) -> bool {
    ["/", "-"].into_iter().any(|separator| {
        let parts = value.split(separator).collect::<Vec<_>>();
        parts.len() == 3
            && parts
                .iter()
                .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
    })
}

fn confidence_label(value: ConfidenceLevel) -> &'static str {
    match value {
        ConfidenceLevel::High => "high",
        ConfidenceLevel::Medium => "medium",
        ConfidenceLevel::Low => "low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::{TranscriptionEngine, TranscriptionMetadata};
    use std::path::PathBuf;

    fn receipt_document(
        pass_id: &str,
        pass_kind: OcrPassKind,
        preprocess_variant: OcrPreprocessVariant,
        markdown: &str,
    ) -> TranscribedDocument {
        TranscribedDocument {
            document_id: "receipt".to_owned(),
            filename: "receipt.png".to_owned(),
            source_path: PathBuf::from("receipt.png"),
            engine: TranscriptionEngine::VertexGeminiSdk,
            metadata: TranscriptionMetadata {
                pass_id: pass_id.to_owned(),
                pass_kind,
                preprocess_variant,
                producer: "google_genai_sdk".to_owned(),
                model: Some("gemini-3-flash-preview".to_owned()),
                geometry_source: crate::transcribe::OcrGeometrySource::None,
                geometry_available: false,
            },
            pages: vec![crate::transcribe::TranscribedPage::text_only(1, markdown)],
        }
    }

    #[test]
    fn receipt_passes_with_matching_key_fields_score_high_confidence() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );
        let verification = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n\n## Totals\n- Merchant Name: BOOK TALK\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );

        let comparison = compare_ocr_passes(&[primary, verification]).expect("compare should work");

        assert_eq!(comparison.overall_confidence, ConfidenceLevel::High);
        assert_eq!(comparison.disagreement_count, 0);
        assert!(comparison
            .fields
            .iter()
            .any(|field| field.field == "total_paid"
                && field.status == OcrComparisonStatus::Consensus
                && field.consensus_value.as_deref() == Some("9.00")));
    }

    #[test]
    fn receipt_passes_with_divergent_totals_are_flagged_low_confidence() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );
        let verification = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 90.00\n",
        );

        let comparison = compare_ocr_passes(&[primary, verification]).expect("compare should work");

        assert_eq!(comparison.overall_confidence, ConfidenceLevel::Low);
        assert_eq!(comparison.disagreement_count, 1);
        assert!(comparison
            .fields
            .iter()
            .any(|field| field.field == "total_paid"
                && field.status == OcrComparisonStatus::Divergent
                && field
                    .disagreement_reason
                    .as_deref()
                    .is_some_and(|value| value.contains("9.00"))));
    }

    #[test]
    fn missing_field_in_one_pass_produces_partial_consensus() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );
        let verification = receipt_document(
            "receipt_verification_original",
            OcrPassKind::Verification,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Merchant Name: Book Talk\n- Total: MYR 9.00\n",
        );

        let comparison = compare_ocr_passes(&[primary, verification]).expect("compare should work");

        assert_eq!(comparison.overall_confidence, ConfidenceLevel::Medium);
        assert!(comparison
            .fields
            .iter()
            .any(|field| field.field == "transaction_date"
                && field.status == OcrComparisonStatus::PartialConsensus
                && field.consensus_value.as_deref() == Some("25/12/2018")));
    }

    #[test]
    fn markdown_render_lists_consensus_and_pass_details() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );
        let verification = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n\n- Merchant Name: BOOK TALK\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );

        let comparison = compare_ocr_passes(&[primary, verification]).expect("compare should work");
        let markdown = render_ocr_comparison_markdown(&comparison);

        assert!(markdown.contains("# OCR Pass Comparison"));
        assert!(markdown.contains("overall_confidence: high"));
        assert!(markdown.contains("receipt_primary_original"));
        assert!(markdown.contains("classification_kind"));
    }

    #[test]
    fn compare_rejects_mismatched_document_ids() {
        let mut first = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Total: MYR 9.00\n",
        );
        let mut second = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n\n- Total: MYR 9.00\n",
        );
        second.document_id = "different".to_owned();

        let error = compare_ocr_passes(&[first.clone(), second]).expect_err("should reject");
        assert!(matches!(
            error,
            OcrComparisonError::MismatchedDocumentIdentity
        ));
        first.document_id = "receipt".to_owned();
    }
}
