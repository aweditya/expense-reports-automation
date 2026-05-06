use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::parse::{parse_document_path, ParseReportError};
use crate::value::ReportValue;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldMetadata {
    pub confidence: ConfidenceLevel,
    pub evidence: Vec<EvidenceReference>,
    pub needs_review: bool,
    pub flags: Vec<String>,
    /// One short sentence the extractor wrote justifying the confidence
    /// level. Helps the FA understand "why was this medium and not high?"
    /// without having to look at the receipt themselves.
    /// Optional with `#[serde(default)]` so older cached extractions
    /// (pre-Stage-6, no confidence_reason field) still deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftReport {
    pub report: ReportValue,
    pub metadata: BTreeMap<String, FieldMetadata>,
}

#[derive(Debug)]
pub enum ParseDraftReportError {
    Parse(ParseReportError),
    InvalidStructure(String),
    InvalidMetadata(String),
}

impl fmt::Display for ParseDraftReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::InvalidStructure(message) => write!(f, "Invalid draft structure: {message}"),
            Self::InvalidMetadata(message) => write!(f, "Invalid field metadata: {message}"),
        }
    }
}

impl std::error::Error for ParseDraftReportError {}

impl From<ParseReportError> for ParseDraftReportError {
    fn from(value: ParseReportError) -> Self {
        Self::Parse(value)
    }
}

pub fn parse_draft_report_path(
    path: impl AsRef<Path>,
) -> Result<DraftReport, ParseDraftReportError> {
    let root = parse_document_path(path)?;
    parse_draft_report_value(root)
}

pub fn parse_draft_report_value(root: ReportValue) -> Result<DraftReport, ParseDraftReportError> {
    let report_root = match root {
        ReportValue::Object(mut object) => object
            .remove("expense_report")
            .unwrap_or_else(|| ReportValue::Object(object)),
        other => {
            return Err(ParseDraftReportError::InvalidStructure(format!(
                "draft root must be an object, found {:?}",
                other.kind()
            )))
        }
    };

    if !matches!(report_root, ReportValue::Object(_)) {
        return Err(ParseDraftReportError::InvalidStructure(
            "top-level expense_report must contain an object".to_owned(),
        ));
    }

    let mut metadata = BTreeMap::new();
    let report = unwrap_draft_node(report_root, "expense_report", &mut metadata)?;
    Ok(DraftReport { report, metadata })
}

fn unwrap_draft_node(
    value: ReportValue,
    path: &str,
    metadata: &mut BTreeMap<String, FieldMetadata>,
) -> Result<ReportValue, ParseDraftReportError> {
    match value {
        ReportValue::Object(mut object) => {
            if looks_like_wrapped_value(&object) {
                let meta = object.remove("_meta");
                let wrapped = object.remove("value").ok_or_else(|| {
                    ParseDraftReportError::InvalidStructure(format!(
                        "wrapped field at {path} is missing its value"
                    ))
                })?;
                if !object.is_empty() {
                    return Err(ParseDraftReportError::InvalidStructure(format!(
                        "wrapped field at {path} contains unexpected keys"
                    )));
                }

                if let Some(meta) = meta {
                    metadata.insert(path.to_owned(), parse_field_metadata(meta, path)?);
                }
                unwrap_draft_node(wrapped, path, metadata)
            } else {
                let mut unwrapped = BTreeMap::new();
                for (key, value) in object {
                    let child_path = format!("{path}.{key}");
                    unwrapped.insert(key, unwrap_draft_node(value, &child_path, metadata)?);
                }
                Ok(ReportValue::Object(unwrapped))
            }
        }
        ReportValue::Array(values) => Ok(ReportValue::Array(
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    unwrap_draft_node(value, &format!("{path}[{index}]"), metadata)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        other => Ok(other),
    }
}

fn looks_like_wrapped_value(object: &BTreeMap<String, ReportValue>) -> bool {
    object.contains_key("value") && object.keys().all(|key| key == "value" || key == "_meta")
}

fn parse_field_metadata(
    value: ReportValue,
    path: &str,
) -> Result<FieldMetadata, ParseDraftReportError> {
    let ReportValue::Object(mut object) = value else {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta at {path} must be an object"
        )));
    };

    let confidence = match object.remove("confidence") {
        Some(ReportValue::String(value)) => parse_confidence(&value, path)?,
        _ => {
            return Err(ParseDraftReportError::InvalidMetadata(format!(
                "_meta.confidence at {path} must be one of high|medium|low"
            )))
        }
    };

    let evidence = if let Some(evidence_value) = object.remove("evidence") {
        parse_evidence_list(evidence_value, path)?
    } else if let Some(source_document_value) = object.remove("source_document") {
        vec![legacy_evidence_reference(source_document_value, path)?]
    } else {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta at {path} must contain either evidence or source_document"
        )));
    };

    let needs_review = match object.remove("needs_review") {
        Some(ReportValue::Bool(value)) => value,
        _ => {
            return Err(ParseDraftReportError::InvalidMetadata(format!(
                "_meta.needs_review at {path} must be a boolean"
            )))
        }
    };

    let flags = match object.remove("flags") {
        Some(ReportValue::Array(values)) => values
            .into_iter()
            .map(|value| match value {
                ReportValue::String(value) => Ok(value),
                _ => Err(ParseDraftReportError::InvalidMetadata(format!(
                    "_meta.flags at {path} must contain only strings"
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(ParseDraftReportError::InvalidMetadata(format!(
                "_meta.flags at {path} must be an array of strings"
            )))
        }
        None => Vec::new(),
    };

    // Optional one-line justification for the chosen confidence level.
    // Older drafts (pre-Stage-6) won't have it; missing → None.
    let confidence_reason = match object.remove("confidence_reason") {
        Some(ReportValue::String(value)) => Some(value),
        Some(_) => {
            return Err(ParseDraftReportError::InvalidMetadata(format!(
                "_meta.confidence_reason at {path} must be a string"
            )))
        }
        None => None,
    };

    if !object.is_empty() {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta at {path} contains unexpected keys"
        )));
    }

    Ok(FieldMetadata {
        confidence,
        evidence,
        needs_review,
        flags,
        confidence_reason,
    })
}

fn parse_confidence(value: &str, path: &str) -> Result<ConfidenceLevel, ParseDraftReportError> {
    match value {
        "high" => Ok(ConfidenceLevel::High),
        "medium" => Ok(ConfidenceLevel::Medium),
        "low" => Ok(ConfidenceLevel::Low),
        _ => Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta.confidence at {path} must be one of high|medium|low"
        ))),
    }
}

fn parse_evidence_list(
    value: ReportValue,
    path: &str,
) -> Result<Vec<EvidenceReference>, ParseDraftReportError> {
    let ReportValue::Array(values) = value else {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta.evidence at {path} must be an array"
        )));
    };

    if values.is_empty() {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta.evidence at {path} must not be empty"
        )));
    }

    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            parse_evidence_reference(value, &format!("{path}._meta.evidence[{index}]"))
        })
        .collect()
}

fn legacy_evidence_reference(
    value: ReportValue,
    path: &str,
) -> Result<EvidenceReference, ParseDraftReportError> {
    let ReportValue::String(source_document) = value else {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta.source_document at {path} must be a non-empty string"
        )));
    };
    if source_document.trim().is_empty() {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta.source_document at {path} must be a non-empty string"
        )));
    }

    let kind = match source_document.as_str() {
        "system_generated" => EvidenceKind::SystemGenerated,
        "fa_input" | "payee_form" | "user_input" => EvidenceKind::UserInput,
        _ => EvidenceKind::Document,
    };

    Ok(match kind {
        EvidenceKind::SystemGenerated => EvidenceReference {
            kind,
            document_id: None,
            filename: None,
            page: None,
            quote: None,
            origin: Some(source_document),
        },
        EvidenceKind::UserInput => EvidenceReference {
            kind,
            document_id: None,
            filename: None,
            page: None,
            quote: None,
            origin: Some(source_document),
        },
        EvidenceKind::Document => EvidenceReference {
            kind,
            document_id: None,
            filename: Some(source_document),
            page: None,
            quote: None,
            origin: None,
        },
        EvidenceKind::DocumentSpan => unreachable!(),
    })
}

fn parse_evidence_reference(
    value: ReportValue,
    path: &str,
) -> Result<EvidenceReference, ParseDraftReportError> {
    let ReportValue::Object(mut object) = value else {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "evidence reference at {path} must be an object"
        )));
    };

    let kind = match object.remove("kind") {
        Some(ReportValue::String(value)) => parse_evidence_kind(&value, path)?,
        _ => {
            return Err(ParseDraftReportError::InvalidMetadata(format!(
                "evidence reference kind at {path} must be one of document|document_span|system_generated|user_input"
            )))
        }
    };

    let document_id = parse_optional_string(object.remove("document_id"), path, "document_id")?;
    let filename = parse_optional_string(object.remove("filename"), path, "filename")?;
    let page = parse_optional_u32(object.remove("page"), path, "page")?;
    let quote = parse_optional_string(object.remove("quote"), path, "quote")?;
    let origin = parse_optional_string(object.remove("origin"), path, "origin")?;

    if !object.is_empty() {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "evidence reference at {path} contains unexpected keys"
        )));
    }

    validate_evidence_reference_shape(
        EvidenceReference {
            kind,
            document_id,
            filename,
            page,
            quote,
            origin,
        },
        path,
    )
}

fn parse_evidence_kind(value: &str, path: &str) -> Result<EvidenceKind, ParseDraftReportError> {
    match value {
        "document" => Ok(EvidenceKind::Document),
        "document_span" => Ok(EvidenceKind::DocumentSpan),
        "system_generated" => Ok(EvidenceKind::SystemGenerated),
        "user_input" => Ok(EvidenceKind::UserInput),
        _ => Err(ParseDraftReportError::InvalidMetadata(format!(
            "evidence reference kind at {path} must be one of document|document_span|system_generated|user_input"
        ))),
    }
}

fn parse_optional_string(
    value: Option<ReportValue>,
    path: &str,
    key: &str,
) -> Result<Option<String>, ParseDraftReportError> {
    match value {
        None => Ok(None),
        Some(ReportValue::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        Some(_) => Err(ParseDraftReportError::InvalidMetadata(format!(
            "{key} at {path} must be a non-empty string when present"
        ))),
    }
}

fn parse_optional_u32(
    value: Option<ReportValue>,
    path: &str,
    key: &str,
) -> Result<Option<u32>, ParseDraftReportError> {
    match value {
        None => Ok(None),
        Some(ReportValue::Number(value)) | Some(ReportValue::String(value)) => {
            value.parse::<u32>().map(Some).map_err(|_| {
                ParseDraftReportError::InvalidMetadata(format!(
                    "{key} at {path} must be a positive integer when present"
                ))
            })
        }
        Some(_) => Err(ParseDraftReportError::InvalidMetadata(format!(
            "{key} at {path} must be a positive integer when present"
        ))),
    }
}

fn validate_evidence_reference_shape(
    reference: EvidenceReference,
    path: &str,
) -> Result<EvidenceReference, ParseDraftReportError> {
    let has_document_handle = reference.document_id.is_some() || reference.filename.is_some();

    match reference.kind {
        EvidenceKind::Document => {
            if !has_document_handle {
                return Err(ParseDraftReportError::InvalidMetadata(format!(
                    "document evidence at {path} must include document_id or filename"
                )));
            }
        }
        EvidenceKind::DocumentSpan => {
            if !has_document_handle || reference.page.is_none() || reference.quote.is_none() {
                return Err(ParseDraftReportError::InvalidMetadata(format!(
                    "document_span evidence at {path} must include document_id or filename, page, and quote"
                )));
            }
        }
        EvidenceKind::SystemGenerated | EvidenceKind::UserInput => {
            if reference.origin.is_none() {
                return Err(ParseDraftReportError::InvalidMetadata(format!(
                    "{:?} evidence at {path} must include origin",
                    reference.kind
                )));
            }
            if has_document_handle || reference.page.is_some() || reference.quote.is_some() {
                return Err(ParseDraftReportError::InvalidMetadata(format!(
                    "{:?} evidence at {path} must not include document/page/quote fields",
                    reference.kind
                )));
            }
        }
    }

    Ok(reference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_document_str, ReportFormat};

    #[test]
    fn unwraps_wrapped_leaf_values_and_metadata() {
        let root = parse_document_str(
            r#"
expense_report:
  general_information:
    category:
      value: expenses_domestic
      _meta:
        confidence: high
        evidence:
          - kind: document_span
            document_id: doc_booking
            filename: booking.pdf
            page: 1
            quote: Expenses (Domestic)
        needs_review: false
        flags: []
"#,
            ReportFormat::Yaml,
        )
        .expect("draft yaml should parse");

        let draft = parse_draft_report_value(root).expect("draft should unwrap");
        let ReportValue::Object(report) = draft.report else {
            panic!("expected report object");
        };
        let ReportValue::Object(general_information) = report
            .get("general_information")
            .cloned()
            .expect("general_information should exist")
        else {
            panic!("expected general_information object");
        };

        assert_eq!(
            general_information.get("category"),
            Some(&ReportValue::String("expenses_domestic".to_owned()))
        );
        assert!(draft
            .metadata
            .contains_key("expense_report.general_information.category"));
    }

    #[test]
    fn preserves_legacy_source_document_for_backward_compatibility() {
        let root = parse_document_str(
            r#"
expense_report:
  general_information:
    category:
      value: expenses_domestic
      _meta:
        confidence: high
        source_document: booking.pdf
        needs_review: false
        flags: []
"#,
            ReportFormat::Yaml,
        )
        .expect("draft yaml should parse");

        let draft = parse_draft_report_value(root).expect("draft should unwrap");
        let evidence = &draft.metadata["expense_report.general_information.category"].evidence;
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].filename.as_deref(), Some("booking.pdf"));
        assert_eq!(evidence[0].kind, EvidenceKind::Document);
    }
}
