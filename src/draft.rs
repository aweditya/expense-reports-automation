use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::parse::{parse_document_path, ParseReportError};
use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMetadata {
    pub confidence: ConfidenceLevel,
    pub source_document: String,
    pub needs_review: bool,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub fn parse_draft_report_path(path: impl AsRef<Path>) -> Result<DraftReport, ParseDraftReportError> {
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
                .map(|(index, value)| unwrap_draft_node(value, &format!("{path}[{index}]"), metadata))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        other => Ok(other),
    }
}

fn looks_like_wrapped_value(object: &BTreeMap<String, ReportValue>) -> bool {
    object.contains_key("value")
        && object
            .keys()
            .all(|key| key == "value" || key == "_meta")
}

fn parse_field_metadata(value: ReportValue, path: &str) -> Result<FieldMetadata, ParseDraftReportError> {
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

    let source_document = match object.remove("source_document") {
        Some(ReportValue::String(value)) if !value.trim().is_empty() => value,
        _ => {
            return Err(ParseDraftReportError::InvalidMetadata(format!(
                "_meta.source_document at {path} must be a non-empty string"
            )))
        }
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

    if !object.is_empty() {
        return Err(ParseDraftReportError::InvalidMetadata(format!(
            "_meta at {path} contains unexpected keys"
        )));
    }

    Ok(FieldMetadata {
        confidence,
        source_document,
        needs_review,
        flags,
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
        source_document: booking.pdf
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
}
