use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::draft::{ConfidenceLevel, DraftReport, EvidenceKind, EvidenceReference, FieldMetadata};
use crate::validation_rules::{
    conditional_rules_for, field_rule, ConditionalRuleType, FieldRule, SchemaType,
};
use crate::value::{ReportValue, ValueKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationIssueKind {
    MissingRequiredField,
    TypeMismatch,
    InvalidEnumValue,
    MissingDependency,
    MissingFieldMetadata,
    MissingEvidenceReference,
    OrphanFieldMetadata,
    LowConfidenceWithoutReview,
    InvalidEvidenceReference,
    UnsupportedExpression,
    UnresolvedExpressionReference,
    ManualReviewRequired,
    InternalSchemaError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub kind: ValidationIssueKind,
    pub path: String,
    pub schema_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Warning)
    }
}

pub fn render_validation_report_json_pretty(
    report: &ValidationReport,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn validate_expense_report(report: &ReportValue) -> ValidationReport {
    let mut validator = Validator {
        issues: Vec::new(),
        root: report,
    };
    validator.validate_node(report, "expense_report", "expense_report");
    ValidationReport {
        issues: validator.issues,
    }
}

pub fn validate_draft_report(draft: &DraftReport) -> ValidationReport {
    let mut report = validate_expense_report(&draft.report);
    let leaf_paths = collect_leaf_paths(&draft.report, "expense_report");
    let leaf_set = leaf_paths.iter().cloned().collect::<BTreeSet<_>>();

    for path in &leaf_paths {
        let Some(metadata) = draft.metadata.get(path) else {
            report.issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                kind: ValidationIssueKind::MissingFieldMetadata,
                path: path.clone(),
                schema_path: path.clone(),
                message: "Leaf field is missing required extraction metadata".to_owned(),
            });
            continue;
        };

        if metadata.confidence == ConfidenceLevel::Low && !metadata.needs_review {
            report.issues.push(ValidationIssue {
                severity: ValidationSeverity::Warning,
                kind: ValidationIssueKind::LowConfidenceWithoutReview,
                path: path.clone(),
                schema_path: path.clone(),
                message: "Low-confidence field should be marked as needing review".to_owned(),
            });
        }

        validate_field_metadata(path, metadata, &mut report);
    }

    for metadata_path in draft.metadata.keys() {
        if !leaf_set.contains(metadata_path) {
            report.issues.push(ValidationIssue {
                severity: ValidationSeverity::Warning,
                kind: ValidationIssueKind::OrphanFieldMetadata,
                path: metadata_path.clone(),
                schema_path: metadata_path.clone(),
                message: "Metadata exists for a path that is not a present leaf field".to_owned(),
            });
        }
    }

    report
}

fn validate_field_metadata(path: &str, metadata: &FieldMetadata, report: &mut ValidationReport) {
    if metadata.evidence.is_empty() {
        report.issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            kind: ValidationIssueKind::MissingEvidenceReference,
            path: path.to_owned(),
            schema_path: path.to_owned(),
            message: "Field metadata must include at least one evidence reference".to_owned(),
        });
        return;
    }

    for (index, evidence) in metadata.evidence.iter().enumerate() {
        if let Some(message) = evidence_integrity_issue(evidence) {
            report.issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                kind: ValidationIssueKind::InvalidEvidenceReference,
                path: format!("{path}._meta.evidence[{index}]"),
                schema_path: path.to_owned(),
                message,
            });
        }
    }
}

fn evidence_integrity_issue(evidence: &EvidenceReference) -> Option<String> {
    let has_document_handle = evidence.document_id.is_some() || evidence.filename.is_some();

    match evidence.kind {
        EvidenceKind::Document => {
            if !has_document_handle {
                Some("document evidence must include document_id or filename".to_owned())
            } else {
                None
            }
        }
        EvidenceKind::DocumentSpan => {
            if !has_document_handle || evidence.page.is_none() || evidence.quote.is_none() {
                Some(
                    "document_span evidence must include document_id or filename, page, and quote"
                        .to_owned(),
                )
            } else {
                None
            }
        }
        EvidenceKind::SystemGenerated | EvidenceKind::UserInput => {
            if evidence.origin.is_none() {
                Some("system_generated/user_input evidence must include origin".to_owned())
            } else if has_document_handle || evidence.page.is_some() || evidence.quote.is_some() {
                Some("system_generated/user_input evidence must not include document/page/quote fields".to_owned())
            } else {
                None
            }
        }
    }
}

struct Validator<'a> {
    issues: Vec<ValidationIssue>,
    root: &'a ReportValue,
}

impl<'a> Validator<'a> {
    fn validate_node(&mut self, value: &'a ReportValue, schema_path: &str, actual_path: &str) {
        let Some(rule) = field_rule(schema_path) else {
            self.push_issue(
                ValidationSeverity::Error,
                ValidationIssueKind::InternalSchemaError,
                actual_path,
                schema_path,
                "Missing generated field rule for schema path",
            );
            return;
        };

        if !self.validate_type(value, rule, actual_path) {
            return;
        }

        match rule.schema_type {
            SchemaType::Object => self.validate_object(value, schema_path, actual_path),
            SchemaType::Array => self.validate_array(value, schema_path, actual_path),
            _ => self.validate_leaf(value, rule, actual_path),
        }
    }

    fn validate_object(&mut self, value: &'a ReportValue, schema_path: &str, actual_path: &str) {
        let Some(object) = value.as_object() else {
            return;
        };

        for child_rule in immediate_child_rules(schema_path) {
            let Some(child_name) = terminal_field_name(child_rule.path) else {
                continue;
            };
            let child_actual_path = join_object_path(actual_path, child_name);

            match object.get(child_name) {
                Some(child_value) => {
                    self.validate_dependencies(child_rule, actual_path);
                    self.validate_node(child_value, child_rule.path, &child_actual_path);
                }
                None => {
                    if self.is_rule_required(child_rule, actual_path) {
                        self.push_issue(
                            ValidationSeverity::Error,
                            ValidationIssueKind::MissingRequiredField,
                            &child_actual_path,
                            child_rule.path,
                            "Required field is missing",
                        );
                    }
                }
            }
        }
    }

    fn validate_array(&mut self, value: &'a ReportValue, schema_path: &str, actual_path: &str) {
        let Some(items) = value.as_array() else {
            return;
        };

        let item_rule = immediate_child_rules(schema_path).into_iter().find(|rule| {
            matches!(
                last_schema_segment(rule.path),
                Some(SchemaPathSegment::AnyIndex)
            )
        });

        let Some(item_rule) = item_rule else {
            if !items.is_empty() {
                self.push_issue(
                    ValidationSeverity::Error,
                    ValidationIssueKind::InternalSchemaError,
                    actual_path,
                    schema_path,
                    "Array schema path is missing its generated item rule",
                );
            }
            return;
        };

        for (index, item) in items.iter().enumerate() {
            let child_actual_path = format!("{actual_path}[{index}]");
            self.validate_node(item, item_rule.path, &child_actual_path);
        }
    }

    fn validate_leaf(&mut self, value: &ReportValue, rule: &FieldRule, actual_path: &str) {
        if rule.schema_type == SchemaType::Enum {
            if let Some(text) = value.as_text() {
                if !rule.allowed_values.contains(&text) {
                    self.push_issue(
                        ValidationSeverity::Error,
                        ValidationIssueKind::InvalidEnumValue,
                        actual_path,
                        rule.path,
                        &format!(
                            "Invalid enum value {:?}; expected one of {:?}",
                            text, rule.allowed_values
                        ),
                    );
                }
            } else {
                self.push_issue(
                    ValidationSeverity::Error,
                    ValidationIssueKind::TypeMismatch,
                    actual_path,
                    rule.path,
                    "Enum field must be represented as text",
                );
            }
        }

        if rule.validation.is_some() {
            self.push_issue(
                ValidationSeverity::Warning,
                ValidationIssueKind::ManualReviewRequired,
                actual_path,
                rule.path,
                "Field has a schema validation note that still requires manual or custom validation logic",
            );
        }
    }

    fn validate_type(&mut self, value: &ReportValue, rule: &FieldRule, actual_path: &str) -> bool {
        let is_valid = match rule.schema_type {
            SchemaType::Object => matches!(value.kind(), ValueKind::Object),
            SchemaType::Array => matches!(value.kind(), ValueKind::Array),
            SchemaType::String => matches!(value.kind(), ValueKind::String),
            SchemaType::Boolean => matches!(value.kind(), ValueKind::Bool),
            SchemaType::Number => {
                matches!(value, ReportValue::Number(_))
                    || value
                        .as_text()
                        .is_some_and(|text| text.parse::<f64>().is_ok())
            }
            SchemaType::Date => {
                matches!(value, ReportValue::Date(_))
                    || value.as_text().is_some_and(is_iso_date_like)
            }
            SchemaType::Enum => value.as_text().is_some(),
        };

        if !is_valid {
            self.push_issue(
                ValidationSeverity::Error,
                ValidationIssueKind::TypeMismatch,
                actual_path,
                rule.path,
                &format!(
                    "Type mismatch: expected {}, found {}",
                    schema_type_name(rule.schema_type),
                    value_kind_name(value.kind())
                ),
            );
        }

        is_valid
    }

    fn validate_dependencies(&mut self, rule: &FieldRule, current_actual_path: &str) {
        for conditional in conditional_rules_for(rule.path) {
            if conditional.rule_type != ConditionalRuleType::DependsOn {
                continue;
            }

            for dependency in conditional.depends_on {
                if self
                    .resolve_reference(current_actual_path, dependency)
                    .is_none()
                {
                    self.push_issue(
                        ValidationSeverity::Error,
                        ValidationIssueKind::MissingDependency,
                        current_actual_path,
                        rule.path,
                        &format!("Dependent field {:?} is missing", dependency),
                    );
                }
            }
        }
    }

    fn is_rule_required(&mut self, rule: &FieldRule, current_actual_path: &str) -> bool {
        if rule.required {
            return true;
        }

        let Some(expression) = rule.required_expression else {
            return false;
        };

        match self.evaluate_expression(current_actual_path, expression) {
            Ok(value) => value,
            Err(issue_kind) => {
                self.push_issue(
                    ValidationSeverity::Warning,
                    issue_kind,
                    current_actual_path,
                    rule.path,
                    &format!("Could not evaluate required expression {:?}", expression),
                );
                false
            }
        }
    }

    fn evaluate_expression(
        &self,
        current_actual_path: &str,
        expression: &str,
    ) -> Result<bool, ValidationIssueKind> {
        if let Some((left, right)) = expression.split_once("==") {
            let reference = left.trim();
            let rhs = right.trim();
            let Some(value) = self.resolve_reference(current_actual_path, reference) else {
                return Ok(false);
            };

            if rhs == "true" || rhs == "false" {
                let Some(actual) = value.as_bool() else {
                    return Err(ValidationIssueKind::UnsupportedExpression);
                };
                return Ok(actual == (rhs == "true"));
            }

            let expected = trim_quotes(rhs);
            let Some(actual) = value.as_text() else {
                return Err(ValidationIssueKind::UnsupportedExpression);
            };
            return Ok(actual == expected);
        }

        if let Some((left, right)) = expression.split_once(" in ") {
            let reference = left.trim();
            let raw_set = right.trim();
            let Some(stripped) = raw_set
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
            else {
                return Err(ValidationIssueKind::UnsupportedExpression);
            };
            let Some(value) = self.resolve_reference(current_actual_path, reference) else {
                return Ok(false);
            };
            let Some(actual) = value.as_text() else {
                return Err(ValidationIssueKind::UnsupportedExpression);
            };
            let options = stripped
                .split(',')
                .map(|entry| trim_quotes(entry.trim()))
                .collect::<Vec<_>>();
            return Ok(options.iter().any(|option| actual == *option));
        }

        Err(ValidationIssueKind::UnsupportedExpression)
    }

    fn resolve_reference(
        &self,
        current_actual_path: &str,
        reference: &str,
    ) -> Option<&'a ReportValue> {
        for candidate in reference_candidates(current_actual_path, reference) {
            if let Some(value) = lookup_actual_path(self.root, &candidate) {
                return Some(value);
            }
        }
        None
    }

    fn push_issue(
        &mut self,
        severity: ValidationSeverity,
        kind: ValidationIssueKind,
        path: &str,
        schema_path: &str,
        message: &str,
    ) {
        self.issues.push(ValidationIssue {
            severity,
            kind,
            path: path.to_owned(),
            schema_path: schema_path.to_owned(),
            message: message.to_owned(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SchemaPathSegment<'a> {
    Field(&'a str),
    AnyIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActualPathSegment<'a> {
    Field(&'a str),
    Index(usize),
}

fn immediate_child_rules(parent_path: &str) -> Vec<&'static FieldRule> {
    crate::FIELD_RULES
        .iter()
        .filter(|rule| is_immediate_child(parent_path, rule.path))
        .collect()
}

fn is_immediate_child(parent: &str, child: &str) -> bool {
    let parent_segments = parse_schema_path(parent);
    let child_segments = parse_schema_path(child);
    child_segments.len() == parent_segments.len() + 1
        && child_segments.starts_with(&parent_segments)
}

fn parse_schema_path(path: &str) -> Vec<SchemaPathSegment<'_>> {
    let mut segments = Vec::new();
    for part in path.split('.') {
        if let Some(prefix) = part.strip_suffix("[]") {
            if !prefix.is_empty() {
                segments.push(SchemaPathSegment::Field(prefix));
            }
            segments.push(SchemaPathSegment::AnyIndex);
        } else {
            segments.push(SchemaPathSegment::Field(part));
        }
    }
    segments
}

fn parse_actual_path(path: &str) -> Vec<ActualPathSegment<'_>> {
    let mut segments = Vec::new();
    for part in path.split('.') {
        if let Some(bracket_index) = part.find('[') {
            let field = &part[..bracket_index];
            if !field.is_empty() {
                segments.push(ActualPathSegment::Field(field));
            }

            let mut rest = &part[bracket_index..];
            while let Some(stripped) = rest.strip_prefix('[') {
                let Some(end_index) = stripped.find(']') else {
                    break;
                };
                let number = &stripped[..end_index];
                if let Ok(index) = number.parse::<usize>() {
                    segments.push(ActualPathSegment::Index(index));
                }
                rest = &stripped[end_index + 1..];
            }
        } else {
            segments.push(ActualPathSegment::Field(part));
        }
    }
    segments
}

fn last_schema_segment(path: &str) -> Option<SchemaPathSegment<'_>> {
    parse_schema_path(path).into_iter().last()
}

fn terminal_field_name(path: &str) -> Option<&str> {
    parse_schema_path(path)
        .into_iter()
        .rev()
        .find_map(|segment| match segment {
            SchemaPathSegment::Field(name) => Some(name),
            SchemaPathSegment::AnyIndex => None,
        })
}

fn join_object_path(base: &str, field: &str) -> String {
    if base.is_empty() {
        field.to_owned()
    } else {
        format!("{base}.{field}")
    }
}

fn schema_type_name(schema_type: SchemaType) -> &'static str {
    match schema_type {
        SchemaType::Object => "object",
        SchemaType::Array => "array",
        SchemaType::String => "string",
        SchemaType::Number => "number",
        SchemaType::Date => "date",
        SchemaType::Enum => "enum",
        SchemaType::Boolean => "boolean",
    }
}

fn value_kind_name(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::Null => "null",
        ValueKind::String => "string",
        ValueKind::Bool => "boolean",
        ValueKind::Array => "array",
        ValueKind::Object => "object",
        ValueKind::Number => "number",
        ValueKind::Date => "date",
    }
}

fn is_iso_date_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn trim_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn reference_candidates(current_actual_path: &str, reference: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let parent_path = parent_actual_path(current_actual_path);
    let current_field_name = current_actual_path.rsplit('.').next();

    if reference.starts_with("expense_report.") {
        push_candidate(&mut candidates, reference.to_owned());
    } else {
        push_candidate(&mut candidates, format!("expense_report.{reference}"));
        if let Some(parent_path) = parent_path {
            push_candidate(&mut candidates, format!("{parent_path}.{reference}"));
        }
        if !reference.contains('.') {
            push_candidate(
                &mut candidates,
                format!("{current_actual_path}.{reference}"),
            );
        }
        if let (Some(parent_path), Some(current_field_name)) = (parent_path, current_field_name) {
            if let Some(stripped_reference) =
                reference.strip_prefix(&format!("{current_field_name}."))
            {
                push_candidate(
                    &mut candidates,
                    format!("{current_actual_path}.{stripped_reference}"),
                );
                push_candidate(&mut candidates, format!("{parent_path}.{reference}"));
            }
        }
        for ancestor in ancestor_paths(current_actual_path) {
            push_candidate(&mut candidates, format!("{ancestor}.{reference}"));
            if !reference.contains('.') {
                push_candidate(&mut candidates, format!("{ancestor}.common.{reference}"));
            }
        }
    }

    candidates
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

fn parent_actual_path(path: &str) -> Option<&str> {
    path.rsplit_once('.').map(|(parent, _)| parent)
}

fn ancestor_paths(path: &str) -> Vec<String> {
    let segments = parse_actual_path(path);
    let mut ancestors = Vec::new();

    for end in (1..=segments.len()).rev() {
        ancestors.push(render_actual_segments(&segments[..end]));
    }

    ancestors
}

fn render_actual_segments(segments: &[ActualPathSegment<'_>]) -> String {
    let mut rendered = String::new();
    for segment in segments {
        match segment {
            ActualPathSegment::Field(name) => {
                if !rendered.is_empty() {
                    rendered.push('.');
                }
                rendered.push_str(name);
            }
            ActualPathSegment::Index(index) => {
                rendered.push('[');
                rendered.push_str(&index.to_string());
                rendered.push(']');
            }
        }
    }
    rendered
}

fn lookup_actual_path<'a>(root: &'a ReportValue, path: &str) -> Option<&'a ReportValue> {
    let mut current = root;
    let mut segments = parse_actual_path(path).into_iter();

    if matches!(
        segments.next(),
        Some(ActualPathSegment::Field("expense_report"))
    ) {
        // The root report value itself corresponds to expense_report.
    } else {
        return None;
    }

    for segment in segments {
        match segment {
            ActualPathSegment::Field(name) => {
                current = current.as_object()?.get(name)?;
            }
            ActualPathSegment::Index(index) => {
                current = current.as_array()?.get(index)?;
            }
        }
    }

    Some(current)
}

fn collect_leaf_paths(value: &ReportValue, path: &str) -> Vec<String> {
    match value {
        ReportValue::Null => Vec::new(),
        ReportValue::Object(object) => object
            .iter()
            .flat_map(|(key, value)| collect_leaf_paths(value, &format!("{path}.{key}")))
            .collect(),
        ReportValue::Array(values) => values
            .iter()
            .enumerate()
            .flat_map(|(index, value)| collect_leaf_paths(value, &format!("{path}[{index}]")))
            .collect(),
        ReportValue::String(_)
        | ReportValue::Bool(_)
        | ReportValue::Number(_)
        | ReportValue::Date(_) => vec![path.to_owned()],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::draft::{
        ConfidenceLevel, DraftReport, EvidenceKind, EvidenceReference, FieldMetadata,
    };
    use crate::value::ReportValue;

    fn text(value: &str) -> ReportValue {
        ReportValue::String(value.to_owned())
    }

    fn date(value: &str) -> ReportValue {
        ReportValue::Date(value.to_owned())
    }

    fn number(value: &str) -> ReportValue {
        ReportValue::Number(value.to_owned())
    }

    fn base_report() -> ReportValue {
        ReportValue::object([
            (
                "general_information",
                ReportValue::object([
                    ("category", text("expenses_domestic")),
                    (
                        "payee",
                        ReportValue::object([
                            ("name", text("Olivia")),
                            ("affiliation", text("stanford_student")),
                        ]),
                    ),
                    ("rush_processing", text("no")),
                    ("payment_method", text("electronic")),
                    (
                        "business_purpose",
                        ReportValue::object([
                            ("who", text("Olivia")),
                            ("what", text("ICLR 2025")),
                            ("when", text("2025-04-21 to 2025-04-29")),
                            ("where", text("Singapore")),
                            ("why", text("Presenting accepted paper")),
                            ("key_30char", text("OLIVIAKOICLR2025")),
                        ]),
                    ),
                    ("event_name", text("PPL Domestic Expenses")),
                    ("student_certification", ReportValue::empty_object()),
                    ("authorized_by", text("Olukotun, Oyekunle A.")),
                ]),
            ),
            (
                "transaction_summary",
                ReportValue::object([
                    ("transaction_date", date("2025-04-29")),
                    ("total_usd", number("0.00")),
                ]),
            ),
            (
                "allocation_and_approvers",
                ReportValue::object([("other_beneficiaries", ReportValue::Bool(false))]),
            ),
        ])
    }

    fn document_evidence(filename: &str) -> EvidenceReference {
        EvidenceReference {
            kind: EvidenceKind::Document,
            document_id: None,
            filename: Some(filename.to_owned()),
            page: None,
            quote: None,
            origin: None,
        }
    }

    fn generated_evidence(origin: &str) -> EvidenceReference {
        EvidenceReference {
            kind: EvidenceKind::SystemGenerated,
            document_id: None,
            filename: None,
            page: None,
            quote: None,
            origin: Some(origin.to_owned()),
        }
    }

    fn user_input_evidence(origin: &str) -> EvidenceReference {
        EvidenceReference {
            kind: EvidenceKind::UserInput,
            document_id: None,
            filename: None,
            page: None,
            quote: None,
            origin: Some(origin.to_owned()),
        }
    }

    fn metadata(
        confidence: ConfidenceLevel,
        source_document: &str,
        needs_review: bool,
    ) -> FieldMetadata {
        let evidence = match source_document {
            "system_generated" => vec![generated_evidence(source_document)],
            "fa_input" | "payee_form" | "user_input" => vec![user_input_evidence(source_document)],
            _ => vec![document_evidence(source_document)],
        };

        FieldMetadata {
            confidence,
            evidence,
            needs_review,
            flags: Vec::new(),
            confidence_reason: None,
        }
    }

    fn complete_base_report_metadata() -> BTreeMap<String, FieldMetadata> {
        BTreeMap::from([
            (
                "expense_report.general_information.category".to_owned(),
                metadata(ConfidenceLevel::High, "itinerary.pdf", false),
            ),
            (
                "expense_report.general_information.payee.name".to_owned(),
                metadata(ConfidenceLevel::High, "itinerary.pdf", false),
            ),
            (
                "expense_report.general_information.payee.affiliation".to_owned(),
                metadata(ConfidenceLevel::Medium, "fa_input", true),
            ),
            (
                "expense_report.general_information.rush_processing".to_owned(),
                metadata(ConfidenceLevel::High, "payee_form", false),
            ),
            (
                "expense_report.general_information.payment_method".to_owned(),
                metadata(ConfidenceLevel::High, "system_generated", false),
            ),
            (
                "expense_report.general_information.business_purpose.who".to_owned(),
                metadata(ConfidenceLevel::High, "itinerary.pdf", false),
            ),
            (
                "expense_report.general_information.business_purpose.what".to_owned(),
                metadata(ConfidenceLevel::High, "registration.pdf", false),
            ),
            (
                "expense_report.general_information.business_purpose.when".to_owned(),
                metadata(ConfidenceLevel::High, "itinerary.pdf", false),
            ),
            (
                "expense_report.general_information.business_purpose.where".to_owned(),
                metadata(ConfidenceLevel::High, "itinerary.pdf", false),
            ),
            (
                "expense_report.general_information.business_purpose.why".to_owned(),
                metadata(ConfidenceLevel::Medium, "program.pdf", true),
            ),
            (
                "expense_report.general_information.business_purpose.key_30char".to_owned(),
                metadata(ConfidenceLevel::High, "system_generated", false),
            ),
            (
                "expense_report.general_information.event_name".to_owned(),
                metadata(ConfidenceLevel::High, "system_generated", false),
            ),
            (
                "expense_report.general_information.authorized_by".to_owned(),
                metadata(ConfidenceLevel::High, "payee_form", false),
            ),
            (
                "expense_report.transaction_summary.transaction_date".to_owned(),
                metadata(ConfidenceLevel::High, "itinerary.pdf", false),
            ),
            (
                "expense_report.transaction_summary.total_usd".to_owned(),
                metadata(ConfidenceLevel::High, "system_generated", false),
            ),
            (
                "expense_report.allocation_and_approvers.other_beneficiaries".to_owned(),
                metadata(ConfidenceLevel::High, "payee_form", false),
            ),
        ])
    }

    #[test]
    fn validates_minimal_base_report() {
        let report = validate_expense_report(&base_report());
        assert!(!report.has_errors(), "{report:?}");
    }

    #[test]
    fn flags_conditional_missing_beneficiary_list() {
        let report = ReportValue::object([
            (
                "general_information",
                ReportValue::object([
                    ("category", text("expenses_domestic")),
                    (
                        "payee",
                        ReportValue::object([
                            ("name", text("Olivia")),
                            ("affiliation", text("stanford_student")),
                        ]),
                    ),
                    ("rush_processing", text("no")),
                    ("payment_method", text("electronic")),
                    (
                        "business_purpose",
                        ReportValue::object([
                            ("who", text("Olivia")),
                            ("what", text("ICLR 2025")),
                            ("when", text("2025-04-21 to 2025-04-29")),
                            ("where", text("Singapore")),
                            ("why", text("Presenting accepted paper")),
                            ("key_30char", text("OLIVIAKOICLR2025")),
                        ]),
                    ),
                    ("event_name", text("PPL Domestic Expenses")),
                    ("student_certification", ReportValue::empty_object()),
                    ("authorized_by", text("Olukotun, Oyekunle A.")),
                ]),
            ),
            (
                "transaction_summary",
                ReportValue::object([
                    ("transaction_date", date("2025-04-29")),
                    ("total_usd", number("0.00")),
                ]),
            ),
            (
                "allocation_and_approvers",
                ReportValue::object([("other_beneficiaries", ReportValue::Bool(true))]),
            ),
        ]);

        let validation = validate_expense_report(&report);
        assert!(validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.allocation_and_approvers.beneficiary_list"));
    }

    #[test]
    fn flags_foreign_transaction_currency_requirements() {
        let report = ReportValue::object([
            (
                "general_information",
                ReportValue::object([
                    ("category", text("expenses_foreign")),
                    (
                        "payee",
                        ReportValue::object([
                            ("name", text("Olivia")),
                            ("affiliation", text("stanford_student")),
                        ]),
                    ),
                    ("rush_processing", text("no")),
                    ("payment_method", text("electronic")),
                    (
                        "business_purpose",
                        ReportValue::object([
                            ("who", text("Olivia")),
                            ("what", text("ICLR 2025")),
                            ("when", text("2025-04-21 to 2025-04-29")),
                            ("where", text("Singapore")),
                            ("why", text("Presenting accepted paper")),
                            ("key_30char", text("OLIVIAKOICLR2025")),
                        ]),
                    ),
                    ("event_name", text("PPL Foreign Expenses")),
                    ("student_certification", ReportValue::empty_object()),
                    ("authorized_by", text("Olukotun, Oyekunle A.")),
                ]),
            ),
            (
                "transaction_summary",
                ReportValue::object([
                    ("transaction_date", date("2025-04-29")),
                    ("total_usd", number("77.34")),
                ]),
            ),
            (
                "transaction_lines",
                ReportValue::array([ReportValue::object([
                    (
                        "common",
                        ReportValue::object([
                            ("date", date("2025-04-29")),
                            ("line_amount_usd", number("77.34")),
                            ("expense_type", text("ground_transportation_foreign")),
                            ("remarks", text("Taxi from hotel to venue")),
                            ("country_of_activity", text("Singapore")),
                            ("foreign_activity_type", text("conference")),
                            (
                                "source_document",
                                ReportValue::object([
                                    ("filename", text("taxi.png")),
                                    ("document_type", text("receipt")),
                                ]),
                            ),
                        ]),
                    ),
                    (
                        "ground_transport_details",
                        ReportValue::object([
                            ("origin", text("Hotel")),
                            ("destination", text("Venue")),
                            ("service_provider", text("Taxi")),
                            ("missing_receipt", ReportValue::Bool(false)),
                        ]),
                    ),
                ])]),
            ),
            (
                "allocation_and_approvers",
                ReportValue::object([("other_beneficiaries", ReportValue::Bool(false))]),
            ),
        ]);

        let validation = validate_expense_report(&report);
        assert!(validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.transaction_lines[0].common.original_currency"));
        assert!(validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.transaction_lines[0].common.original_amount"));
        assert!(validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.transaction_lines[0].common.exchange_rate"));
    }

    #[test]
    fn draft_validation_requires_metadata_for_present_leaf_fields() {
        let draft = DraftReport {
            report: base_report(),
            metadata: BTreeMap::new(),
        };

        let validation = validate_draft_report(&draft);
        assert!(validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingFieldMetadata
            && issue.path == "expense_report.general_information.category"));
    }

    #[test]
    fn draft_validation_accepts_complete_metadata_and_flags_low_confidence_review_gaps() {
        let mut metadata = complete_base_report_metadata();
        metadata.insert(
            "expense_report.general_information.event_name".to_owned(),
            FieldMetadata {
                confidence: ConfidenceLevel::Low,
                evidence: vec![generated_evidence("system_generated")],
                needs_review: false,
                flags: Vec::new(),
                confidence_reason: None,
            },
        );

        let draft = DraftReport {
            report: base_report(),
            metadata,
        };

        let validation = validate_draft_report(&draft);
        assert!(!validation
            .issues
            .iter()
            .any(|issue| issue.kind == ValidationIssueKind::MissingFieldMetadata));
        assert!(validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::LowConfidenceWithoutReview
            && issue.path == "expense_report.general_information.event_name"));
    }

    #[test]
    fn draft_validation_flags_missing_evidence_references() {
        let mut metadata = complete_base_report_metadata();
        metadata.insert(
            "expense_report.general_information.event_name".to_owned(),
            FieldMetadata {
                confidence: ConfidenceLevel::High,
                evidence: Vec::new(),
                needs_review: false,
                flags: Vec::new(),
                confidence_reason: None,
            },
        );

        let draft = DraftReport {
            report: base_report(),
            metadata,
        };

        let validation = validate_draft_report(&draft);
        assert!(validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingEvidenceReference
            && issue.path == "expense_report.general_information.event_name"));
    }
}
