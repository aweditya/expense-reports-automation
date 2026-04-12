use std::fmt;

use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;
use serde_yaml::Mapping as YamlMapping;
use serde_yaml::Value as YamlValue;

use crate::draft::{ConfidenceLevel, DraftReport, EvidenceKind, EvidenceReference, FieldMetadata};
use crate::value::ReportValue;

#[derive(Debug)]
pub enum RenderDraftReportError {
    MissingMetadata(String),
    Yaml(serde_yaml::Error),
    Json(serde_json::Error),
}

impl fmt::Display for RenderDraftReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingMetadata(path) => {
                write!(f, "cannot render draft instance: missing metadata for leaf field {path}")
            }
            Self::Yaml(err) => write!(f, "YAML render error: {err}"),
            Self::Json(err) => write!(f, "JSON render error: {err}"),
        }
    }
}

impl std::error::Error for RenderDraftReportError {}

impl From<serde_yaml::Error> for RenderDraftReportError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Yaml(value)
    }
}

impl From<serde_json::Error> for RenderDraftReportError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn render_draft_report_yaml(draft: &DraftReport) -> Result<String, RenderDraftReportError> {
    let value = draft_to_yaml_value(draft)?;
    Ok(serde_yaml::to_string(&value)?)
}

pub fn render_draft_report_json_pretty(draft: &DraftReport) -> Result<String, RenderDraftReportError> {
    let value = draft_to_json_value(draft)?;
    Ok(serde_json::to_string_pretty(&value)?)
}

pub fn draft_to_yaml_value(draft: &DraftReport) -> Result<YamlValue, RenderDraftReportError> {
    let mut root = YamlMapping::new();
    root.insert(
        YamlValue::String("expense_report".to_owned()),
        render_report_value_yaml(&draft.report, "expense_report", draft)?,
    );
    Ok(YamlValue::Mapping(root))
}

pub fn draft_to_json_value(draft: &DraftReport) -> Result<JsonValue, RenderDraftReportError> {
    let mut root = JsonMap::new();
    root.insert(
        "expense_report".to_owned(),
        render_report_value_json(&draft.report, "expense_report", draft)?,
    );
    Ok(JsonValue::Object(root))
}

fn render_report_value_yaml(
    value: &ReportValue,
    path: &str,
    draft: &DraftReport,
) -> Result<YamlValue, RenderDraftReportError> {
    match value {
        ReportValue::Null => Ok(YamlValue::Null),
        ReportValue::Object(object) => {
            let mut mapping = YamlMapping::new();
            for (key, value) in object {
                mapping.insert(
                    YamlValue::String(key.clone()),
                    render_report_value_yaml(value, &format!("{path}.{key}"), draft)?,
                );
            }
            Ok(YamlValue::Mapping(mapping))
        }
        ReportValue::Array(values) => Ok(YamlValue::Sequence(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    render_report_value_yaml(value, &format!("{path}[{index}]"), draft)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        leaf => render_wrapped_leaf_yaml(leaf, path, draft),
    }
}

fn render_report_value_json(
    value: &ReportValue,
    path: &str,
    draft: &DraftReport,
) -> Result<JsonValue, RenderDraftReportError> {
    match value {
        ReportValue::Null => Ok(JsonValue::Null),
        ReportValue::Object(object) => {
            let mut mapping = JsonMap::new();
            for (key, value) in object {
                mapping.insert(
                    key.clone(),
                    render_report_value_json(value, &format!("{path}.{key}"), draft)?,
                );
            }
            Ok(JsonValue::Object(mapping))
        }
        ReportValue::Array(values) => Ok(JsonValue::Array(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    render_report_value_json(value, &format!("{path}[{index}]"), draft)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        leaf => render_wrapped_leaf_json(leaf, path, draft),
    }
}

fn render_wrapped_leaf_yaml(
    leaf: &ReportValue,
    path: &str,
    draft: &DraftReport,
) -> Result<YamlValue, RenderDraftReportError> {
    let metadata = draft
        .metadata
        .get(path)
        .ok_or_else(|| RenderDraftReportError::MissingMetadata(path.to_owned()))?;

    let mut mapping = YamlMapping::new();
    mapping.insert(YamlValue::String("value".to_owned()), render_leaf_yaml(leaf));
    mapping.insert(
        YamlValue::String("_meta".to_owned()),
        field_metadata_to_yaml(metadata),
    );
    Ok(YamlValue::Mapping(mapping))
}

fn render_wrapped_leaf_json(
    leaf: &ReportValue,
    path: &str,
    draft: &DraftReport,
) -> Result<JsonValue, RenderDraftReportError> {
    let metadata = draft
        .metadata
        .get(path)
        .ok_or_else(|| RenderDraftReportError::MissingMetadata(path.to_owned()))?;

    let mut mapping = JsonMap::new();
    mapping.insert("value".to_owned(), render_leaf_json(leaf));
    mapping.insert("_meta".to_owned(), field_metadata_to_json(metadata));
    Ok(JsonValue::Object(mapping))
}

fn render_leaf_yaml(value: &ReportValue) -> YamlValue {
    match value {
        ReportValue::Null => YamlValue::Null,
        ReportValue::String(value) => YamlValue::String(value.clone()),
        ReportValue::Bool(value) => YamlValue::Bool(*value),
        ReportValue::Number(value) => YamlValue::String(value.clone()),
        ReportValue::Date(value) => YamlValue::String(value.clone()),
        ReportValue::Array(_) | ReportValue::Object(_) => unreachable!(),
    }
}

fn render_leaf_json(value: &ReportValue) -> JsonValue {
    match value {
        ReportValue::Null => JsonValue::Null,
        ReportValue::String(value) => JsonValue::String(value.clone()),
        ReportValue::Bool(value) => JsonValue::Bool(*value),
        ReportValue::Number(value) => JsonValue::String(value.clone()),
        ReportValue::Date(value) => JsonValue::String(value.clone()),
        ReportValue::Array(_) | ReportValue::Object(_) => unreachable!(),
    }
}

fn field_metadata_to_yaml(metadata: &FieldMetadata) -> YamlValue {
    let mut mapping = YamlMapping::new();
    mapping.insert(
        YamlValue::String("confidence".to_owned()),
        YamlValue::String(confidence_to_str(metadata.confidence).to_owned()),
    );
    mapping.insert(
        YamlValue::String("evidence".to_owned()),
        YamlValue::Sequence(
            metadata
                .evidence
                .iter()
                .map(evidence_reference_to_yaml)
                .collect(),
        ),
    );
    mapping.insert(
        YamlValue::String("needs_review".to_owned()),
        YamlValue::Bool(metadata.needs_review),
    );
    mapping.insert(
        YamlValue::String("flags".to_owned()),
        YamlValue::Sequence(
            metadata
                .flags
                .iter()
                .cloned()
                .map(YamlValue::String)
                .collect(),
        ),
    );
    YamlValue::Mapping(mapping)
}

fn field_metadata_to_json(metadata: &FieldMetadata) -> JsonValue {
    let mut mapping = JsonMap::new();
    mapping.insert(
        "confidence".to_owned(),
        JsonValue::String(confidence_to_str(metadata.confidence).to_owned()),
    );
    mapping.insert(
        "evidence".to_owned(),
        JsonValue::Array(
            metadata
                .evidence
                .iter()
                .map(evidence_reference_to_json)
                .collect(),
        ),
    );
    mapping.insert("needs_review".to_owned(), JsonValue::Bool(metadata.needs_review));
    mapping.insert(
        "flags".to_owned(),
        JsonValue::Array(
            metadata
                .flags
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    JsonValue::Object(mapping)
}

fn evidence_reference_to_yaml(reference: &EvidenceReference) -> YamlValue {
    let mut mapping = YamlMapping::new();
    mapping.insert(
        YamlValue::String("kind".to_owned()),
        YamlValue::String(evidence_kind_to_str(reference.kind).to_owned()),
    );
    maybe_insert_yaml_string(&mut mapping, "document_id", reference.document_id.as_deref());
    maybe_insert_yaml_string(&mut mapping, "filename", reference.filename.as_deref());
    maybe_insert_yaml_u32(&mut mapping, "page", reference.page);
    maybe_insert_yaml_string(&mut mapping, "quote", reference.quote.as_deref());
    maybe_insert_yaml_string(&mut mapping, "origin", reference.origin.as_deref());
    YamlValue::Mapping(mapping)
}

fn evidence_reference_to_json(reference: &EvidenceReference) -> JsonValue {
    let mut mapping = JsonMap::new();
    mapping.insert(
        "kind".to_owned(),
        JsonValue::String(evidence_kind_to_str(reference.kind).to_owned()),
    );
    maybe_insert_json_string(&mut mapping, "document_id", reference.document_id.as_deref());
    maybe_insert_json_string(&mut mapping, "filename", reference.filename.as_deref());
    maybe_insert_json_u32(&mut mapping, "page", reference.page);
    maybe_insert_json_string(&mut mapping, "quote", reference.quote.as_deref());
    maybe_insert_json_string(&mut mapping, "origin", reference.origin.as_deref());
    JsonValue::Object(mapping)
}

fn maybe_insert_yaml_string(mapping: &mut YamlMapping, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        mapping.insert(
            YamlValue::String(key.to_owned()),
            YamlValue::String(value.to_owned()),
        );
    }
}

fn maybe_insert_yaml_u32(mapping: &mut YamlMapping, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        mapping.insert(
            YamlValue::String(key.to_owned()),
            YamlValue::Number(serde_yaml::Number::from(value)),
        );
    }
}

fn maybe_insert_json_string(mapping: &mut JsonMap<String, JsonValue>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        mapping.insert(key.to_owned(), JsonValue::String(value.to_owned()));
    }
}

fn maybe_insert_json_u32(mapping: &mut JsonMap<String, JsonValue>, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        mapping.insert(key.to_owned(), JsonValue::Number(value.into()));
    }
}

fn confidence_to_str(value: ConfidenceLevel) -> &'static str {
    match value {
        ConfidenceLevel::High => "high",
        ConfidenceLevel::Medium => "medium",
        ConfidenceLevel::Low => "low",
    }
}

fn evidence_kind_to_str(value: EvidenceKind) -> &'static str {
    match value {
        EvidenceKind::Document => "document",
        EvidenceKind::DocumentSpan => "document_span",
        EvidenceKind::SystemGenerated => "system_generated",
        EvidenceKind::UserInput => "user_input",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::{DraftReport, EvidenceKind, EvidenceReference, FieldMetadata};
    use crate::parse::{parse_document_str, ReportFormat};
    use crate::value::ReportValue;
    use std::collections::BTreeMap;

    fn sample_draft() -> DraftReport {
        DraftReport {
            report: ReportValue::object([(
                "general_information",
                ReportValue::object([("category", ReportValue::String("expenses_domestic".to_owned()))]),
            )]),
            metadata: BTreeMap::from([(
                "expense_report.general_information.category".to_owned(),
                FieldMetadata {
                    confidence: ConfidenceLevel::High,
                    evidence: vec![EvidenceReference {
                        kind: EvidenceKind::DocumentSpan,
                        document_id: Some("doc_itinerary".to_owned()),
                        filename: Some("itinerary.pdf".to_owned()),
                        page: Some(1),
                        quote: Some("Expenses (Domestic)".to_owned()),
                        origin: None,
                    }],
                    needs_review: false,
                    flags: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn renders_round_trip_yaml_for_draft_instances() {
        let draft = sample_draft();
        let yaml = render_draft_report_yaml(&draft).expect("yaml should render");
        let parsed = crate::draft::parse_draft_report_value(
            parse_document_str(&yaml, ReportFormat::Yaml).expect("rendered yaml should parse"),
        )
        .expect("rendered yaml should unwrap");
        assert_eq!(draft, parsed);
    }
}
