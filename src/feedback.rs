use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::draft::{ConfidenceLevel, DraftReport, EvidenceKind, EvidenceReference, FieldMetadata};
use crate::parse::ReportFormat;
use crate::readiness::{summarize_validation_readiness, ReadinessIssue, ReadinessIssueClass};
use crate::validation_rules::{field_rule, SourceTier};
use crate::validator::ValidationReport;
use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionOperation {
    Added,
    Updated,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackCategory {
    OcrError,
    WrongDocumentClassification,
    WrongExpenseTypeClassification,
    WrongCrossDocumentMerge,
    MissingRequiredField,
    WrongDerivedField,
    StanfordPolicyMismatch,
    StanfordSiteWorkflowMismatch,
    UnclearOrUndocumentedRule,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    Accepted,
    Returned,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionFieldFeedback {
    pub path: Option<String>,
    pub message: String,
    pub category: Option<FeedbackCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionFeedback {
    pub status: SubmissionStatus,
    pub message: Option<String>,
    pub category: Option<FeedbackCategory>,
    pub returned_fields: Vec<SubmissionFieldFeedback>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionAnnotation {
    pub path: String,
    pub reason: FeedbackCategory,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackSummary {
    pub change_count: usize,
    pub added_count: usize,
    pub updated_count: usize,
    pub cleared_count: usize,
    pub change_with_preexisting_issue_count: usize,
    pub change_with_confirmed_reason_count: usize,
    pub submission_status: Option<SubmissionStatus>,
    pub returned_field_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldCorrection {
    pub path: String,
    pub schema_path: String,
    pub label: String,
    pub operation: CorrectionOperation,
    pub original_value: Option<String>,
    pub corrected_value: Option<String>,
    pub schema_source: Option<String>,
    pub original_confidence: Option<ConfidenceLevel>,
    pub original_needs_review: Option<bool>,
    pub original_evidence: Vec<EvidenceReference>,
    pub corrected_evidence: Vec<EvidenceReference>,
    pub preexisting_issue: Option<ReadinessIssueClass>,
    pub preexisting_issue_message: Option<String>,
    pub site_feedback_messages: Vec<String>,
    pub suggested_reason: FeedbackCategory,
    pub confirmed_reason: Option<FeedbackCategory>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackCapture {
    pub summary: FeedbackSummary,
    pub field_changes: Vec<FieldCorrection>,
    pub submission_feedback: Option<SubmissionFeedback>,
}

#[derive(Debug)]
pub enum FeedbackError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
    UnsupportedFormat(String),
    InvalidStructure(String),
}

impl fmt::Display for FeedbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON parse/render error: {err}"),
            Self::Yaml(err) => write!(f, "YAML parse/render error: {err}"),
            Self::UnsupportedFormat(ext) => {
                write!(
                    f,
                    "unsupported feedback format {ext:?}; expected .json, .yaml, or .yml"
                )
            }
            Self::InvalidStructure(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for FeedbackError {}

impl From<std::io::Error> for FeedbackError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for FeedbackError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<serde_yaml::Error> for FeedbackError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Yaml(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum CorrectionAnnotationDocument {
    Wrapped {
        corrections: Vec<CorrectionAnnotation>,
    },
    Bare(Vec<CorrectionAnnotation>),
}

pub fn capture_feedback(
    original: &DraftReport,
    corrected: &DraftReport,
    original_validation: Option<&ValidationReport>,
    annotations: &[CorrectionAnnotation],
    submission_feedback: Option<SubmissionFeedback>,
) -> FeedbackCapture {
    let mut original_leaves = BTreeMap::new();
    let mut corrected_leaves = BTreeMap::new();
    collect_leaf_values(&original.report, "expense_report", &mut original_leaves);
    collect_leaf_values(&corrected.report, "expense_report", &mut corrected_leaves);

    let readiness_issues = original_validation
        .map(summarize_validation_readiness)
        .map(|report| report.issues)
        .unwrap_or_default();
    let annotation_map = annotations
        .iter()
        .map(|annotation| (annotation.path.clone(), annotation))
        .collect::<BTreeMap<_, _>>();

    let mut changed_paths = BTreeSet::new();
    changed_paths.extend(original_leaves.keys().cloned());
    changed_paths.extend(corrected_leaves.keys().cloned());

    let mut field_changes = changed_paths
        .into_iter()
        .filter_map(|path| {
            build_field_correction(
                &path,
                original_leaves.get(&path).copied(),
                corrected_leaves.get(&path).copied(),
                original,
                corrected,
                &readiness_issues,
                &annotation_map,
                submission_feedback.as_ref(),
            )
        })
        .collect::<Vec<_>>();

    field_changes.sort_by(|lhs, rhs| lhs.path.cmp(&rhs.path));

    let summary = FeedbackSummary {
        change_count: field_changes.len(),
        added_count: field_changes
            .iter()
            .filter(|change| change.operation == CorrectionOperation::Added)
            .count(),
        updated_count: field_changes
            .iter()
            .filter(|change| change.operation == CorrectionOperation::Updated)
            .count(),
        cleared_count: field_changes
            .iter()
            .filter(|change| change.operation == CorrectionOperation::Cleared)
            .count(),
        change_with_preexisting_issue_count: field_changes
            .iter()
            .filter(|change| change.preexisting_issue.is_some())
            .count(),
        change_with_confirmed_reason_count: field_changes
            .iter()
            .filter(|change| change.confirmed_reason.is_some())
            .count(),
        submission_status: submission_feedback.as_ref().map(|feedback| feedback.status),
        returned_field_count: submission_feedback
            .as_ref()
            .map_or(0, |feedback| feedback.returned_fields.len()),
    };

    FeedbackCapture {
        summary,
        field_changes,
        submission_feedback,
    }
}

pub fn render_feedback_capture_json_pretty(
    capture: &FeedbackCapture,
) -> Result<String, FeedbackError> {
    Ok(serde_json::to_string_pretty(capture)?)
}

pub fn render_feedback_capture_markdown(capture: &FeedbackCapture) -> String {
    let mut lines = vec!["# Feedback Capture".to_owned(), String::new()];

    lines.push("## Summary".to_owned());
    lines.push(format!("- Changes: {}", capture.summary.change_count));
    lines.push(format!(
        "- Operations: {} added, {} updated, {} cleared",
        capture.summary.added_count, capture.summary.updated_count, capture.summary.cleared_count
    ));
    lines.push(format!(
        "- With preexisting issue: {}",
        capture.summary.change_with_preexisting_issue_count
    ));
    lines.push(format!(
        "- With confirmed reason: {}",
        capture.summary.change_with_confirmed_reason_count
    ));
    if let Some(status) = capture.summary.submission_status {
        lines.push(format!(
            "- Submission status: {}",
            submission_status_name(status)
        ));
    }
    if capture.summary.returned_field_count > 0 {
        lines.push(format!(
            "- Returned field count: {}",
            capture.summary.returned_field_count
        ));
    }
    lines.push(String::new());

    lines.push("## Field Changes".to_owned());
    if capture.field_changes.is_empty() {
        lines.push("- None".to_owned());
    } else {
        for change in &capture.field_changes {
            lines.push(format!(
                "- {} [{}] {} -> {}",
                change.label,
                correction_operation_name(change.operation),
                change.original_value.as_deref().unwrap_or("[missing]"),
                change.corrected_value.as_deref().unwrap_or("[missing]")
            ));
            lines.push(format!("  Path: {}", change.path));
            lines.push(format!(
                "  Reason: {}{}",
                feedback_category_name(change.confirmed_reason.unwrap_or(change.suggested_reason)),
                change
                    .confirmed_reason
                    .is_none()
                    .then_some(" (suggested)")
                    .unwrap_or("")
            ));
            if let Some(issue) = change.preexisting_issue {
                lines.push(format!(
                    "  Preexisting issue: {}",
                    readiness_issue_class_name(issue)
                ));
            }
            if let Some(note) = change.note.as_deref() {
                lines.push(format!("  Note: {note}"));
            }
            for message in &change.site_feedback_messages {
                lines.push(format!("  Site feedback: {message}"));
            }
        }
    }
    lines.push(String::new());

    lines.push("## Submission Feedback".to_owned());
    match &capture.submission_feedback {
        Some(feedback) => {
            lines.push(format!(
                "- Status: {}",
                submission_status_name(feedback.status)
            ));
            if let Some(message) = feedback.message.as_deref() {
                lines.push(format!("- Message: {message}"));
            }
            if let Some(category) = feedback.category {
                lines.push(format!("- Category: {}", feedback_category_name(category)));
            }
            if feedback.returned_fields.is_empty() {
                lines.push("- Returned fields: none".to_owned());
            } else {
                lines.push("- Returned fields:".to_owned());
                for field in &feedback.returned_fields {
                    let path = field.path.as_deref().unwrap_or("[unspecified]");
                    let suffix = field
                        .category
                        .map(|category| format!(" ({})", feedback_category_name(category)))
                        .unwrap_or_default();
                    lines.push(format!("  - {path}: {}{suffix}", field.message));
                }
            }
        }
        None => lines.push("- None".to_owned()),
    }

    lines.join("\n")
}

pub fn parse_correction_annotations_path(
    path: impl AsRef<Path>,
) -> Result<Vec<CorrectionAnnotation>, FeedbackError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;
    match detect_feedback_format(path) {
        Some(ReportFormat::Json) => parse_correction_annotations_str(&contents, ReportFormat::Json),
        Some(ReportFormat::Yaml) => parse_correction_annotations_str(&contents, ReportFormat::Yaml),
        None => parse_correction_annotations_str(&contents, ReportFormat::Json)
            .or_else(|_| parse_correction_annotations_str(&contents, ReportFormat::Yaml)),
    }
}

pub fn parse_submission_feedback_path(
    path: impl AsRef<Path>,
) -> Result<SubmissionFeedback, FeedbackError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;
    match detect_feedback_format(path) {
        Some(ReportFormat::Json) => parse_submission_feedback_str(&contents, ReportFormat::Json),
        Some(ReportFormat::Yaml) => parse_submission_feedback_str(&contents, ReportFormat::Yaml),
        None => parse_submission_feedback_str(&contents, ReportFormat::Json)
            .or_else(|_| parse_submission_feedback_str(&contents, ReportFormat::Yaml)),
    }
}

pub fn parse_correction_annotations_str(
    input: &str,
    format: ReportFormat,
) -> Result<Vec<CorrectionAnnotation>, FeedbackError> {
    let document = match format {
        ReportFormat::Json => serde_json::from_str::<CorrectionAnnotationDocument>(input)?,
        ReportFormat::Yaml => serde_yaml::from_str::<CorrectionAnnotationDocument>(input)?,
    };

    let corrections = match document {
        CorrectionAnnotationDocument::Wrapped { corrections } => corrections,
        CorrectionAnnotationDocument::Bare(corrections) => corrections,
    };
    Ok(corrections)
}

pub fn parse_submission_feedback_str(
    input: &str,
    format: ReportFormat,
) -> Result<SubmissionFeedback, FeedbackError> {
    match format {
        ReportFormat::Json => Ok(serde_json::from_str(input)?),
        ReportFormat::Yaml => Ok(serde_yaml::from_str(input)?),
    }
}

pub(crate) fn apply_user_input_override(
    draft: &mut DraftReport,
    path: &str,
    value: ReportValue,
    origin: &str,
) -> Result<(), String> {
    let normalized_path = path
        .strip_prefix("expense_report.")
        .or_else(|| (path == "expense_report").then_some(""))
        .ok_or_else(|| format!("override path must start with expense_report: {path}"))?;

    set_value_at_path(&mut draft.report, normalized_path, value.clone())?;
    clear_metadata_subtree(&mut draft.metadata, path);
    insert_leaf_metadata(
        &mut draft.metadata,
        path,
        &value,
        &FieldMetadata {
            confidence: ConfidenceLevel::High,
            evidence: vec![EvidenceReference {
                kind: EvidenceKind::UserInput,
                document_id: None,
                filename: None,
                page: None,
                quote: None,
                origin: Some(origin.to_owned()),
            }],
            needs_review: false,
            flags: Vec::new(),
        },
    );
    Ok(())
}

pub(crate) fn apply_system_generated_override(
    draft: &mut DraftReport,
    path: &str,
    value: ReportValue,
    metadata: FieldMetadata,
) -> Result<(), String> {
    let normalized_path = path
        .strip_prefix("expense_report.")
        .or_else(|| (path == "expense_report").then_some(""))
        .ok_or_else(|| format!("override path must start with expense_report: {path}"))?;

    set_value_at_path(&mut draft.report, normalized_path, value.clone())?;
    clear_metadata_subtree(&mut draft.metadata, path);
    insert_leaf_metadata(&mut draft.metadata, path, &value, &metadata);
    Ok(())
}

pub(crate) fn clear_draft_field(draft: &mut DraftReport, path: &str) -> Result<(), String> {
    let normalized_path = path
        .strip_prefix("expense_report.")
        .or_else(|| (path == "expense_report").then_some(""))
        .ok_or_else(|| format!("override path must start with expense_report: {path}"))?;

    remove_value_at_path(&mut draft.report, normalized_path)?;
    clear_metadata_subtree(&mut draft.metadata, path);
    Ok(())
}

fn build_field_correction(
    path: &str,
    original_value: Option<&ReportValue>,
    corrected_value: Option<&ReportValue>,
    original: &DraftReport,
    corrected: &DraftReport,
    readiness_issues: &[ReadinessIssue],
    annotation_map: &BTreeMap<String, &CorrectionAnnotation>,
    submission_feedback: Option<&SubmissionFeedback>,
) -> Option<FieldCorrection> {
    if original_value == corrected_value {
        return None;
    }

    let operation = match (original_value, corrected_value) {
        (None, Some(_)) => CorrectionOperation::Added,
        (Some(_), None) => CorrectionOperation::Cleared,
        (Some(_), Some(_)) => CorrectionOperation::Updated,
        (None, None) => return None,
    };
    let schema_path = wildcard_schema_path(path);
    let annotation = annotation_map.get(path).copied();
    let issue = matching_issue(path, readiness_issues);
    let site_messages = matching_site_feedback_messages(path, submission_feedback);
    let original_metadata = original.metadata.get(path);
    let corrected_metadata = corrected.metadata.get(path);

    let suggested_reason =
        suggest_feedback_category(operation, &schema_path, original_metadata, issue);

    Some(FieldCorrection {
        path: path.to_owned(),
        schema_path: schema_path.clone(),
        label: humanize_path_tail(path),
        operation,
        original_value: original_value.and_then(report_value_to_string),
        corrected_value: corrected_value.and_then(report_value_to_string),
        schema_source: field_rule(&schema_path)
            .and_then(|rule| rule.effective_source)
            .map(source_tier_name)
            .map(str::to_owned),
        original_confidence: original_metadata.map(|metadata| metadata.confidence),
        original_needs_review: original_metadata.map(|metadata| metadata.needs_review),
        original_evidence: original_metadata
            .map(|metadata| metadata.evidence.clone())
            .unwrap_or_default(),
        corrected_evidence: corrected_metadata
            .map(|metadata| metadata.evidence.clone())
            .unwrap_or_default(),
        preexisting_issue: issue.map(|issue| issue.class),
        preexisting_issue_message: issue.map(|issue| issue.message.clone()),
        site_feedback_messages: site_messages,
        suggested_reason,
        confirmed_reason: annotation.map(|annotation| annotation.reason),
        note: annotation.and_then(|annotation| annotation.note.clone()),
    })
}

fn collect_leaf_values<'a>(
    value: &'a ReportValue,
    path: &str,
    out: &mut BTreeMap<String, &'a ReportValue>,
) {
    match value {
        ReportValue::Object(object) => {
            for (key, value) in object {
                collect_leaf_values(value, &format!("{path}.{key}"), out);
            }
        }
        ReportValue::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_leaf_values(value, &format!("{path}[{index}]"), out);
            }
        }
        _ => {
            out.insert(path.to_owned(), value);
        }
    }
}

fn matching_issue<'a>(path: &str, issues: &'a [ReadinessIssue]) -> Option<&'a ReadinessIssue> {
    ancestor_paths(path)
        .into_iter()
        .find_map(|candidate| issues.iter().find(|issue| issue.path == candidate))
}

fn matching_site_feedback_messages(
    path: &str,
    feedback: Option<&SubmissionFeedback>,
) -> Vec<String> {
    let Some(feedback) = feedback else {
        return Vec::new();
    };

    let candidates = ancestor_paths(path);
    feedback
        .returned_fields
        .iter()
        .filter(|field| {
            field
                .path
                .as_deref()
                .is_some_and(|candidate| candidates.iter().any(|value| value == candidate))
        })
        .map(|field| field.message.clone())
        .collect()
}

fn ancestor_paths(path: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut current = path.to_owned();
    ancestors.push(current.clone());
    while let Some(parent) = parent_path(&current) {
        current = parent;
        ancestors.push(current.clone());
    }
    ancestors
}

fn parent_path(path: &str) -> Option<String> {
    if path == "expense_report" {
        return None;
    }

    if path.ends_with(']') {
        let bracket_index = path.rfind('[')?;
        return Some(path[..bracket_index].to_owned());
    }

    path.rfind('.').map(|index| path[..index].to_owned())
}

fn suggest_feedback_category(
    operation: CorrectionOperation,
    schema_path: &str,
    original_metadata: Option<&FieldMetadata>,
    issue: Option<&ReadinessIssue>,
) -> FeedbackCategory {
    if operation == CorrectionOperation::Added {
        return FeedbackCategory::MissingRequiredField;
    }

    let tail = schema_path.rsplit('.').next().unwrap_or(schema_path);
    if tail == "document_type" {
        return FeedbackCategory::WrongDocumentClassification;
    }
    if tail == "expense_type" {
        return FeedbackCategory::WrongExpenseTypeClassification;
    }
    if field_rule(schema_path)
        .and_then(|rule| rule.effective_source)
        .is_some_and(|source| source == SourceTier::T2)
    {
        return FeedbackCategory::WrongDerivedField;
    }
    if original_metadata.is_some_and(|metadata| {
        metadata.evidence.iter().any(|evidence| {
            evidence.kind == EvidenceKind::SystemGenerated
                && evidence
                    .origin
                    .as_deref()
                    .is_some_and(|origin| origin.starts_with("bundle_synthesis."))
        })
    }) {
        return FeedbackCategory::WrongCrossDocumentMerge;
    }
    if issue.is_some_and(|issue| issue.class == ReadinessIssueClass::ManualReview) {
        return FeedbackCategory::UnclearOrUndocumentedRule;
    }

    FeedbackCategory::Other
}

fn report_value_to_string(value: &ReportValue) -> Option<String> {
    match value {
        ReportValue::Null => None,
        ReportValue::String(value) | ReportValue::Number(value) | ReportValue::Date(value) => {
            Some(value.clone())
        }
        ReportValue::Bool(value) => Some(value.to_string()),
        ReportValue::Array(_) | ReportValue::Object(_) => None,
    }
}

fn humanize_path_tail(path: &str) -> String {
    let tail = path.rsplit('.').next().unwrap_or(path);
    let tail = tail.rsplit(']').next().unwrap_or(tail);
    tail.replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn detect_feedback_format(path: &Path) -> Option<ReportFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "json" => Some(ReportFormat::Json),
        "yaml" | "yml" => Some(ReportFormat::Yaml),
        _ => None,
    }
}

fn source_tier_name(source: SourceTier) -> &'static str {
    match source {
        SourceTier::T1 => "t1",
        SourceTier::T2 => "t2",
        SourceTier::T3 => "t3",
    }
}

fn correction_operation_name(operation: CorrectionOperation) -> &'static str {
    match operation {
        CorrectionOperation::Added => "added",
        CorrectionOperation::Updated => "updated",
        CorrectionOperation::Cleared => "cleared",
    }
}

fn feedback_category_name(category: FeedbackCategory) -> &'static str {
    match category {
        FeedbackCategory::OcrError => "ocr_error",
        FeedbackCategory::WrongDocumentClassification => "wrong_document_classification",
        FeedbackCategory::WrongExpenseTypeClassification => "wrong_expense_type_classification",
        FeedbackCategory::WrongCrossDocumentMerge => "wrong_cross_document_merge",
        FeedbackCategory::MissingRequiredField => "missing_required_field",
        FeedbackCategory::WrongDerivedField => "wrong_derived_field",
        FeedbackCategory::StanfordPolicyMismatch => "stanford_policy_mismatch",
        FeedbackCategory::StanfordSiteWorkflowMismatch => "stanford_site_workflow_mismatch",
        FeedbackCategory::UnclearOrUndocumentedRule => "unclear_or_undocumented_rule",
        FeedbackCategory::Other => "other",
    }
}

fn readiness_issue_class_name(class: ReadinessIssueClass) -> &'static str {
    match class {
        ReadinessIssueClass::AutomationGap => "automation_gap",
        ReadinessIssueClass::UserInputRequired => "user_input_required",
        ReadinessIssueClass::ManualReview => "manual_review",
        ReadinessIssueClass::OtherWarning => "other_warning",
    }
}

fn submission_status_name(status: SubmissionStatus) -> &'static str {
    match status {
        SubmissionStatus::Accepted => "accepted",
        SubmissionStatus::Returned => "returned",
        SubmissionStatus::Rejected => "rejected",
    }
}

fn wildcard_schema_path(path: &str) -> String {
    let mut output = String::new();
    let mut chars = path.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '[' {
            while chars
                .next_if(|candidate| candidate.is_ascii_digit())
                .is_some()
            {}
            if chars.next_if_eq(&']').is_some() {
                output.push_str("[]");
                continue;
            }
            output.push('[');
            continue;
        }
        output.push(ch);
    }
    output
}

fn clear_metadata_subtree(metadata: &mut BTreeMap<String, FieldMetadata>, path: &str) {
    metadata.retain(|candidate, _| !is_same_or_child_path(candidate, path));
}

fn insert_leaf_metadata(
    metadata: &mut BTreeMap<String, FieldMetadata>,
    path: &str,
    value: &ReportValue,
    template: &FieldMetadata,
) {
    match value {
        ReportValue::Object(object) => {
            for (key, child) in object {
                insert_leaf_metadata(metadata, &format!("{path}.{key}"), child, template);
            }
        }
        ReportValue::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                insert_leaf_metadata(metadata, &format!("{path}[{index}]"), child, template);
            }
        }
        _ => {
            metadata.insert(path.to_owned(), template.clone());
        }
    }
}

fn is_same_or_child_path(candidate: &str, prefix: &str) -> bool {
    candidate == prefix
        || candidate
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
}

fn set_value_at_path(root: &mut ReportValue, path: &str, value: ReportValue) -> Result<(), String> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }

    let segments = parse_path_segments(path)?;
    set_value_segments(root, &segments, value)
}

fn remove_value_at_path(root: &mut ReportValue, path: &str) -> Result<(), String> {
    if path.is_empty() {
        *root = ReportValue::Null;
        return Ok(());
    }

    let segments = parse_path_segments(path)?;
    remove_value_segments(root, &segments)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Key(String),
    Index(usize),
}

fn parse_path_segments(path: &str) -> Result<Vec<PathSegment>, String> {
    let mut segments = Vec::new();
    for part in path.split('.') {
        let mut remainder = part;
        loop {
            if let Some(index_start) = remainder.find('[') {
                let key = &remainder[..index_start];
                if !key.is_empty() {
                    segments.push(PathSegment::Key(key.to_owned()));
                }
                let tail = &remainder[index_start + 1..];
                let index_end = tail
                    .find(']')
                    .ok_or_else(|| format!("invalid path segment: {part}"))?;
                let index = tail[..index_end]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid array index in path: {part}"))?;
                segments.push(PathSegment::Index(index));
                remainder = &tail[index_end + 1..];
                if remainder.is_empty() {
                    break;
                }
            } else {
                if !remainder.is_empty() {
                    segments.push(PathSegment::Key(remainder.to_owned()));
                }
                break;
            }
        }
    }
    Ok(segments)
}

fn set_value_segments(
    current: &mut ReportValue,
    segments: &[PathSegment],
    value: ReportValue,
) -> Result<(), String> {
    if segments.is_empty() {
        *current = value;
        return Ok(());
    }

    match &segments[0] {
        PathSegment::Key(key) => {
            let next_container = if matches!(segments.get(1), Some(PathSegment::Index(_))) {
                ReportValue::Array(Vec::new())
            } else {
                ReportValue::Object(BTreeMap::new())
            };
            let object = match current {
                ReportValue::Object(object) => object,
                ReportValue::Null => {
                    *current = ReportValue::Object(BTreeMap::new());
                    current
                        .as_object_mut()
                        .ok_or_else(|| "failed to create object".to_owned())?
                }
                other => {
                    return Err(format!(
                        "expected object while setting key {key}, found {:?}",
                        other.kind()
                    ))
                }
            };
            if segments.len() == 1 {
                object.insert(key.clone(), value);
                return Ok(());
            }
            let child = object.entry(key.clone()).or_insert(next_container);
            set_value_segments(child, &segments[1..], value)
        }
        PathSegment::Index(index) => {
            let next_container = if matches!(segments.get(1), Some(PathSegment::Index(_))) {
                ReportValue::Array(Vec::new())
            } else {
                ReportValue::Object(BTreeMap::new())
            };
            let array = match current {
                ReportValue::Array(array) => array,
                ReportValue::Null => {
                    *current = ReportValue::Array(Vec::new());
                    current
                        .as_array_mut()
                        .ok_or_else(|| "failed to create array".to_owned())?
                }
                other => {
                    return Err(format!(
                        "expected array while setting index {index}, found {:?}",
                        other.kind()
                    ))
                }
            };
            while array.len() <= *index {
                array.push(ReportValue::Null);
            }
            if segments.len() == 1 {
                array[*index] = value;
                return Ok(());
            }
            if matches!(array[*index], ReportValue::Null) {
                array[*index] = next_container;
            }
            set_value_segments(&mut array[*index], &segments[1..], value)
        }
    }
}

fn remove_value_segments(current: &mut ReportValue, segments: &[PathSegment]) -> Result<(), String> {
    if segments.is_empty() {
        *current = ReportValue::Null;
        return Ok(());
    }

    match &segments[0] {
        PathSegment::Key(key) => {
            let object = match current {
                ReportValue::Object(object) => object,
                ReportValue::Null => return Ok(()),
                other => {
                    return Err(format!(
                        "expected object while clearing key {key}, found {:?}",
                        other.kind()
                    ))
                }
            };

            if segments.len() == 1 {
                object.remove(key);
                return Ok(());
            }

            if let Some(child) = object.get_mut(key) {
                remove_value_segments(child, &segments[1..])?;
                if matches!(child, ReportValue::Object(map) if map.is_empty())
                    || matches!(child, ReportValue::Array(values) if values.iter().all(|value| matches!(value, ReportValue::Null)))
                    || matches!(child, ReportValue::Null)
                {
                    object.remove(key);
                }
            }
            Ok(())
        }
        PathSegment::Index(index) => {
            let array = match current {
                ReportValue::Array(array) => array,
                ReportValue::Null => return Ok(()),
                other => {
                    return Err(format!(
                        "expected array while clearing index {index}, found {:?}",
                        other.kind()
                    ))
                }
            };

            if *index >= array.len() {
                return Ok(());
            }

            if segments.len() == 1 {
                array[*index] = ReportValue::Null;
            } else {
                remove_value_segments(&mut array[*index], &segments[1..])?;
            }

            while array.last().is_some_and(|value| matches!(value, ReportValue::Null)) {
                array.pop();
            }
            Ok(())
        }
    }
}

trait MutableReportValueExt {
    fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, ReportValue>>;
    fn as_array_mut(&mut self) -> Option<&mut Vec<ReportValue>>;
}

impl MutableReportValueExt for ReportValue {
    fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, ReportValue>> {
        match self {
            ReportValue::Object(value) => Some(value),
            _ => None,
        }
    }

    fn as_array_mut(&mut self) -> Option<&mut Vec<ReportValue>> {
        match self {
            ReportValue::Array(value) => Some(value),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_user_input_override, capture_feedback, parse_correction_annotations_str,
        parse_submission_feedback_str, render_feedback_capture_markdown, CorrectionAnnotation,
        CorrectionOperation, FeedbackCategory, SubmissionStatus,
    };
    use crate::bundle_synthesis::{synthesize_bundle_projection_with_fx, StaticFxRateProvider};
    use crate::parse::ReportFormat;
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};
    use crate::value::ReportValue;

    fn synthetic_projection() -> crate::BundleProjectionResult {
        let documents = generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect::<Vec<_>>();
        let provider = StaticFxRateProvider::demo();
        synthesize_bundle_projection_with_fx(&documents, &provider)
    }

    #[test]
    fn captures_added_updated_and_issue_backed_changes() {
        let projection = synthetic_projection();
        let mut corrected = projection.draft.clone();

        apply_user_input_override(
            &mut corrected,
            "expense_report.general_information.payee.affiliation",
            ReportValue::from("stanford_staff"),
            "fa.override.affiliation",
        )
        .expect("override should apply");
        apply_user_input_override(
            &mut corrected,
            "expense_report.general_information.authorized_by",
            ReportValue::from("Dana Torres"),
            "fa.override.authorized_by",
        )
        .expect("override should apply");
        apply_user_input_override(
            &mut corrected,
            "expense_report.general_information.event_name",
            ReportValue::from("Singapore Research Travel"),
            "fa.override.event_name",
        )
        .expect("override should apply");
        apply_user_input_override(
            &mut corrected,
            "expense_report.transaction_lines[2].meal_details.attendees",
            ReportValue::array([
                ReportValue::object([
                    ("name", ReportValue::from("Olivia Park")),
                    ("affiliation", ReportValue::from("Stanford staff")),
                ]),
                ReportValue::object([
                    ("name", ReportValue::from("Collaborator A")),
                    ("affiliation", ReportValue::from("External collaborator")),
                ]),
            ]),
            "fa.override.meal_attendees",
        )
        .expect("override should apply");

        let capture = capture_feedback(
            &projection.draft,
            &corrected,
            Some(&projection.validation),
            &[CorrectionAnnotation {
                path: "expense_report.general_information.payee.affiliation".to_owned(),
                reason: FeedbackCategory::MissingRequiredField,
                note: Some("FA confirmed employee affiliation".to_owned()),
            }],
            None,
        );

        assert_eq!(capture.summary.added_count, 6);
        assert_eq!(capture.summary.updated_count, 1);
        assert!(capture.field_changes.iter().any(|change| change.path
            == "expense_report.general_information.payee.affiliation"
            && change.operation == CorrectionOperation::Added
            && change.preexisting_issue.is_some()
            && change.confirmed_reason == Some(FeedbackCategory::MissingRequiredField)));
        assert!(capture.field_changes.iter().any(|change| change.path
            == "expense_report.general_information.event_name"
            && change.operation == CorrectionOperation::Updated
            && change.suggested_reason == FeedbackCategory::WrongCrossDocumentMerge));
        assert!(capture.field_changes.iter().any(|change| change.path
            == "expense_report.transaction_lines[2].meal_details.attendees[0].name"
            && change.preexisting_issue.is_some()));
    }

    #[test]
    fn markdown_render_includes_submission_status_and_suggested_reason() {
        let projection = synthetic_projection();
        let mut corrected = projection.draft.clone();
        apply_user_input_override(
            &mut corrected,
            "expense_report.general_information.authorized_by",
            ReportValue::from("Dana Torres"),
            "fa.override.authorized_by",
        )
        .expect("override should apply");

        let capture = capture_feedback(
            &projection.draft,
            &corrected,
            Some(&projection.validation),
            &[],
            Some(super::SubmissionFeedback {
                status: SubmissionStatus::Returned,
                message: Some("Oracle returned one field".to_owned()),
                category: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                returned_fields: vec![super::SubmissionFieldFeedback {
                    path: Some("expense_report.general_information.authorized_by".to_owned()),
                    message: "Approver must match delegation record".to_owned(),
                    category: Some(FeedbackCategory::StanfordPolicyMismatch),
                }],
            }),
        );
        let rendered = render_feedback_capture_markdown(&capture);

        assert!(rendered.contains("Submission status: returned"));
        assert!(rendered.contains("Approver must match delegation record"));
        assert!(rendered.contains("missing_required_field"));
    }

    #[test]
    fn parses_annotation_documents_from_wrapped_yaml() {
        let parsed = parse_correction_annotations_str(
            r#"
corrections:
  - path: expense_report.general_information.authorized_by
    reason: missing_required_field
    note: Confirmed by FA
"#,
            ReportFormat::Yaml,
        )
        .expect("annotations should parse");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].reason, FeedbackCategory::MissingRequiredField);
    }

    #[test]
    fn parses_submission_feedback_from_json() {
        let parsed = parse_submission_feedback_str(
            r#"{
              "status": "accepted",
              "message": "Submitted successfully",
              "category": "other",
              "returned_fields": []
            }"#,
            ReportFormat::Json,
        )
        .expect("submission feedback should parse");

        assert_eq!(parsed.status, SubmissionStatus::Accepted);
        assert!(parsed.returned_fields.is_empty());
    }
}
