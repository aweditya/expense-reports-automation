use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::document_extract::extract_document_facts;
use crate::document_facts::{
    DocumentExtractionIssue, DocumentFactsPayload, DocumentKind, ExtractedDocumentFacts,
    ExtractionStatus, IssueSeverity, MoneyAmount, Observed, ReceiptFacts,
};
use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrConsistencyStatus {
    Pass,
    Warning,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrConsistencyCheck {
    pub check: String,
    pub status: OcrConsistencyStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrComparisonResult {
    pub document_id: String,
    pub filename: String,
    pub passes: Vec<OcrPassSummary>,
    pub fields: Vec<OcrFieldComparison>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consistency_checks: Vec<OcrConsistencyCheck>,
    pub disagreement_count: usize,
    pub overall_confidence: ConfidenceLevel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentOcrComparisonSummary {
    pub document_id: String,
    pub compared_pass_count: usize,
    pub overall_confidence: ConfidenceLevel,
    pub disagreement_count: usize,
    pub divergent_fields: Vec<String>,
    pub consistency_warning_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consistency_notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_summaries: Vec<OcrFieldComparisonSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrFieldComparisonSummary {
    pub field: String,
    pub status: OcrComparisonStatus,
    pub confidence: ConfidenceLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus_value: Option<String>,
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
        exact_value_equivalent,
    )];

    if extracted
        .iter()
        .all(|(_, facts)| facts.classification.kind == DocumentKind::Receipt)
    {
        fields.extend(compare_receipt_fields(&extracted));
    }
    let consistency_checks = if extracted
        .iter()
        .all(|(_, facts)| facts.classification.kind == DocumentKind::Receipt)
    {
        compare_receipt_consistency(&extracted)
    } else {
        Vec::new()
    };

    let disagreement_count = fields
        .iter()
        .filter(|field| field.status == OcrComparisonStatus::Divergent)
        .count();

    let overall_confidence = overall_confidence(&fields, &consistency_checks);

    Ok(OcrComparisonResult {
        document_id: first.document_id.clone(),
        filename: first.filename.clone(),
        passes: extracted
            .iter()
            .map(|(document, facts)| pass_summary(document, facts))
            .collect(),
        consistency_checks,
        overall_confidence,
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

pub fn summarize_ocr_comparison(comparison: &OcrComparisonResult) -> DocumentOcrComparisonSummary {
    DocumentOcrComparisonSummary {
        document_id: comparison.document_id.clone(),
        compared_pass_count: comparison.passes.len(),
        overall_confidence: comparison.overall_confidence,
        disagreement_count: comparison.disagreement_count,
        divergent_fields: comparison
            .fields
            .iter()
            .filter(|field| field.status == OcrComparisonStatus::Divergent)
            .map(|field| field.field.clone())
            .collect(),
        consistency_warning_count: comparison
            .consistency_checks
            .iter()
            .filter(|check| check.status == OcrConsistencyStatus::Warning)
            .count(),
        consistency_notes: comparison
            .consistency_checks
            .iter()
            .filter(|check| check.status == OcrConsistencyStatus::Warning)
            .map(|check| check.message.clone())
            .collect(),
        field_summaries: comparison
            .fields
            .iter()
            .map(|field| OcrFieldComparisonSummary {
                field: field.field.clone(),
                status: field.status,
                confidence: field.confidence,
                consensus_value: field.consensus_value.clone(),
            })
            .collect(),
    }
}

pub fn resolve_receipt_ocr_consensus(
    primary: &ExtractedDocumentFacts,
    secondary: &ExtractedDocumentFacts,
    comparison: &OcrComparisonResult,
) -> Result<ExtractedDocumentFacts, OcrComparisonError> {
    if primary.document_id != secondary.document_id
        || primary.filename != secondary.filename
        || comparison.document_id != primary.document_id
        || comparison.filename != primary.filename
    {
        return Err(OcrComparisonError::MismatchedDocumentIdentity);
    }

    let (
        DocumentFactsPayload::Receipt(primary_receipt),
        DocumentFactsPayload::Receipt(secondary_receipt),
    ) = (&primary.facts, &secondary.facts)
    else {
        return Ok(primary.clone());
    };

    let mut resolved = primary.clone();
    let mut receipt = primary_receipt.clone();
    let mut issues = primary.issues.clone();

    receipt.merchant_name = reconcile_observed_field(
        "merchant_name",
        &primary_receipt.merchant_name,
        &secondary_receipt.merchant_name,
        field_comparison(comparison, "merchant_name"),
        &mut issues,
    );
    receipt.transaction_date = reconcile_observed_field(
        "transaction_date",
        &primary_receipt.transaction_date,
        &secondary_receipt.transaction_date,
        field_comparison(comparison, "transaction_date"),
        &mut issues,
    );
    receipt.total_paid = reconcile_total_paid_field(
        &primary_receipt.total_paid,
        &secondary_receipt.total_paid,
        field_comparison(comparison, "total_paid"),
        field_comparison(comparison, "total_paid_currency"),
        &mut issues,
    );
    receipt.line_items = reconcile_line_items(
        &primary_receipt.line_items,
        &secondary_receipt.line_items,
        field_comparison(comparison, "line_item_count"),
        &mut issues,
    );
    receipt.merchant_location = fill_missing_secondary_observed(
        "merchant_location",
        &primary_receipt.merchant_location,
        &secondary_receipt.merchant_location,
    );
    receipt.subtotal = fill_missing_secondary_observed(
        "subtotal",
        &primary_receipt.subtotal,
        &secondary_receipt.subtotal,
    );
    receipt.tax_amount = fill_missing_secondary_observed(
        "tax_amount",
        &primary_receipt.tax_amount,
        &secondary_receipt.tax_amount,
    );
    receipt.tip_amount = fill_missing_secondary_observed(
        "tip_amount",
        &primary_receipt.tip_amount,
        &secondary_receipt.tip_amount,
    );

    resolved.facts = DocumentFactsPayload::Receipt(receipt.clone());
    resolved.issues = issues;
    resolved.extraction_status = resolve_extraction_status(
        primary.extraction_status,
        secondary.extraction_status,
        comparison.overall_confidence,
        &receipt,
    );

    Ok(resolved)
}

pub fn render_ocr_comparison_json_pretty(
    comparison: &OcrComparisonResult,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(comparison)
}

pub fn render_ocr_comparison_html(comparison: &OcrComparisonResult) -> String {
    let mut html = String::new();
    html.push_str(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>OCR Pass Comparison</title><style>\
         :root{color-scheme:light;font-family:ui-sans-serif,system-ui,sans-serif;}\
         body{margin:0;background:#f7f3eb;color:#231f1a;}\
         main{max-width:1260px;margin:0 auto;padding:32px 24px 48px;}\
         h1,h2,h3{margin:0;}\
         p{margin:0;}\
         .summary,.section,.field{background:#fffdf9;border:1px solid #ddcfbb;border-radius:18px;box-shadow:0 8px 24px rgba(86,61,35,.08);}\
         .summary,.section{padding:20px 22px;margin-bottom:18px;}\
         .summary-header{display:flex;justify-content:space-between;gap:16px;align-items:end;margin-bottom:16px;flex-wrap:wrap;}\
         .eyebrow{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#8a5a2b;font-weight:700;margin-bottom:8px;}\
         .summary-subtitle{color:#7b7064;line-height:1.5;max-width:760px;}\
         .field{padding:18px 20px;margin-bottom:14px;}\
         .grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;}\
         .metric{padding:12px 14px;border-radius:14px;background:#f3ece0;border:1px solid #e1d3bf;}\
         .metric-label{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#8a5a2b;font-weight:700;}\
         .metric-value{margin-top:6px;font-size:20px;font-weight:700;}\
         .badge{display:inline-flex;align-items:center;gap:6px;padding:4px 10px;border-radius:999px;font-size:12px;font-weight:700;text-transform:uppercase;letter-spacing:.05em;border:1px solid transparent;}\
         .badge.high,.badge.consensus,.badge.pass{background:#d8f0df;color:#195c31;border-color:rgba(25,92,49,.12);}\
         .badge.medium,.badge.partialconsensus,.badge.unavailable{background:#fff0c9;color:#7d5700;border-color:rgba(125,87,0,.12);}\
         .badge.low,.badge.divergent,.badge.missing,.badge.warning{background:#ffd7d2;color:#8f2414;border-color:rgba(143,36,20,.12);}\
         code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px;}\
         .pass-grid,.candidate-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:12px;}\
         .pass-card,.candidate-card{border:1px solid #eadfce;border-radius:16px;background:#fff;padding:14px 16px;}\
         .pass-card header,.field header{display:flex;justify-content:space-between;gap:12px;align-items:start;flex-wrap:wrap;}\
         .pass-card-title{display:grid;gap:4px;}\
         .pass-meta{color:#7b7064;font-size:13px;line-height:1.5;}\
         .pass-details{margin-top:14px;display:grid;gap:8px;}\
         .detail-row{display:grid;gap:4px;}\
         .detail-label{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#8a5a2b;font-weight:700;}\
         .detail-value{line-height:1.45;word-break:break-word;}\
         .muted{color:#7b7064;}\
         .section-heading{display:flex;justify-content:space-between;gap:16px;align-items:end;margin-bottom:14px;flex-wrap:wrap;}\
         .section-copy{color:#7b7064;line-height:1.5;max-width:760px;}\
         .reason{margin-top:10px;padding:10px 12px;border-radius:12px;background:#f8ede7;color:#7b3426;}\
         .candidate-grid{margin-top:12px;}\
         .candidate-card.missing{background:#f9f4ee;}\
         .candidate-card header{display:flex;justify-content:space-between;gap:12px;align-items:start;flex-wrap:wrap;margin-bottom:10px;}\
         .candidate-value{font-size:18px;font-weight:700;line-height:1.35;}\
         .candidate-meta{color:#7b7064;font-size:13px;line-height:1.5;}\
         .field-topline{display:flex;gap:8px;flex-wrap:wrap;margin-top:10px;}\
         .field-summary{margin-top:10px;color:#7b7064;line-height:1.5;}\
         .consistency-list{display:grid;gap:12px;}\
         .field-list{display:grid;gap:14px;}\
         </style></head><body><main>",
    );
    html.push_str("<div class=\"summary\"><div class=\"summary-header\"><div><p class=\"eyebrow\">Developer OCR diff</p><h1>OCR Pass Comparison</h1><p class=\"summary-subtitle\">Side-by-side inspection of OCR passes for one document. Use this view to see which fields agreed, which ones drifted, and whether arithmetic consistency checks succeeded before the FA sees the result.</p></div></div><div class=\"grid\">");
    html.push_str(&metric_card("document_id", &comparison.document_id));
    html.push_str(&metric_card("filename", &comparison.filename));
    html.push_str(&metric_card(
        "overall_confidence",
        confidence_label(comparison.overall_confidence),
    ));
    html.push_str(&metric_card(
        "disagreement_count",
        &comparison.disagreement_count.to_string(),
    ));
    html.push_str(&metric_card(
        "consistency_warnings",
        &comparison
            .consistency_checks
            .iter()
            .filter(|check| check.status == OcrConsistencyStatus::Warning)
            .count()
            .to_string(),
    ));
    html.push_str("</div></div>");

    html.push_str("<section class=\"section\"><div class=\"section-heading\"><div><p class=\"eyebrow\">Passes</p><h2>Per-pass extraction</h2></div><p class=\"section-copy\">Each card shows the structured extraction produced by one OCR pass after its own preprocess lane. This is the fastest way to see whether disagreement comes from the OCR itself or from downstream reconciliation.</p></div><div class=\"pass-grid\">");
    for pass in &comparison.passes {
        html.push_str("<article class=\"pass-card\"><header><div class=\"pass-card-title\">");
        html.push_str(&format!(
            "<h3><code>{}</code></h3>",
            escape_html(&pass.pass_id)
        ));
        html.push_str(&format!(
            "<p class=\"pass-meta\">{} pass · {} preprocess · {} · {:?}</p>",
            escape_html(pass.pass_kind.as_str()),
            escape_html(pass.preprocess_variant.as_str()),
            escape_html(pass.classification_kind.as_str()),
            pass.extraction_status
        ));
        html.push_str("</div>");
        html.push_str(&format!(
            "<span class=\"badge {}\">{}</span>",
            confidence_label(pass.classification_confidence),
            escape_html(confidence_label(pass.classification_confidence))
        ));
        html.push_str("</header><div class=\"pass-details\">");
        html.push_str(&detail_row(
            "Merchant",
            pass.merchant_name.as_deref().unwrap_or("[missing]"),
        ));
        html.push_str(&detail_row(
            "Date",
            pass.transaction_date.as_deref().unwrap_or("[missing]"),
        ));
        html.push_str(&detail_row(
            "Total",
            pass.total_paid.as_deref().unwrap_or("[missing]"),
        ));
        html.push_str(&detail_row(
            "Currency",
            pass.total_paid_currency.as_deref().unwrap_or("[missing]"),
        ));
        html.push_str(&detail_row("Line Items", &pass.line_item_count.to_string()));
        html.push_str("</div></article>");
    }
    html.push_str("</div></section>");

    if !comparison.consistency_checks.is_empty() {
        html.push_str("<section class=\"section\"><div class=\"section-heading\"><div><p class=\"eyebrow\">Internal checks</p><h2>Consistency Checks</h2></div><p class=\"section-copy\">These checks are independent of pass agreement. A warning here means the receipt content itself does not add up cleanly, so even matching OCR passes should be reviewed carefully.</p></div><div class=\"consistency-list\">");
        for check in &comparison.consistency_checks {
            html.push_str("<article class=\"field\">");
            html.push_str("<header>");
            html.push_str(&format!(
                "<h3><code>{}</code></h3>",
                escape_html(&check.check)
            ));
            html.push_str(&format!(
                "<span class=\"badge {}\">{}</span>",
                consistency_status_class(check.status),
                escape_html(consistency_status_label(check.status))
            ));
            html.push_str("</header>");
            html.push_str(&format!(
                "<p class=\"field-summary\"><strong>Summary:</strong> <span class=\"muted\">{}</span></p>",
                escape_html(&check.message)
            ));
            html.push_str("</article>");
        }
        html.push_str("</div></section>");
    }

    html.push_str("<section class=\"section\"><div class=\"section-heading\"><div><p class=\"eyebrow\">Field matrix</p><h2>Field Comparison</h2></div><p class=\"section-copy\">Each field card shows the consensus verdict plus the raw value emitted by every pass. This is the main debugging surface for understanding whether a value is stable enough to trust.</p></div><div class=\"field-list\">");
    for field in &comparison.fields {
        html.push_str("<article class=\"field\">");
        html.push_str("<header>");
        html.push_str(&format!(
            "<h3><code>{}</code></h3>",
            escape_html(&field.field)
        ));
        html.push_str("<div class=\"field-topline\">");
        html.push_str(&format!(
            "<span class=\"badge {}\">{}</span>",
            status_class(field.status),
            escape_html(status_label(field.status))
        ));
        html.push_str(&format!(
            "<span class=\"badge {}\">{}</span>",
            confidence_label(field.confidence),
            escape_html(confidence_label(field.confidence))
        ));
        html.push_str("</div></header>");
        html.push_str(&format!(
            "<p class=\"field-summary\"><strong>Consensus:</strong> <span class=\"muted\">{}</span></p>",
            escape_html(field.consensus_value.as_deref().unwrap_or("[missing]"))
        ));
        if let Some(reason) = field.disagreement_reason.as_deref() {
            html.push_str(&format!(
                "<div class=\"reason\"><strong>Reason:</strong> {}</div>",
                escape_html(reason)
            ));
        }
        html.push_str("<div class=\"candidate-grid\">");
        for candidate in &field.candidates {
            let card_class = if candidate.value.is_some() {
                ""
            } else {
                " missing"
            };
            html.push_str("<article class=\"candidate-card");
            html.push_str(card_class);
            html.push_str("\"><header><div>");
            html.push_str(&format!(
                "<h3><code>{}</code></h3>",
                escape_html(&candidate.pass_id)
            ));
            html.push_str("</div>");
            html.push_str(&format!(
                "<span class=\"badge {}\">{}</span>",
                candidate
                    .extractor_confidence
                    .map(confidence_label)
                    .unwrap_or("medium"),
                escape_html(
                    candidate
                        .extractor_confidence
                        .map(confidence_label)
                        .unwrap_or("unknown")
                )
            ));
            html.push_str("</header>");
            html.push_str("<p class=\"candidate-value\">");
            html.push_str(&escape_html(
                candidate.value.as_deref().unwrap_or("[missing]"),
            ));
            html.push_str("</p>");
            html.push_str("<p class=\"candidate-meta\">");
            html.push_str("Pass-specific extracted value");
            html.push_str("</p></article>");
        }
        html.push_str("</div></article>");
    }
    html.push_str("</div></section></main></body></html>");
    html
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
        format!(
            "- consistency_warning_count: {}",
            comparison
                .consistency_checks
                .iter()
                .filter(|check| check.status == OcrConsistencyStatus::Warning)
                .count()
        ),
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
    if !comparison.consistency_checks.is_empty() {
        lines.push("## Consistency Checks".to_owned());
        for check in &comparison.consistency_checks {
            lines.push(format!(
                "- `{}`: status=`{}` summary=`{}`",
                check.check,
                consistency_status_label(check.status),
                check.message
            ));
        }
        lines.push(String::new());
    }
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
            merchant_name_equivalent,
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
            exact_value_equivalent,
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
            exact_value_equivalent,
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
            exact_value_equivalent,
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
            exact_value_equivalent,
        ),
    ]
}

fn compare_receipt_consistency(
    extracted: &[(&TranscribedDocument, ExtractedDocumentFacts)],
) -> Vec<OcrConsistencyCheck> {
    vec![
        aggregate_consistency_check(
            "subtotal_tax_tip_matches_total",
            extracted,
            evaluate_subtotal_tax_tip_consistency,
            "Subtotal plus tax/tip does not match the total in",
            "All evaluable OCR passes kept subtotal, tax, tip, and total internally consistent.",
            "Not enough subtotal/tax/total data to evaluate subtotal-to-total consistency.",
        ),
        aggregate_consistency_check(
            "line_items_sum_matches_subtotal_or_total",
            extracted,
            evaluate_line_item_sum_consistency,
            "Line-item totals do not match the receipt subtotal/total in",
            "All evaluable OCR passes kept line-item amounts consistent with the receipt total.",
            "Not enough line-item amount data to evaluate amount-rollup consistency.",
        ),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConsistencyEvaluation {
    pass_id: String,
    message: String,
}

fn aggregate_consistency_check(
    check: &str,
    extracted: &[(&TranscribedDocument, ExtractedDocumentFacts)],
    evaluator: fn(&ExtractedDocumentFacts) -> Option<bool>,
    warning_prefix: &str,
    pass_message: &str,
    unavailable_message: &str,
) -> OcrConsistencyCheck {
    let mut failures = Vec::new();
    let mut evaluated_passes = 0usize;

    for (document, facts) in extracted {
        match evaluator(facts) {
            Some(true) => {
                evaluated_passes += 1;
            }
            Some(false) => {
                evaluated_passes += 1;
                failures.push(ConsistencyEvaluation {
                    pass_id: document.metadata.pass_id.clone(),
                    message: document.metadata.preprocess_variant.as_str().to_owned(),
                });
            }
            None => {}
        }
    }

    let (status, message) = if evaluated_passes == 0 {
        (
            OcrConsistencyStatus::Unavailable,
            unavailable_message.to_owned(),
        )
    } else if failures.is_empty() {
        (OcrConsistencyStatus::Pass, pass_message.to_owned())
    } else {
        let failing_passes = failures
            .into_iter()
            .map(|failure| format!("{} ({})", failure.pass_id, failure.message))
            .collect::<Vec<_>>()
            .join(", ");
        (
            OcrConsistencyStatus::Warning,
            format!("{warning_prefix} {failing_passes}."),
        )
    };

    OcrConsistencyCheck {
        check: check.to_owned(),
        status,
        message,
    }
}

fn evaluate_subtotal_tax_tip_consistency(facts: &ExtractedDocumentFacts) -> Option<bool> {
    let receipt = receipt_facts(facts)?;
    let total = parse_money_amount(receipt.total_paid.as_ref()?.value.amount.as_str())?;
    let subtotal_observed = receipt.subtotal.as_ref()?;
    let subtotal = parse_money_amount(subtotal_observed.value.amount.as_str())?;
    let subtotal_currency = subtotal_observed.value.currency.as_deref();
    let total_currency = receipt.total_paid.as_ref()?.value.currency.as_deref();
    if !currencies_match(subtotal_currency, total_currency) {
        return Some(false);
    }
    let tax = match receipt.tax_amount.as_ref() {
        Some(value) => {
            if !currencies_match(value.value.currency.as_deref(), total_currency) {
                return Some(false);
            }
            parse_money_amount(value.value.amount.as_str())?
        }
        None => 0.0,
    };
    let tip = match receipt.tip_amount.as_ref() {
        Some(value) => {
            if !currencies_match(value.value.currency.as_deref(), total_currency) {
                return Some(false);
            }
            parse_money_amount(value.value.amount.as_str())?
        }
        None => 0.0,
    };
    Some(nearly_equal(subtotal + tax + tip, total))
}

fn evaluate_line_item_sum_consistency(facts: &ExtractedDocumentFacts) -> Option<bool> {
    let receipt = receipt_facts(facts)?;
    if receipt.line_items.is_empty() {
        return None;
    }
    let line_sum = receipt
        .line_items
        .iter()
        .map(|item| parse_money_amount(item.amount.value.amount.as_str()))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .sum::<f64>();
    let line_currency = receipt
        .line_items
        .first()
        .and_then(|item| item.amount.value.currency.as_deref());
    if receipt
        .line_items
        .iter()
        .any(|item| !currencies_match(item.amount.value.currency.as_deref(), line_currency))
    {
        return Some(false);
    }

    if let Some(subtotal) = receipt.subtotal.as_ref() {
        if currencies_match(subtotal.value.currency.as_deref(), line_currency) {
            return Some(nearly_equal(
                line_sum,
                parse_money_amount(subtotal.value.amount.as_str())?,
            ));
        }
        return Some(false);
    }
    let total = receipt.total_paid.as_ref()?;
    if !currencies_match(total.value.currency.as_deref(), line_currency) {
        return Some(false);
    }
    Some(nearly_equal(
        line_sum,
        parse_money_amount(total.value.amount.as_str())?,
    ))
}

fn parse_money_amount(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

fn nearly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 0.02
}

fn currencies_match(left: Option<&str>, right: Option<&str>) -> bool {
    left == right || left.is_none() || right.is_none()
}

fn field_comparison<'a>(
    comparison: &'a OcrComparisonResult,
    field_name: &str,
) -> Option<&'a OcrFieldComparison> {
    comparison
        .fields
        .iter()
        .find(|field| field.field == field_name)
}

fn reconcile_observed_field<T: Clone>(
    field_name: &str,
    primary: &Option<Observed<T>>,
    secondary: &Option<Observed<T>>,
    comparison: Option<&OcrFieldComparison>,
    issues: &mut Vec<DocumentExtractionIssue>,
) -> Option<Observed<T>> {
    let (mut chosen, used_secondary) = choose_observed_variant(primary, secondary, comparison);
    let Some(observed) = chosen.as_mut() else {
        return None;
    };

    if used_secondary {
        observed
            .flags
            .push(format!("ocr_secondary_fill_{field_name}"));
        observed.evidence.push(system_generated_evidence(&format!(
            "ocr_compare.secondary_fill.{field_name}"
        )));
        issues.push(ocr_resolution_issue(
            &format!("ocr_secondary_fill_{field_name}"),
            &format!("Filled {field_name} from the secondary OCR pass"),
        ));
    }

    if let Some(comparison) = comparison {
        apply_comparison_signal(field_name, observed, comparison, issues);
    }

    Some(observed.clone())
}

fn reconcile_total_paid_field(
    primary: &Option<Observed<MoneyAmount>>,
    secondary: &Option<Observed<MoneyAmount>>,
    amount_comparison: Option<&OcrFieldComparison>,
    currency_comparison: Option<&OcrFieldComparison>,
    issues: &mut Vec<DocumentExtractionIssue>,
) -> Option<Observed<MoneyAmount>> {
    let (mut chosen, used_secondary) =
        choose_observed_variant(primary, secondary, amount_comparison);
    let Some(observed) = chosen.as_mut() else {
        return None;
    };

    if used_secondary {
        observed
            .flags
            .push("ocr_secondary_fill_total_paid".to_owned());
        observed.evidence.push(system_generated_evidence(
            "ocr_compare.secondary_fill.total_paid",
        ));
        issues.push(ocr_resolution_issue(
            "ocr_secondary_fill_total_paid",
            "Filled total_paid from the secondary OCR pass",
        ));
    }

    if observed.value.currency.is_none() {
        if let (Some(primary_total), Some(secondary_total)) = (primary, secondary) {
            if normalize_generic_value(&primary_total.value.amount)
                == normalize_generic_value(&secondary_total.value.amount)
            {
                if let Some(currency) = secondary_total.value.currency.clone() {
                    observed.value.currency = Some(currency);
                    observed
                        .flags
                        .push("ocr_secondary_fill_total_paid_currency".to_owned());
                    observed.evidence.push(system_generated_evidence(
                        "ocr_compare.secondary_fill.total_paid_currency",
                    ));
                    issues.push(ocr_resolution_issue(
                        "ocr_secondary_fill_total_paid_currency",
                        "Filled total_paid_currency from the secondary OCR pass",
                    ));
                }
            }
        }
    }

    if let Some(comparison) = amount_comparison {
        apply_comparison_signal("total_paid", observed, comparison, issues);
    }
    if let Some(comparison) = currency_comparison {
        match comparison.status {
            OcrComparisonStatus::Consensus => {
                if observed.confidence != ConfidenceLevel::Low {
                    observed.confidence =
                        max_confidence(observed.confidence, comparison.confidence);
                }
            }
            OcrComparisonStatus::PartialConsensus => {
                if observed.confidence != ConfidenceLevel::Low {
                    observed.confidence = ConfidenceLevel::Medium;
                    observed
                        .flags
                        .push("ocr_partial_consensus_total_paid_currency".to_owned());
                    observed.evidence.push(system_generated_evidence(
                        "ocr_compare.partial_consensus.total_paid_currency",
                    ));
                }
            }
            OcrComparisonStatus::Divergent => {
                observed.confidence = ConfidenceLevel::Low;
                observed
                    .flags
                    .push("ocr_pass_disagreement_total_paid_currency".to_owned());
                observed.evidence.push(system_generated_evidence(
                    "ocr_compare.disagreement.total_paid_currency",
                ));
                issues.push(ocr_resolution_issue(
                    "ocr_pass_disagreement_total_paid_currency",
                    "OCR passes disagreed on total_paid_currency; retained the better-supported amount",
                ));
            }
            OcrComparisonStatus::Missing => {}
        }
    }

    Some(observed.clone())
}

fn reconcile_line_items(
    primary: &[crate::document_facts::ReceiptLineItemFacts],
    secondary: &[crate::document_facts::ReceiptLineItemFacts],
    comparison: Option<&OcrFieldComparison>,
    issues: &mut Vec<DocumentExtractionIssue>,
) -> Vec<crate::document_facts::ReceiptLineItemFacts> {
    let mut chosen = if primary.is_empty() && !secondary.is_empty() {
        issues.push(ocr_resolution_issue(
            "ocr_secondary_fill_line_items",
            "Filled line_items from the secondary OCR pass",
        ));
        secondary.to_vec()
    } else {
        primary.to_vec()
    };

    if let Some(comparison) = comparison {
        if comparison.status == OcrComparisonStatus::Divergent {
            chosen.iter_mut().for_each(|item| {
                item.description.confidence = ConfidenceLevel::Low;
                item.description
                    .flags
                    .push("ocr_pass_disagreement_line_item_count".to_owned());
                item.description.evidence.push(system_generated_evidence(
                    "ocr_compare.disagreement.line_item_count",
                ));
                item.amount.confidence = ConfidenceLevel::Low;
                item.amount
                    .flags
                    .push("ocr_pass_disagreement_line_item_count".to_owned());
                item.amount.evidence.push(system_generated_evidence(
                    "ocr_compare.disagreement.line_item_count",
                ));
            });
            issues.push(ocr_resolution_issue(
                "ocr_pass_disagreement_line_item_count",
                "OCR passes disagreed on line_item_count; retained the current line item set",
            ));
        }
    }

    chosen
}

fn choose_observed_variant<T: Clone>(
    primary: &Option<Observed<T>>,
    secondary: &Option<Observed<T>>,
    comparison: Option<&OcrFieldComparison>,
) -> (Option<Observed<T>>, bool) {
    let primary_rank = primary
        .as_ref()
        .map(|value| confidence_rank(value.confidence))
        .unwrap_or(-1);
    let secondary_rank = secondary
        .as_ref()
        .map(|value| confidence_rank(value.confidence))
        .unwrap_or(-1);

    let prefer_secondary = match comparison.map(|value| value.status) {
        Some(OcrComparisonStatus::Consensus | OcrComparisonStatus::PartialConsensus) => {
            primary.is_none() || (secondary.is_some() && secondary_rank > primary_rank)
        }
        Some(OcrComparisonStatus::Divergent) => {
            secondary.is_some() && (primary.is_none() || secondary_rank > primary_rank)
        }
        Some(OcrComparisonStatus::Missing) => false,
        None => primary.is_none() && secondary.is_some(),
    };

    if prefer_secondary {
        (secondary.clone().or_else(|| primary.clone()), true)
    } else {
        (primary.clone().or_else(|| secondary.clone()), false)
    }
}

fn fill_missing_secondary_observed<T: Clone>(
    field_name: &str,
    primary: &Option<Observed<T>>,
    secondary: &Option<Observed<T>>,
) -> Option<Observed<T>> {
    if primary.is_some() {
        return primary.clone();
    }
    let mut chosen = secondary.clone()?;
    chosen
        .flags
        .push(format!("ocr_secondary_fill_{field_name}"));
    chosen.evidence.push(system_generated_evidence(&format!(
        "ocr_compare.secondary_fill.{field_name}"
    )));
    Some(chosen)
}

fn apply_comparison_signal<T>(
    field_name: &str,
    observed: &mut Observed<T>,
    comparison: &OcrFieldComparison,
    issues: &mut Vec<DocumentExtractionIssue>,
) {
    match comparison.status {
        OcrComparisonStatus::Consensus => {
            observed.confidence = max_confidence(observed.confidence, comparison.confidence);
            observed.evidence.push(system_generated_evidence(&format!(
                "ocr_compare.consensus.{field_name}"
            )));
        }
        OcrComparisonStatus::PartialConsensus => {
            observed.confidence = ConfidenceLevel::Medium;
            observed
                .flags
                .push(format!("ocr_partial_consensus_{field_name}"));
            observed.evidence.push(system_generated_evidence(&format!(
                "ocr_compare.partial_consensus.{field_name}"
            )));
        }
        OcrComparisonStatus::Divergent => {
            observed.confidence = ConfidenceLevel::Low;
            observed
                .flags
                .push(format!("ocr_pass_disagreement_{field_name}"));
            observed.evidence.push(system_generated_evidence(&format!(
                "ocr_compare.disagreement.{field_name}"
            )));
            issues.push(ocr_resolution_issue(
                &format!("ocr_pass_disagreement_{field_name}"),
                &format!(
                    "OCR passes disagreed on {field_name}; retained the better-supported value"
                ),
            ));
        }
        OcrComparisonStatus::Missing => {}
    }
}

fn resolve_extraction_status(
    primary: ExtractionStatus,
    secondary: ExtractionStatus,
    overall_confidence: ConfidenceLevel,
    receipt: &ReceiptFacts,
) -> ExtractionStatus {
    if matches!(primary, ExtractionStatus::Partial)
        || matches!(secondary, ExtractionStatus::Partial)
    {
        return ExtractionStatus::Partial;
    }

    if overall_confidence == ConfidenceLevel::Low || receipt_needs_review(receipt) {
        ExtractionStatus::NeedsReview
    } else {
        primary
    }
}

fn receipt_needs_review(receipt: &ReceiptFacts) -> bool {
    receipt
        .merchant_name
        .as_ref()
        .is_some_and(Observed::needs_review)
        || receipt
            .transaction_date
            .as_ref()
            .is_some_and(Observed::needs_review)
        || receipt
            .total_paid
            .as_ref()
            .is_some_and(Observed::needs_review)
        || receipt
            .subtotal
            .as_ref()
            .is_some_and(Observed::needs_review)
        || receipt
            .tax_amount
            .as_ref()
            .is_some_and(Observed::needs_review)
        || receipt
            .tip_amount
            .as_ref()
            .is_some_and(Observed::needs_review)
        || receipt
            .line_items
            .iter()
            .any(|item| item.description.needs_review() || item.amount.needs_review())
}

fn max_confidence(left: ConfidenceLevel, right: ConfidenceLevel) -> ConfidenceLevel {
    if confidence_rank(left) >= confidence_rank(right) {
        left
    } else {
        right
    }
}

fn confidence_rank(value: ConfidenceLevel) -> i8 {
    match value {
        ConfidenceLevel::Low => 0,
        ConfidenceLevel::Medium => 1,
        ConfidenceLevel::High => 2,
    }
}

fn system_generated_evidence(origin: &str) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::SystemGenerated,
        document_id: None,
        filename: None,
        page: None,
        quote: None,
        origin: Some(origin.to_owned()),
    }
}

fn ocr_resolution_issue(code: &str, message: &str) -> DocumentExtractionIssue {
    DocumentExtractionIssue {
        severity: IssueSeverity::Warning,
        code: code.to_owned(),
        message: message.to_owned(),
        evidence: vec![system_generated_evidence(
            "ocr_compare.resolve_receipt_ocr_consensus",
        )],
    }
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
    equivalent: fn(&str, &str) -> bool,
) -> OcrFieldComparison {
    let mut grouped_values = Vec::<(String, String, usize)>::new();
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
                if let Some((_, _, count)) = grouped_values
                    .iter_mut()
                    .find(|(group_normalized, _, _)| equivalent(&normalized, group_normalized))
                {
                    *count += 1;
                } else {
                    grouped_values.push((normalized, value.to_owned(), 1));
                }
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
    } else if grouped_values.len() == 1 {
        let consensus_value = grouped_values
            .first()
            .map(|(_, display_value, _)| display_value.clone());
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
        let rendered_values = grouped_values
            .iter()
            .map(|(_, display_value, _)| display_value.clone())
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

fn overall_confidence(
    fields: &[OcrFieldComparison],
    consistency_checks: &[OcrConsistencyCheck],
) -> ConfidenceLevel {
    let mut confidence = if fields
        .iter()
        .any(|field| field.status == OcrComparisonStatus::Divergent)
    {
        ConfidenceLevel::Low
    } else if fields
        .iter()
        .any(|field| field.status == OcrComparisonStatus::PartialConsensus)
        || fields
            .iter()
            .any(|field| field.status == OcrComparisonStatus::Missing)
    {
        ConfidenceLevel::Medium
    } else if fields
        .iter()
        .all(|field| field.confidence == ConfidenceLevel::High)
    {
        ConfidenceLevel::High
    } else {
        ConfidenceLevel::Medium
    };
    if consistency_checks
        .iter()
        .any(|check| check.status == OcrConsistencyStatus::Warning)
    {
        confidence = lower_confidence(confidence);
    }
    confidence
}

fn lower_confidence(value: ConfidenceLevel) -> ConfidenceLevel {
    match value {
        ConfidenceLevel::High => ConfidenceLevel::Medium,
        ConfidenceLevel::Medium => ConfidenceLevel::Low,
        ConfidenceLevel::Low => ConfidenceLevel::Low,
    }
}

fn normalize_generic_value(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn exact_value_equivalent(left: &str, right: &str) -> bool {
    left == right
}

fn normalize_merchant_name(value: &str) -> String {
    let normalized = value
        .to_ascii_lowercase()
        .replace('.', " ")
        .replace(',', " ")
        .replace('&', " and ");
    let normalized = normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches(|ch: char| matches!(ch, '-' | ':' | '*' | '•') || ch.is_whitespace())
        .to_owned();
    strip_common_merchant_prefix(&normalized).to_owned()
}

fn strip_common_merchant_prefix<'a>(value: &'a str) -> &'a str {
    [
        "merchant name:",
        "merchant:",
        "merchant name",
        "merchant",
        "store:",
        "store",
        "vendor:",
        "vendor",
        "company:",
        "company",
    ]
    .into_iter()
    .find_map(|prefix| value.strip_prefix(prefix).map(str::trim))
    .unwrap_or(value)
}

fn collapse_alphanumeric(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn merchant_name_equivalent(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let collapsed_left = collapse_alphanumeric(left);
    let collapsed_right = collapse_alphanumeric(right);
    if collapsed_left == collapsed_right {
        return true;
    }
    if collapsed_left.len() < 8 || collapsed_right.len() < 8 {
        return false;
    }
    let max_len = collapsed_left.len().max(collapsed_right.len());
    let allowed_distance = if max_len >= 24 { 2 } else { 1 };
    bounded_levenshtein_distance(&collapsed_left, &collapsed_right, allowed_distance)
        .is_some_and(|distance| distance <= allowed_distance)
}

fn bounded_levenshtein_distance(left: &str, right: &str, limit: usize) -> Option<usize> {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let left_len = left_chars.len();
    let right_len = right_chars.len();

    if left_len.abs_diff(right_len) > limit {
        return None;
    }

    let mut previous = (0..=right_len).collect::<Vec<_>>();
    let mut current = vec![0usize; right_len + 1];

    for (left_index, left_char) in left_chars.iter().enumerate() {
        current[0] = left_index + 1;
        let mut row_minimum = current[0];
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            let substitution = previous[right_index] + substitution_cost;
            let value = insertion.min(deletion).min(substitution);
            current[right_index + 1] = value;
            row_minimum = row_minimum.min(value);
        }
        if row_minimum > limit {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }

    let distance = previous[right_len];
    (distance <= limit).then_some(distance)
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

fn consistency_status_label(value: OcrConsistencyStatus) -> &'static str {
    match value {
        OcrConsistencyStatus::Pass => "pass",
        OcrConsistencyStatus::Warning => "warning",
        OcrConsistencyStatus::Unavailable => "unavailable",
    }
}

fn consistency_status_class(value: OcrConsistencyStatus) -> &'static str {
    match value {
        OcrConsistencyStatus::Pass => "consensus",
        OcrConsistencyStatus::Warning => "divergent",
        OcrConsistencyStatus::Unavailable => "missing",
    }
}

fn status_label(value: OcrComparisonStatus) -> &'static str {
    match value {
        OcrComparisonStatus::Consensus => "consensus",
        OcrComparisonStatus::PartialConsensus => "partial consensus",
        OcrComparisonStatus::Divergent => "divergent",
        OcrComparisonStatus::Missing => "missing",
    }
}

fn status_class(value: OcrComparisonStatus) -> &'static str {
    match value {
        OcrComparisonStatus::Consensus => "consensus",
        OcrComparisonStatus::PartialConsensus => "partialconsensus",
        OcrComparisonStatus::Divergent => "divergent",
        OcrComparisonStatus::Missing => "missing",
    }
}

fn metric_card(label: &str, value: &str) -> String {
    format!(
        "<div class=\"metric\"><div class=\"metric-label\">{}</div><div class=\"metric-value\">{}</div></div>",
        escape_html(label),
        escape_html(value)
    )
}

fn detail_row(label: &str, value: &str) -> String {
    format!(
        "<div class=\"detail-row\"><div class=\"detail-label\">{}</div><div class=\"detail-value\">{}</div></div>",
        escape_html(label),
        escape_html(value)
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
                grounding_preprocess_variant: None,
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
    fn merchant_name_comparison_tolerates_small_ocr_edits() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Merchant Name: BOOK TA .K (TAMAN DAYA) SDN BHD\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );
        let verification = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n\n- Merchant Name: BOOK TALK (TAMAN DAYA) SDN BHD\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );

        let comparison = compare_ocr_passes(&[primary, verification]).expect("compare should work");
        let merchant = comparison
            .fields
            .iter()
            .find(|field| field.field == "merchant_name")
            .expect("merchant comparison");

        assert_eq!(merchant.status, OcrComparisonStatus::Consensus);
        assert_eq!(comparison.disagreement_count, 0);
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
    fn receipt_consistency_warning_lowers_confidence_even_when_passes_agree() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Subtotal: MYR 8.00\n- Tax: MYR 1.00\n- Total: MYR 20.00\n",
        );
        let verification = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n\n- Merchant Name: BOOK TALK\n- Date: 25/12/2018\n- Subtotal: MYR 8.00\n- Tax: MYR 1.00\n- Total: MYR 20.00\n",
        );

        let comparison = compare_ocr_passes(&[primary, verification]).expect("compare should work");

        assert_eq!(comparison.disagreement_count, 0);
        assert_eq!(comparison.overall_confidence, ConfidenceLevel::Medium);
        assert!(comparison
            .consistency_checks
            .iter()
            .any(|check| check.status == OcrConsistencyStatus::Warning));
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
    fn html_render_lists_consensus_and_pass_details() {
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
        let html = render_ocr_comparison_html(&comparison);

        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("OCR Pass Comparison"));
        assert!(html.contains("receipt_primary_original"));
        assert!(html.contains("consensus"));
        assert!(html.contains("Book Talk"));
        assert!(html.contains("Per-pass extraction"));
        assert!(html.contains("Field Comparison"));
        assert!(html.contains("candidate-grid"));
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

    #[test]
    fn summary_extracts_confidence_and_divergent_fields() {
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
        let summary = summarize_ocr_comparison(&comparison);

        assert_eq!(summary.document_id, "receipt");
        assert_eq!(summary.compared_pass_count, 2);
        assert_eq!(summary.overall_confidence, ConfidenceLevel::Low);
        assert_eq!(summary.disagreement_count, 1);
        assert_eq!(summary.divergent_fields, vec!["total_paid".to_owned()]);
        assert!(summary
            .field_summaries
            .iter()
            .any(|field| field.field == "total_paid"
                && field.status == OcrComparisonStatus::Divergent
                && field.confidence == ConfidenceLevel::Low));
    }

    #[test]
    fn merchant_name_equivalence_stays_conservative_for_distinct_merchants() {
        assert!(merchant_name_equivalent(
            "book ta k taman daya sdn bhd",
            "book talk taman daya sdn bhd"
        ));
        assert_eq!(
            normalize_merchant_name("- Merchant: BOOK TALK (TAMAN DAYA) SDN BHD"),
            normalize_merchant_name("BOOK TALK (TAMAN DAYA) SDN BHD")
        );
        assert!(!merchant_name_equivalent(
            "book talk taman daya sdn bhd",
            "book world taman daya sdn bhd"
        ));
    }

    #[test]
    fn receipt_consensus_resolution_fills_missing_fields_from_secondary_pass() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n- Total: 9.00\n",
        );
        let secondary = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n- Merchant Name: BOOK TALK\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );

        let primary_facts = extract_document_facts(&primary);
        let secondary_facts = extract_document_facts(&secondary);
        let comparison = compare_ocr_passes(&[primary, secondary]).expect("compare should work");
        let resolved = resolve_receipt_ocr_consensus(&primary_facts, &secondary_facts, &comparison)
            .expect("resolution should work");

        let DocumentFactsPayload::Receipt(receipt) = &resolved.facts else {
            panic!("expected receipt facts");
        };

        assert_eq!(
            receipt
                .merchant_name
                .as_ref()
                .map(|value| value.value.as_str()),
            Some("BOOK TALK")
        );
        assert_eq!(
            receipt
                .transaction_date
                .as_ref()
                .map(|value| value.value.as_str()),
            Some("25/12/2018")
        );
        assert_eq!(
            receipt
                .total_paid
                .as_ref()
                .and_then(|value| value.value.currency.as_deref()),
            Some("MYR")
        );
        assert!(resolved
            .issues
            .iter()
            .any(|issue| issue.code == "ocr_secondary_fill_merchant_name"));
        assert_eq!(resolved.extraction_status, ExtractionStatus::Partial);
    }

    #[test]
    fn receipt_consensus_resolution_marks_divergent_fields_for_review() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
        );
        let secondary = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 90.00\n",
        );

        let primary_facts = extract_document_facts(&primary);
        let secondary_facts = extract_document_facts(&secondary);
        let comparison = compare_ocr_passes(&[primary, secondary]).expect("compare should work");
        let resolved = resolve_receipt_ocr_consensus(&primary_facts, &secondary_facts, &comparison)
            .expect("resolution should work");

        let DocumentFactsPayload::Receipt(receipt) = &resolved.facts else {
            panic!("expected receipt facts");
        };

        assert_eq!(
            receipt.total_paid.as_ref().map(|value| value.confidence),
            Some(ConfidenceLevel::Low)
        );
        assert!(receipt.total_paid.as_ref().is_some_and(|value| value
            .flags
            .iter()
            .any(|flag| flag == "ocr_pass_disagreement_total_paid")));
        assert!(resolved
            .issues
            .iter()
            .any(|issue| issue.code == "ocr_pass_disagreement_total_paid"));
        assert_eq!(resolved.extraction_status, ExtractionStatus::NeedsReview);
    }
}
