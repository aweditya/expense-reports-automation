use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::bundle_synthesis::CanonicalExpenseBundle;
use crate::draft::{ConfidenceLevel, DraftReport, EvidenceReference};
use crate::readiness::{
    summarize_validation_readiness, ReadinessIssue, ReadinessIssueClass, ReadinessReport,
};
use crate::validator::ValidationReport;
use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilingStatus {
    AutomationBlocked,
    UserInputRequired,
    ManualReviewRequired,
    ReadyToFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfidenceSummary {
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub needs_review: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketSummary {
    pub filing_status: FilingStatus,
    pub payee_name: Option<String>,
    pub event_name: Option<String>,
    pub trip_window: Option<String>,
    pub report_total_usd: Option<String>,
    pub category: Option<String>,
    pub transaction_type: Option<String>,
    pub transaction_line_count: usize,
    pub document_count: usize,
    pub readiness: ReviewReadinessSummary,
    pub confidence: ConfidenceSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReadinessSummary {
    pub automation_gap_count: usize,
    pub user_input_gap_count: usize,
    pub manual_review_count: usize,
    pub other_warning_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewIssueEntry {
    pub class: ReadinessIssueClass,
    pub path: String,
    pub label: String,
    pub source: Option<String>,
    pub current_value: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyField {
    pub path: String,
    pub label: String,
    pub control: String,
    pub value: Option<String>,
    pub present: bool,
    pub needs_review: bool,
    pub required: bool,
    pub source: Option<String>,
    pub entry_mode: String,
    pub evidence: Vec<EvidenceReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopySectionInstance {
    pub path: String,
    pub label: String,
    pub fields: Vec<CopyField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopySection {
    pub key: String,
    pub label: String,
    pub repeated: bool,
    pub instances: Vec<CopySectionInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentChecklistItem {
    pub line_index: usize,
    pub expense_type: Option<String>,
    pub remarks: Option<String>,
    pub filenames: Vec<String>,
    pub document_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPacket {
    pub summary: PacketSummary,
    pub issues_queue: Vec<ReviewIssueEntry>,
    pub copy_sections: Vec<CopySection>,
    pub attachment_checklist: Vec<AttachmentChecklistItem>,
}

#[derive(Debug)]
pub enum ReviewPacketError {
    UiFieldMapParse(String),
    Json(serde_json::Error),
}

impl std::fmt::Display for ReviewPacketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UiFieldMapParse(message) => write!(f, "{message}"),
            Self::Json(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ReviewPacketError {}

impl From<serde_json::Error> for ReviewPacketError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UiFieldMap {
    sections: Vec<UiSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UiSection {
    key: String,
    label: String,
    path: String,
    repeated: bool,
    fields: Vec<UiField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UiField {
    path: String,
    label: String,
    control: String,
    source: Option<String>,
    entry_mode: String,
    required: bool,
}

static UI_FIELD_MAP: OnceLock<Result<UiFieldMap, String>> = OnceLock::new();

pub fn build_review_packet(
    bundle: &CanonicalExpenseBundle,
    draft: &DraftReport,
    validation: &ValidationReport,
) -> Result<ReviewPacket, ReviewPacketError> {
    let readiness = summarize_validation_readiness(validation);
    build_review_packet_with_readiness(bundle, draft, &readiness)
}

pub fn build_review_packet_with_readiness(
    bundle: &CanonicalExpenseBundle,
    draft: &DraftReport,
    readiness: &ReadinessReport,
) -> Result<ReviewPacket, ReviewPacketError> {
    let ui_map = load_ui_field_map().map_err(ReviewPacketError::UiFieldMapParse)?;

    Ok(ReviewPacket {
        summary: build_packet_summary(bundle, draft, readiness),
        issues_queue: build_issue_queue(draft, readiness, ui_map),
        copy_sections: build_copy_sections(draft, ui_map),
        attachment_checklist: build_attachment_checklist(bundle),
    })
}

pub fn render_review_packet_json_pretty(
    packet: &ReviewPacket,
) -> Result<String, ReviewPacketError> {
    Ok(serde_json::to_string_pretty(packet)?)
}

pub fn render_review_packet_markdown(packet: &ReviewPacket) -> String {
    let mut lines = vec!["# Review Packet".to_owned(), String::new()];

    lines.push("## Packet Summary".to_owned());
    lines.push(format!(
        "- Filing Status: {}",
        filing_status_label(packet.summary.filing_status)
    ));
    push_optional_line(
        &mut lines,
        "- Payee: ",
        packet.summary.payee_name.as_deref(),
    );
    push_optional_line(
        &mut lines,
        "- Event: ",
        packet.summary.event_name.as_deref(),
    );
    push_optional_line(
        &mut lines,
        "- Trip Window: ",
        packet.summary.trip_window.as_deref(),
    );
    push_optional_line(
        &mut lines,
        "- Report Total USD: ",
        packet.summary.report_total_usd.as_deref(),
    );
    push_optional_line(
        &mut lines,
        "- Category: ",
        packet.summary.category.as_deref(),
    );
    push_optional_line(
        &mut lines,
        "- Transaction Type: ",
        packet.summary.transaction_type.as_deref(),
    );
    lines.push(format!(
        "- Packet Size: {} document(s), {} transaction line(s)",
        packet.summary.document_count, packet.summary.transaction_line_count
    ));
    lines.push(format!(
        "- Readiness: {} automation gap(s), {} user input gap(s), {} manual review item(s)",
        packet.summary.readiness.automation_gap_count,
        packet.summary.readiness.user_input_gap_count,
        packet.summary.readiness.manual_review_count
    ));
    lines.push(format!(
        "- Confidence: {} high, {} medium, {} low, {} needs review",
        packet.summary.confidence.high,
        packet.summary.confidence.medium,
        packet.summary.confidence.low,
        packet.summary.confidence.needs_review
    ));
    lines.push(String::new());

    lines.push("## Issues Queue".to_owned());
    if packet.issues_queue.is_empty() {
        lines.push("- None".to_owned());
    } else {
        for issue in &packet.issues_queue {
            lines.push(format!(
                "- [{}] {} ({}){}",
                readiness_class_label(issue.class),
                issue.label,
                issue.path,
                issue
                    .current_value
                    .as_ref()
                    .map(|value| format!(" -> {value}"))
                    .unwrap_or_default()
            ));
            lines.push(format!("  {}", issue.message));
        }
    }
    lines.push(String::new());

    lines.push("## Oracle Copy View".to_owned());
    for section in &packet.copy_sections {
        lines.push(format!("### {}", section.label));
        for instance in &section.instances {
            if section.repeated {
                lines.push(format!("- {}", instance.label));
            }
            for field in &instance.fields {
                lines.push(format!(
                    "  - {}: {}{}",
                    field.label,
                    field.value.as_deref().unwrap_or("[missing]"),
                    if field.needs_review { " [review]" } else { "" }
                ));
            }
        }
    }
    lines.push(String::new());

    lines.push("## Attachment Checklist".to_owned());
    if packet.attachment_checklist.is_empty() {
        lines.push("- None".to_owned());
    } else {
        for item in &packet.attachment_checklist {
            lines.push(format!(
                "- Line {}: {}",
                item.line_index + 1,
                item.expense_type
                    .as_deref()
                    .unwrap_or("unknown expense type")
            ));
            if let Some(remarks) = item.remarks.as_deref() {
                lines.push(format!("  - Remarks: {remarks}"));
            }
            if !item.filenames.is_empty() {
                lines.push(format!("  - Files: {}", item.filenames.join(", ")));
            }
        }
    }

    lines.join("\n")
}

fn build_packet_summary(
    bundle: &CanonicalExpenseBundle,
    draft: &DraftReport,
    readiness: &ReadinessReport,
) -> PacketSummary {
    PacketSummary {
        filing_status: filing_status_from_readiness(readiness),
        payee_name: value_text_at(
            &draft.report,
            "expense_report.general_information.payee.name",
        ),
        event_name: value_text_at(
            &draft.report,
            "expense_report.general_information.event_name",
        ),
        trip_window: value_text_at(
            &draft.report,
            "expense_report.general_information.business_purpose.when",
        ),
        report_total_usd: value_text_at(
            &draft.report,
            "expense_report.transaction_summary.total_usd",
        ),
        category: value_text_at(&draft.report, "expense_report.general_information.category"),
        transaction_type: value_text_at(
            &draft.report,
            "expense_report.transaction_summary.transaction_type",
        ),
        transaction_line_count: draft
            .report
            .as_object()
            .and_then(|root| root.get("transaction_lines"))
            .and_then(ReportValue::as_array)
            .map_or(0, |lines| lines.len()),
        document_count: bundle.documents.len(),
        readiness: ReviewReadinessSummary {
            automation_gap_count: readiness.automation_gap_count(),
            user_input_gap_count: readiness.user_input_required_count(),
            manual_review_count: readiness.manual_review_count(),
            other_warning_count: readiness.other_warning_count(),
        },
        confidence: confidence_summary(draft),
    }
}

fn build_issue_queue(
    draft: &DraftReport,
    readiness: &ReadinessReport,
    ui_map: &UiFieldMap,
) -> Vec<ReviewIssueEntry> {
    let mut issues = readiness
        .issues
        .iter()
        .map(|issue| {
            let label = ui_field_for_issue(ui_map, issue)
                .map(|field| field.label.clone())
                .unwrap_or_else(|| humanize_path_tail(&issue.schema_path));
            ReviewIssueEntry {
                class: issue.class,
                path: issue.path.clone(),
                label,
                source: issue.source.clone(),
                current_value: value_text_at(&draft.report, &issue.path),
                message: issue.message.clone(),
            }
        })
        .collect::<Vec<_>>();

    issues.sort_by_key(|issue| {
        (
            issue_class_rank(issue.class),
            issue.label.clone(),
            issue.path.clone(),
        )
    });
    issues
}

fn build_copy_sections(draft: &DraftReport, ui_map: &UiFieldMap) -> Vec<CopySection> {
    ui_map
        .sections
        .iter()
        .filter_map(|section| {
            let instances = build_section_instances(draft, section);
            if instances.is_empty() {
                None
            } else {
                Some(CopySection {
                    key: section.key.clone(),
                    label: section.label.clone(),
                    repeated: section.repeated,
                    instances,
                })
            }
        })
        .collect()
}

fn build_section_instances(draft: &DraftReport, section: &UiSection) -> Vec<CopySectionInstance> {
    if section.repeated {
        let line_count = actual_line_count(draft, &section.path);
        (0..line_count)
            .map(|index| {
                let resolved_section_path = section.path.replace("[]", &format!("[{index}]"));
                CopySectionInstance {
                    path: resolved_section_path,
                    label: format!("{} {}", section.label, index + 1),
                    fields: section
                        .fields
                        .iter()
                        .filter_map(|field| build_copy_field(draft, field, Some(index)))
                        .collect(),
                }
            })
            .filter(|instance: &CopySectionInstance| !instance.fields.is_empty())
            .collect()
    } else {
        let fields = section
            .fields
            .iter()
            .filter_map(|field| build_copy_field(draft, field, None))
            .collect::<Vec<_>>();
        if fields.is_empty() {
            Vec::new()
        } else {
            vec![CopySectionInstance {
                path: section.path.clone(),
                label: section.label.clone(),
                fields,
            }]
        }
    }
}

fn build_copy_field(
    draft: &DraftReport,
    field: &UiField,
    line_index: Option<usize>,
) -> Option<CopyField> {
    let resolved_path = resolve_ui_field_path(&field.path, line_index);
    let value = value_at(&draft.report, &resolved_path);
    let present = value.is_some();
    let relevant = line_index.map_or(true, |index| {
        field_is_relevant_for_line(draft, field, index)
    });

    if !relevant || (!present && !field.required) {
        return None;
    }

    let metadata = draft.metadata.get(&resolved_path);
    Some(CopyField {
        path: resolved_path.clone(),
        label: field.label.clone(),
        control: field.control.clone(),
        value: value.and_then(report_value_to_string),
        present,
        needs_review: metadata.is_some_and(|value| value.needs_review),
        required: field.required,
        source: field.source.clone(),
        entry_mode: field.entry_mode.clone(),
        evidence: metadata.map_or_else(Vec::new, |value| value.evidence.clone()),
    })
}

fn build_attachment_checklist(bundle: &CanonicalExpenseBundle) -> Vec<AttachmentChecklistItem> {
    bundle
        .expense_lines
        .iter()
        .filter(|line| line.projection_supported)
        .enumerate()
        .map(|(line_index, line)| AttachmentChecklistItem {
            line_index,
            expense_type: line.expense_type.as_ref().map(|value| value.value.clone()),
            remarks: line.remarks.as_ref().map(|value| value.value.clone()),
            filenames: line
                .source_documents
                .iter()
                .map(|document| document.filename.clone())
                .collect(),
            document_types: line
                .source_documents
                .iter()
                .map(|document| document.document_type.clone())
                .collect(),
        })
        .collect()
}

fn filing_status_from_readiness(readiness: &ReadinessReport) -> FilingStatus {
    if readiness.automation_gap_count() > 0 {
        FilingStatus::AutomationBlocked
    } else if readiness.user_input_required_count() > 0 {
        FilingStatus::UserInputRequired
    } else if readiness.manual_review_count() > 0 {
        FilingStatus::ManualReviewRequired
    } else {
        FilingStatus::ReadyToFile
    }
}

fn confidence_summary(draft: &DraftReport) -> ConfidenceSummary {
    let mut summary = ConfidenceSummary {
        high: 0,
        medium: 0,
        low: 0,
        needs_review: 0,
    };

    for metadata in draft.metadata.values() {
        match metadata.confidence {
            ConfidenceLevel::High => summary.high += 1,
            ConfidenceLevel::Medium => summary.medium += 1,
            ConfidenceLevel::Low => summary.low += 1,
        }
        if metadata.needs_review {
            summary.needs_review += 1;
        }
    }

    summary
}

fn load_ui_field_map() -> Result<&'static UiFieldMap, String> {
    let result = UI_FIELD_MAP.get_or_init(|| {
        serde_yaml::from_str(include_str!("../generated/ui_field_map.yaml"))
            .map_err(|err| format!("failed to parse generated/ui_field_map.yaml: {err}"))
    });
    result.as_ref().map_err(Clone::clone)
}

fn ui_field_for_issue<'a>(ui_map: &'a UiFieldMap, issue: &ReadinessIssue) -> Option<&'a UiField> {
    let target = wildcard_schema_path(&issue.schema_path);
    ui_map
        .sections
        .iter()
        .flat_map(|section| section.fields.iter())
        .find(|field| field.path == target)
}

fn actual_line_count(draft: &DraftReport, section_path: &str) -> usize {
    let path = section_path
        .strip_prefix("expense_report.")
        .unwrap_or(section_path);
    value_at(&draft.report, path)
        .and_then(ReportValue::as_array)
        .map_or(0, |lines| lines.len())
}

fn resolve_ui_field_path(path: &str, line_index: Option<usize>) -> String {
    let path = line_index.map_or_else(
        || path.to_owned(),
        |index| path.replacen("[]", &format!("[{index}]"), 1),
    );
    path.replace("[]", "[0]")
}

fn field_is_relevant_for_line(draft: &DraftReport, field: &UiField, line_index: usize) -> bool {
    let wildcard_path = field
        .path
        .strip_prefix("expense_report.transaction_lines[].")
        .unwrap_or(&field.path);
    let top_group = wildcard_path.split('.').next().unwrap_or(wildcard_path);
    if top_group == "common" {
        return true;
    }

    value_at(
        &draft.report,
        &format!("expense_report.transaction_lines[{line_index}].{top_group}"),
    )
    .is_some()
}

fn value_text_at(report: &ReportValue, path: &str) -> Option<String> {
    value_at(report, path).and_then(report_value_to_string)
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

fn value_at<'a>(value: &'a ReportValue, path: &str) -> Option<&'a ReportValue> {
    let normalized_path = path.strip_prefix("expense_report.").unwrap_or(path);
    let mut current = value;
    for segment in normalized_path.split('.') {
        let mut segment = segment;
        loop {
            if let Some(index_start) = segment.find('[') {
                let key = &segment[..index_start];
                if !key.is_empty() {
                    current = current.as_object()?.get(key)?;
                }
                let remainder = &segment[index_start + 1..];
                let index_end = remainder.find(']')?;
                let index = remainder[..index_end].parse::<usize>().ok()?;
                current = current.as_array()?.get(index)?;
                segment = &remainder[index_end + 1..];
                if segment.is_empty() {
                    break;
                }
            } else {
                current = current.as_object()?.get(segment)?;
                break;
            }
        }
    }
    Some(current)
}

fn wildcard_schema_path(path: &str) -> String {
    let mut normalized = String::new();
    let mut chars = path.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '[' {
            while let Some(next) = chars.peek() {
                if *next == ']' {
                    chars.next();
                    break;
                }
                chars.next();
            }
            normalized.push_str("[]");
        } else {
            normalized.push(ch);
        }
    }
    normalized
}

fn humanize_path_tail(path: &str) -> String {
    path.rsplit('.').next().unwrap_or(path).replace('_', " ")
}

fn issue_class_rank(class: ReadinessIssueClass) -> u8 {
    match class {
        ReadinessIssueClass::AutomationGap => 0,
        ReadinessIssueClass::UserInputRequired => 1,
        ReadinessIssueClass::ManualReview => 2,
        ReadinessIssueClass::OtherWarning => 3,
    }
}

fn filing_status_label(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "automation_blocked",
        FilingStatus::UserInputRequired => "user_input_required",
        FilingStatus::ManualReviewRequired => "manual_review_required",
        FilingStatus::ReadyToFile => "ready_to_file",
    }
}

fn readiness_class_label(class: ReadinessIssueClass) -> &'static str {
    match class {
        ReadinessIssueClass::AutomationGap => "automation_gap",
        ReadinessIssueClass::UserInputRequired => "user_input_required",
        ReadinessIssueClass::ManualReview => "manual_review",
        ReadinessIssueClass::OtherWarning => "other_warning",
    }
}

fn push_optional_line(lines: &mut Vec<String>, prefix: &str, value: Option<&str>) {
    lines.push(format!("{}{}", prefix, value.unwrap_or("[missing]")));
}

#[cfg(test)]
mod tests {
    use super::{build_review_packet, render_review_packet_markdown, FilingStatus};
    use crate::bundle_synthesis::{
        synthesize_bundle_projection, synthesize_bundle_projection_with_fx, StaticFxRateProvider,
    };
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};

    fn synthetic_documents() -> Vec<crate::ExtractedDocumentFacts> {
        generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect()
    }

    #[test]
    fn review_packet_summarizes_fx_ready_bundle_for_fa() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");

        assert_eq!(
            packet.summary.filing_status,
            FilingStatus::UserInputRequired
        );
        assert_eq!(packet.summary.payee_name.as_deref(), Some("Olivia Park"));
        assert_eq!(
            packet.summary.event_name.as_deref(),
            Some("Foreign Expenses")
        );
        assert_eq!(packet.summary.report_total_usd.as_deref(), Some("1889.66"));
        assert_eq!(packet.summary.readiness.automation_gap_count, 0);
        assert_eq!(packet.summary.readiness.user_input_gap_count, 4);
        assert_eq!(packet.issues_queue.len(), 7);
        assert!(packet
            .copy_sections
            .iter()
            .any(|section| section.key == "general_information"));
        assert!(packet
            .copy_sections
            .iter()
            .any(|section| section.key == "transaction_lines"));
        assert!(packet
            .copy_sections
            .iter()
            .any(|section| section.key == "allocation_and_approvers"));
        assert_eq!(packet.attachment_checklist.len(), 3);
    }

    #[test]
    fn review_packet_copy_view_tracks_repeated_transaction_lines() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");

        let transaction_lines = packet
            .copy_sections
            .iter()
            .find(|section| section.key == "transaction_lines")
            .expect("transaction line section should exist");
        assert_eq!(transaction_lines.instances.len(), 3);
        assert!(transaction_lines.instances[0]
            .fields
            .iter()
            .any(|field| field.path
                == "expense_report.transaction_lines[0].common.source_documents[0].filename"
                && field.value.as_deref() == Some("synthetic_flight_itinerary_baseline.md")));
        assert!(!transaction_lines.instances[0]
            .fields
            .iter()
            .any(|field| field.path
                == "expense_report.transaction_lines[0].lodging_details.hotel_name"));
        assert!(transaction_lines.instances[2]
            .fields
            .iter()
            .any(|field| field.path
                == "expense_report.transaction_lines[2].meal_details.meal_purpose"
                && field.value.as_deref() == Some("Business meal during travel in Singapore")
                && field.needs_review));
    }

    #[test]
    fn review_packet_marks_no_fx_draft_as_automation_blocked() {
        let projection = synthesize_bundle_projection(&synthetic_documents());
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");

        assert_eq!(
            packet.summary.filing_status,
            FilingStatus::AutomationBlocked
        );
        assert!(packet.issues_queue.iter().any(|issue| issue.class
            == crate::ReadinessIssueClass::AutomationGap
            && issue.path == "expense_report.transaction_summary.total_usd"));
    }

    #[test]
    fn review_packet_markdown_contains_summary_and_issue_headings() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_packet_markdown(&packet);

        assert!(rendered.contains("# Review Packet"));
        assert!(rendered.contains("## Packet Summary"));
        assert!(rendered.contains("## Issues Queue"));
        assert!(rendered.contains("## Oracle Copy View"));
        assert!(rendered.contains("## Attachment Checklist"));
        assert!(rendered.contains("user_input_required"));
    }
}
