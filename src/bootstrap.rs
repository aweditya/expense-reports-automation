use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::draft::{ConfidenceLevel, DraftReport, EvidenceKind, EvidenceReference, FieldMetadata};
use crate::transcribe::{transcribe_document_path, TranscribedDocument, TranscriptionError};
use crate::value::ReportValue;

#[derive(Debug)]
pub enum BootstrapExtractionError {
    Transcription(TranscriptionError),
    MissingField(&'static str),
    InvalidField { field: &'static str, value: String },
    UnsupportedDocument(&'static str),
}

impl fmt::Display for BootstrapExtractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transcription(err) => write!(f, "{err}"),
            Self::MissingField(field) => {
                write!(f, "missing expected Stanford summary field: {field}")
            }
            Self::InvalidField { field, value } => {
                write!(f, "invalid Stanford summary field {field}: {value}")
            }
            Self::UnsupportedDocument(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for BootstrapExtractionError {}

impl From<TranscriptionError> for BootstrapExtractionError {
    fn from(value: TranscriptionError) -> Self {
        Self::Transcription(value)
    }
}

pub fn extract_stanford_summary_path(
    path: impl AsRef<Path>,
) -> Result<DraftReport, BootstrapExtractionError> {
    let document = transcribe_document_path(path)?;
    extract_stanford_summary_document(&document)
}

pub fn extract_stanford_summary_document(
    document: &TranscribedDocument,
) -> Result<DraftReport, BootstrapExtractionError> {
    let first_page =
        document
            .pages
            .first()
            .ok_or(BootstrapExtractionError::UnsupportedDocument(
                "Stanford summary extraction requires at least one transcribed page",
            ))?;
    let normalized = normalize_whitespace(&first_page.text);

    if !normalized.contains("Expense Report") || !normalized.contains("Business Purpose") {
        return Err(BootstrapExtractionError::UnsupportedDocument(
            "document does not look like a Stanford expense report summary PDF",
        ));
    }

    let category_quote = if normalized.contains("Expenses (Foreign)") {
        "Expenses (Foreign)"
    } else if normalized.contains("Expenses (Domestic)") {
        "Expenses (Domestic)"
    } else {
        return Err(BootstrapExtractionError::MissingField(
            "general_information.category",
        ));
    };
    let category = match category_quote {
        "Expenses (Foreign)" => "expenses_foreign",
        "Expenses (Domestic)" => "expenses_domestic",
        _ => unreachable!(),
    };
    let transaction_type = match category {
        "expenses_foreign" => "foreign",
        "expenses_domestic" => "domestic",
        _ => unreachable!(),
    };

    let payee_raw = extract_between(&normalized, "Payee:", "Event Name:").ok_or(
        BootstrapExtractionError::MissingField("general_information.payee.name"),
    )?;
    let event_name = extract_between(&normalized, "Event Name:", "Report Total:").ok_or(
        BootstrapExtractionError::MissingField("general_information.event_name"),
    )?;
    let report_total_raw = extract_between_any(
        &normalized,
        "Report Total:",
        &[
            "SUNet ID:",
            "Travel Card Business Expenses:",
            "ER",
            "Expense Report",
        ],
    )
    .ok_or(BootstrapExtractionError::MissingField(
        "transaction_summary.total_usd",
    ))?;
    let report_total = normalize_amount(&report_total_raw).ok_or_else(|| {
        BootstrapExtractionError::InvalidField {
            field: "transaction_summary.total_usd",
            value: report_total_raw.clone(),
        }
    })?;
    let submitted_on_raw = extract_between(&normalized, "Submitted On:", "Advance Applied:")
        .ok_or(BootstrapExtractionError::MissingField(
            "transaction_summary.transaction_date",
        ))?;
    let submitted_on = parse_summary_date(&submitted_on_raw).ok_or_else(|| {
        BootstrapExtractionError::InvalidField {
            field: "transaction_summary.transaction_date",
            value: submitted_on_raw.clone(),
        }
    })?;
    let rush_processing_raw = extract_between(
        &normalized,
        "Rush Processing:",
        "Itemized Personal Expenses:",
    )
    .ok_or(BootstrapExtractionError::MissingField(
        "general_information.rush_processing",
    ))?;
    let rush_processing = normalize_yes_no(&rush_processing_raw).ok_or_else(|| {
        BootstrapExtractionError::InvalidField {
            field: "general_information.rush_processing",
            value: rush_processing_raw.clone(),
        }
    })?;
    let payment_method_raw =
        extract_between(&normalized, "Payment Method:", "Reimbursement Amount:").ok_or(
            BootstrapExtractionError::MissingField("general_information.payment_method"),
        )?;
    let payment_method = normalize_whitespace(&payment_method_raw).to_ascii_lowercase();
    let status_raw = extract_between(&normalized, "Status:", "Business Purpose");
    let status = status_raw
        .as_ref()
        .map(|value| normalize_whitespace(value).to_ascii_lowercase());
    let transaction_number = extract_transaction_number(&normalized).ok_or(
        BootstrapExtractionError::MissingField("transaction_summary.transaction_number"),
    )?;

    let business_purpose_block = extract_between(
        &normalized,
        "Business Purpose",
        "FYI: Business Purpose (first 30 characters)",
    )
    .ok_or(BootstrapExtractionError::MissingField(
        "general_information.business_purpose",
    ))?;
    let purpose_who = extract_between(&business_purpose_block, "WHO:", "WHAT:").ok_or(
        BootstrapExtractionError::MissingField("general_information.business_purpose.who"),
    )?;
    let purpose_what = extract_between(&business_purpose_block, "WHAT:", "WHEN:").ok_or(
        BootstrapExtractionError::MissingField("general_information.business_purpose.what"),
    )?;
    let purpose_when = extract_between(&business_purpose_block, "WHEN:", "WHERE:").ok_or(
        BootstrapExtractionError::MissingField("general_information.business_purpose.when"),
    )?;
    let purpose_where = extract_between(&business_purpose_block, "WHERE:", "WHY:").ok_or(
        BootstrapExtractionError::MissingField("general_information.business_purpose.where"),
    )?;
    let purpose_why = extract_after(&business_purpose_block, "WHY:").ok_or(
        BootstrapExtractionError::MissingField("general_information.business_purpose.why"),
    )?;
    let purpose_key_raw = extract_between(
        &normalized,
        "FYI: Business Purpose (first 30 characters)",
        "Student certification for Authorized Expense",
    )
    .ok_or(BootstrapExtractionError::MissingField(
        "general_information.business_purpose.key_30char",
    ))?;
    let purpose_key = truncate_chars(&normalize_whitespace(&purpose_key_raw), 30);
    let certification_block = extract_between(
        &normalized,
        "Student certification for Authorized Expense",
        "Expense Authorized By:",
    )
    .unwrap_or_default();
    let authorized_by = extract_between(&normalized, "Expense Authorized By:", "Affiliation:")
        .ok_or(BootstrapExtractionError::MissingField(
            "general_information.authorized_by",
        ))?;

    let payee_name = clean_payee_name(&payee_raw)
        .or_else(|| infer_payee_name_from_who(&purpose_who))
        .ok_or(BootstrapExtractionError::MissingField(
            "general_information.payee.name",
        ))?;
    let affiliation = infer_affiliation(&purpose_who);

    let certification_supports = certification_block
        .contains("Directly support faculty member's project or research program");
    let certification_presenting =
        certification_block.contains("Are related to presenting at a conference");
    let certification_degree =
        certification_block.contains("Are an integral part of this student's degree work");
    let certification_employment = certification_block.contains("Are related to employment");
    let certification_other = certification_block.contains("Other");

    let mut metadata = BTreeMap::new();

    insert_metadata(
        &mut metadata,
        "expense_report.general_information.category",
        document_span_metadata(
            document,
            first_page.page_number,
            category_quote,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.payee.name",
        document_span_metadata(
            document,
            first_page.page_number,
            &payee_name,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.payee.affiliation",
        document_span_metadata(
            document,
            first_page.page_number,
            &purpose_who,
            ConfidenceLevel::Medium,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.rush_processing",
        document_span_metadata(
            document,
            first_page.page_number,
            &format!("Rush Processing: {rush_processing_raw}"),
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.payment_method",
        document_span_metadata(
            document,
            first_page.page_number,
            &format!("Payment Method: {payment_method_raw}"),
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.business_purpose.who",
        document_span_metadata(
            document,
            first_page.page_number,
            &purpose_who,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.business_purpose.what",
        document_span_metadata(
            document,
            first_page.page_number,
            &purpose_what,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.business_purpose.when",
        document_span_metadata(
            document,
            first_page.page_number,
            &purpose_when,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.business_purpose.where",
        document_span_metadata(
            document,
            first_page.page_number,
            &purpose_where,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.business_purpose.why",
        document_span_metadata(
            document,
            first_page.page_number,
            &purpose_why,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.business_purpose.key_30char",
        document_span_metadata(
            document,
            first_page.page_number,
            &purpose_key_raw,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.event_name",
        document_span_metadata(
            document,
            first_page.page_number,
            &event_name,
            ConfidenceLevel::High,
        ),
    );
    if certification_supports {
        insert_metadata(
            &mut metadata,
            "expense_report.general_information.student_certification.supports_faculty_research",
            document_span_metadata(
                document,
                first_page.page_number,
                "Directly support faculty member's project or research program",
                ConfidenceLevel::High,
            ),
        );
    }
    if certification_presenting {
        insert_metadata(
            &mut metadata,
            "expense_report.general_information.student_certification.presenting_at_conference",
            document_span_metadata(
                document,
                first_page.page_number,
                "Are related to presenting at a conference",
                ConfidenceLevel::High,
            ),
        );
    }
    if certification_degree {
        insert_metadata(
            &mut metadata,
            "expense_report.general_information.student_certification.integral_to_degree_work",
            document_span_metadata(
                document,
                first_page.page_number,
                "Are an integral part of this student's degree work",
                ConfidenceLevel::High,
            ),
        );
    }
    if certification_employment {
        insert_metadata(
            &mut metadata,
            "expense_report.general_information.student_certification.related_to_employment",
            document_span_metadata(
                document,
                first_page.page_number,
                "Are related to employment",
                ConfidenceLevel::High,
            ),
        );
    }
    if certification_other {
        insert_metadata(
            &mut metadata,
            "expense_report.general_information.student_certification.other",
            document_span_metadata(
                document,
                first_page.page_number,
                "Other",
                ConfidenceLevel::Medium,
            ),
        );
    }
    insert_metadata(
        &mut metadata,
        "expense_report.general_information.authorized_by",
        document_span_metadata(
            document,
            first_page.page_number,
            &authorized_by,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.transaction_summary.transaction_type",
        system_generated_metadata(
            "mirrored_from_general_information.category",
            ConfidenceLevel::High,
            false,
            Vec::new(),
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.transaction_summary.transaction_number",
        document_span_metadata(
            document,
            first_page.page_number,
            &transaction_number,
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.transaction_summary.transaction_date",
        document_span_metadata(
            document,
            first_page.page_number,
            &format!("Submitted On: {submitted_on_raw}"),
            ConfidenceLevel::High,
        ),
    );
    if let Some(status) = &status {
        insert_metadata(
            &mut metadata,
            "expense_report.transaction_summary.status",
            document_span_metadata(
                document,
                first_page.page_number,
                &format!(
                    "Status: {}",
                    status_raw
                        .as_deref()
                        .map(normalize_whitespace)
                        .unwrap_or_else(|| status.clone())
                ),
                ConfidenceLevel::High,
            ),
        );
    }
    insert_metadata(
        &mut metadata,
        "expense_report.transaction_summary.total_usd",
        document_span_metadata(
            document,
            first_page.page_number,
            &format!("Report Total: {report_total_raw}"),
            ConfidenceLevel::High,
        ),
    );
    insert_metadata(
        &mut metadata,
        "expense_report.allocation_and_approvers.other_beneficiaries",
        system_generated_metadata(
            "bootstrap_extractor_default_false",
            ConfidenceLevel::Low,
            true,
            vec!["not_present_in_transaction_summary".to_owned()],
        ),
    );

    let mut student_certification_fields = Vec::new();
    if certification_supports {
        student_certification_fields.push(("supports_faculty_research", ReportValue::Bool(true)));
    }
    if certification_presenting {
        student_certification_fields.push(("presenting_at_conference", ReportValue::Bool(true)));
    }
    if certification_degree {
        student_certification_fields.push(("integral_to_degree_work", ReportValue::Bool(true)));
    }
    if certification_employment {
        student_certification_fields.push(("related_to_employment", ReportValue::Bool(true)));
    }
    if certification_other {
        student_certification_fields.push(("other", ReportValue::Bool(true)));
    }

    let mut transaction_summary_fields = vec![
        (
            "transaction_type",
            ReportValue::String(transaction_type.to_owned()),
        ),
        (
            "transaction_number",
            ReportValue::String(transaction_number),
        ),
        ("transaction_date", ReportValue::Date(submitted_on)),
        ("total_usd", ReportValue::Number(report_total)),
    ];
    if let Some(status) = status {
        transaction_summary_fields.push(("status", ReportValue::String(status)));
    }

    let report = ReportValue::object([
        (
            "general_information",
            ReportValue::object([
                ("category", ReportValue::String(category.to_owned())),
                (
                    "payee",
                    ReportValue::object([
                        ("name", ReportValue::String(payee_name)),
                        ("affiliation", ReportValue::String(affiliation.to_owned())),
                    ]),
                ),
                (
                    "rush_processing",
                    ReportValue::String(rush_processing.to_owned()),
                ),
                ("payment_method", ReportValue::String(payment_method)),
                (
                    "business_purpose",
                    ReportValue::object([
                        ("who", ReportValue::String(purpose_who)),
                        ("what", ReportValue::String(purpose_what)),
                        ("when", ReportValue::String(purpose_when)),
                        ("where", ReportValue::String(purpose_where)),
                        ("why", ReportValue::String(purpose_why)),
                        ("key_30char", ReportValue::String(purpose_key)),
                    ]),
                ),
                ("event_name", ReportValue::String(event_name)),
                (
                    "student_certification",
                    ReportValue::object(student_certification_fields),
                ),
                ("authorized_by", ReportValue::String(authorized_by)),
            ]),
        ),
        (
            "transaction_summary",
            ReportValue::object(transaction_summary_fields),
        ),
        (
            "allocation_and_approvers",
            ReportValue::object([("other_beneficiaries", ReportValue::Bool(false))]),
        ),
    ]);

    Ok(DraftReport { report, metadata })
}

fn insert_metadata(
    metadata: &mut BTreeMap<String, FieldMetadata>,
    path: &str,
    value: FieldMetadata,
) {
    metadata.insert(path.to_owned(), value);
}

fn document_span_metadata(
    document: &TranscribedDocument,
    page: u32,
    quote: &str,
    confidence: ConfidenceLevel,
) -> FieldMetadata {
    FieldMetadata {
        confidence,
        evidence: vec![EvidenceReference {
            kind: EvidenceKind::DocumentSpan,
            document_id: Some(document.document_id.clone()),
            filename: Some(document.filename.clone()),
            page: Some(page),
            quote: Some(normalize_whitespace(quote)),
            origin: None,
        }],
        needs_review: false,
        flags: Vec::new(),
    }
}

fn system_generated_metadata(
    origin: &str,
    confidence: ConfidenceLevel,
    needs_review: bool,
    flags: Vec<String>,
) -> FieldMetadata {
    FieldMetadata {
        confidence,
        evidence: vec![EvidenceReference {
            kind: EvidenceKind::SystemGenerated,
            document_id: None,
            filename: None,
            page: None,
            quote: None,
            origin: Some(origin.to_owned()),
        }],
        needs_review,
        flags,
    }
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_between(text: &str, start: &str, end: &str) -> Option<String> {
    let start_index = text.find(start)? + start.len();
    let rest = &text[start_index..];
    let end_index = rest.find(end)?;
    Some(rest[..end_index].trim().to_owned())
}

fn extract_between_any(text: &str, start: &str, ends: &[&str]) -> Option<String> {
    let start_index = text.find(start)? + start.len();
    let rest = &text[start_index..];
    let end_index = ends.iter().filter_map(|end| rest.find(end)).min()?;
    Some(rest[..end_index].trim().to_owned())
}

fn extract_after(text: &str, start: &str) -> Option<String> {
    let start_index = text.find(start)? + start.len();
    Some(text[start_index..].trim().to_owned())
}

fn normalize_amount(value: &str) -> Option<String> {
    let cleaned = value
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();

    if cleaned.is_empty() || !cleaned.contains('.') {
        None
    } else {
        Some(cleaned)
    }
}

fn normalize_yes_no(value: &str) -> Option<&'static str> {
    match normalize_whitespace(value).to_ascii_lowercase().as_str() {
        "yes" => Some("yes"),
        "no" => Some("no"),
        _ => None,
    }
}

fn parse_summary_date(value: &str) -> Option<String> {
    let mut parts = value.trim().split('-');
    let day = parts.next()?;
    let month = parts.next()?;
    let year = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let month = match month.to_ascii_uppercase().as_str() {
        "JAN" => "01",
        "FEB" => "02",
        "MAR" => "03",
        "APR" => "04",
        "MAY" => "05",
        "JUN" => "06",
        "JUL" => "07",
        "AUG" => "08",
        "SEP" => "09",
        "OCT" => "10",
        "NOV" => "11",
        "DEC" => "12",
        _ => return None,
    };

    Some(format!("{year}-{month}-{:0>2}", day))
}

fn extract_transaction_number(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for index in 0..bytes.len().saturating_sub(8) {
        if bytes.get(index) == Some(&b'E') && bytes.get(index + 1) == Some(&b'R') {
            let candidate = &text[index..index + 9];
            if candidate.chars().skip(2).all(|ch| ch.is_ascii_digit()) {
                return Some(candidate.to_owned());
            }
        }
    }
    None
}

fn clean_payee_name(value: &str) -> Option<String> {
    let cleaned = normalize_whitespace(value.trim_matches(|ch: char| {
        !(ch.is_ascii_alphanumeric() || ch == ' ' || ch == '\'' || ch == '-')
    }));
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn infer_payee_name_from_who(who: &str) -> Option<String> {
    let before_approval = who.split(" approved by ").next().unwrap_or(who);
    let candidate = before_approval
        .split_whitespace()
        .rev()
        .find(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()))?;
    clean_payee_name(candidate)
}

fn infer_affiliation(who: &str) -> &'static str {
    let who = who.to_ascii_lowercase();
    if who.contains("post doc") || who.contains("postdoc") {
        "stanford_postdoc"
    } else if who.contains("student") {
        "stanford_student"
    } else if who.contains("staff") {
        "stanford_staff"
    } else if who.contains("faculty") || who.contains("professor") {
        "stanford_faculty"
    } else {
        "other"
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::{
        TranscribedDocument, TranscribedPage, TranscriptionEngine, TranscriptionMetadata,
    };
    use crate::validator::validate_draft_report;
    use std::path::PathBuf;

    #[test]
    fn extracts_valid_draft_from_summary_transcript() {
        let document = TranscribedDocument {
            document_id: "er5499574_redacted".to_owned(),
            filename: "ER5499574_Redacted.pdf".to_owned(),
            source_path: PathBuf::from("ER5499574_Redacted.pdf"),
            engine: TranscriptionEngine::PdfToText,
            metadata: TranscriptionMetadata::primary_for_engine(
                TranscriptionEngine::PdfToText,
                "pdftotext",
                None,
            ),
            pages: vec![TranscribedPage::text_only(
                1,
                r#"
                    Page 1 of 3 USD
                    Transaction Summary for Payee: Olivia Event Name: ICLR 2025 Report Total: 4,973.58
                    ER5499574 Expense Report Expenses (Foreign) Category:
                    Expense Dates: 23-APR-2025 - 04-MAY-2025 Cash & Other Business Expenses: 4,973.58
                    Submitted On: 20-NOV-2025 Advance Applied:
                    Rush Processing: No Itemized Personal Expenses:
                    Payment Method: Electronic Reimbursement Amount: 4,973.58
                    Status: Paid
                    Business Purpose O. ICLR Conference Travel WHO: Electrical Engineering PhD student Olivia approved by Professor Kunle Olukotun WHAT: Travel expenses for attendance to ICLR 2025 WHEN: Travel dates April 21, 2025 - April 29, 2025 WHERE: Singapore WHY: Presented an accepted paper.
                    FYI: Business Purpose (first 30 characters) O. ICLR Conference Travel
                    Student certification for Authorized Expense
                    ● Directly support faculty member's project or research program (Requires faculty approval)
                    ● Are an integral part of this student's degree work (Requires faculty approval. Does not apply to post docs.)
                    Expense Authorized By: Olukotun, Oyekunle A. Affiliation: FACULTY
                "#
                .to_owned(),
            )],
        };

        let draft = extract_stanford_summary_document(&document).expect("summary should extract");
        let report = validate_draft_report(&draft);
        assert!(!report.has_errors());
        assert!(!report.has_warnings());

        let payee_name = draft
            .report
            .as_object()
            .and_then(|root| root.get("general_information"))
            .and_then(ReportValue::as_object)
            .and_then(|info| info.get("payee"))
            .and_then(ReportValue::as_object)
            .and_then(|payee| payee.get("name"))
            .and_then(ReportValue::as_text);
        assert_eq!(payee_name, Some("Olivia"));
    }
}
