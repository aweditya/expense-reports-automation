use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::bundle_synthesis::CanonicalExpenseBundle;
use crate::document_facts::{DocumentFactsPayload, DocumentKind, ExtractionStatus};
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
    pub allowed_values: Vec<String>,
    pub collection_columns: Vec<CopyCollectionColumn>,
    pub collection_rows: Vec<CopyCollectionRow>,
    pub value: Option<String>,
    pub present: bool,
    pub needs_review: bool,
    pub required: bool,
    pub source: Option<String>,
    pub entry_mode: String,
    pub evidence: Vec<EvidenceReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyCollectionColumn {
    pub key: String,
    pub label: String,
    pub control: String,
    pub allowed_values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyCollectionRow {
    pub values: BTreeMap<String, String>,
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
pub struct DocumentSnapshotField {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSnapshotCard {
    pub document_id: String,
    pub filename: String,
    pub kind: String,
    pub extraction_status: String,
    pub used_in_bundle: bool,
    pub projected_to_filing: bool,
    pub status_label: String,
    pub summary_fields: Vec<DocumentSnapshotField>,
    pub issue_messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPacket {
    pub summary: PacketSummary,
    pub issues_queue: Vec<ReviewIssueEntry>,
    pub copy_sections: Vec<CopySection>,
    pub attachment_checklist: Vec<AttachmentChecklistItem>,
    #[serde(default)]
    pub document_snapshots: Vec<DocumentSnapshotCard>,
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
    allowed_values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollectionHelperSpec {
    parent_path: String,
    label: String,
    source: Option<String>,
    entry_mode: String,
    required: bool,
    columns: Vec<CopyCollectionColumn>,
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
        copy_sections: build_copy_sections(draft, readiness, ui_map),
        attachment_checklist: build_attachment_checklist(bundle),
        document_snapshots: build_document_snapshots(bundle),
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

    lines.push("## Source Documents".to_owned());
    if packet.document_snapshots.is_empty() {
        lines.push("- None".to_owned());
    } else {
        for document in &packet.document_snapshots {
            lines.push(format!(
                "- {} ({}) [{}]",
                document.filename, document.kind, document.status_label
            ));
            for field in &document.summary_fields {
                lines.push(format!("  - {}: {}", field.label, field.value));
            }
            for issue in &document.issue_messages {
                lines.push(format!("  - Issue: {}", issue));
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

fn build_copy_sections(
    draft: &DraftReport,
    readiness: &ReadinessReport,
    ui_map: &UiFieldMap,
) -> Vec<CopySection> {
    ui_map
        .sections
        .iter()
        .filter_map(|section| {
            let instances = build_section_instances(draft, readiness, section);
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

fn build_section_instances(
    draft: &DraftReport,
    readiness: &ReadinessReport,
    section: &UiSection,
) -> Vec<CopySectionInstance> {
    if section.repeated {
        let line_count = actual_line_count(draft, &section.path);
        (0..line_count)
            .map(|index| {
                let resolved_section_path = section.path.replace("[]", &format!("[{index}]"));
                let mut fields = section
                    .fields
                    .iter()
                    .filter(|field| !is_nested_collection_child(&field.path))
                    .filter_map(|field| build_copy_field(draft, field, Some(index)))
                    .collect::<Vec<_>>();
                fields.extend(build_collection_helper_fields(
                    draft,
                    readiness,
                    section,
                    Some(index),
                ));
                CopySectionInstance {
                    path: resolved_section_path,
                    label: format!("{} {}", section.label, index + 1),
                    fields,
                }
            })
            .filter(|instance: &CopySectionInstance| !instance.fields.is_empty())
            .collect()
    } else {
        let mut fields = section
            .fields
            .iter()
            .filter(|field| !is_nested_collection_child(&field.path))
            .filter_map(|field| build_copy_field(draft, field, None))
            .collect::<Vec<_>>();
        fields.extend(build_collection_helper_fields(
            draft,
            readiness,
            section,
            None,
        ));
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
        allowed_values: field.allowed_values.clone(),
        collection_columns: Vec::new(),
        collection_rows: Vec::new(),
        value: value.and_then(report_value_to_string),
        present,
        needs_review: metadata.is_some_and(|value| value.needs_review),
        required: field.required,
        source: field.source.clone(),
        entry_mode: field.entry_mode.clone(),
        evidence: metadata.map_or_else(Vec::new, |value| value.evidence.clone()),
    })
}

fn build_collection_helper_fields(
    draft: &DraftReport,
    readiness: &ReadinessReport,
    section: &UiSection,
    line_index: Option<usize>,
) -> Vec<CopyField> {
    collect_collection_helper_specs(section)
        .into_iter()
        .filter_map(|spec| {
            let resolved_path = resolve_ui_field_path(&spec.parent_path, line_index);
            let issue = readiness
                .issues
                .iter()
                .find(|issue| issue.path == resolved_path);
            let rows = collection_rows_at(&draft.report, &resolved_path, &spec.columns);
            let present_non_empty = !rows.is_empty();
            if !present_non_empty && issue.is_none() {
                return None;
            }

            Some(CopyField {
                path: resolved_path,
                label: spec.label,
                control: "structured_list".to_owned(),
                allowed_values: Vec::new(),
                collection_columns: spec.columns,
                collection_rows: rows,
                value: None,
                present: present_non_empty,
                needs_review: issue.is_some_and(|issue| issue.class == ReadinessIssueClass::ManualReview),
                required: spec.required || issue.is_some(),
                source: spec.source,
                entry_mode: spec.entry_mode,
                evidence: Vec::new(),
            })
        })
        .collect()
}

fn collect_collection_helper_specs(section: &UiSection) -> Vec<CollectionHelperSpec> {
    let mut grouped = std::collections::BTreeMap::<String, CollectionHelperSpec>::new();

    for field in &section.fields {
        let Some((parent_path, child_key)) = collection_parent_and_key(&field.path) else {
            continue;
        };
        let parent_label = title_case_label(&humanize_path_tail(&parent_path));
        let entry = grouped
            .entry(parent_path.clone())
            .or_insert_with(|| CollectionHelperSpec {
                parent_path,
                label: parent_label,
                source: field.source.clone(),
                entry_mode: field.entry_mode.clone(),
                required: field.required,
                columns: Vec::new(),
            });
        entry.required |= field.required;
        if !entry.columns.iter().any(|column| column.key == child_key) {
            entry.columns.push(CopyCollectionColumn {
                key: child_key.to_owned(),
                label: field.label.clone(),
                control: field.control.clone(),
                allowed_values: field.allowed_values.clone(),
            });
        }
    }

    grouped
        .into_values()
        .filter(|spec| !spec.columns.is_empty())
        .collect()
}

fn collection_parent_and_key(path: &str) -> Option<(String, String)> {
    let marker_index = path.rfind("[]")?;
    let parent = path[..marker_index].to_owned();
    let child = path[marker_index + 2..].strip_prefix('.')?;
    if child.is_empty() || child.contains('.') || child.contains('[') {
        return None;
    }
    Some((parent, child.to_owned()))
}

fn is_nested_collection_child(path: &str) -> bool {
    path.matches("[]").count() > 1
}

fn collection_rows_at(
    report: &ReportValue,
    path: &str,
    columns: &[CopyCollectionColumn],
) -> Vec<CopyCollectionRow> {
    value_at(report, path)
        .and_then(ReportValue::as_array)
        .map(|items| {
            items.iter()
                .filter_map(|item| {
                    let object = item.as_object()?;
                    let mut values = BTreeMap::new();
                    for column in columns {
                        let value = object
                            .get(&column.key)
                            .and_then(report_value_to_string)
                            .unwrap_or_default();
                        values.insert(column.key.clone(), value);
                    }
                    Some(CopyCollectionRow { values })
                })
                .collect()
        })
        .unwrap_or_default()
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

fn build_document_snapshots(bundle: &CanonicalExpenseBundle) -> Vec<DocumentSnapshotCard> {
    let mut used_document_ids = BTreeSet::new();
    let mut projected_document_ids = BTreeSet::new();

    for line in &bundle.expense_lines {
        for source_document in &line.source_documents {
            used_document_ids.insert(source_document.document_id.clone());
            if line.projection_supported {
                projected_document_ids.insert(source_document.document_id.clone());
            }
        }
    }

    bundle
        .documents
        .iter()
        .map(|document| {
            let used_in_bundle = used_document_ids.contains(&document.document_id);
            let projected_to_filing = projected_document_ids.contains(&document.document_id);
            let status_label = if projected_to_filing {
                "projected into filing".to_owned()
            } else if used_in_bundle {
                "parsed for bundle context only".to_owned()
            } else if matches!(document.extraction_status, ExtractionStatus::Unsupported)
                || matches!(document.classification.kind, DocumentKind::Unknown)
            {
                "ocr captured, not yet supported".to_owned()
            } else {
                "ocr captured, not projected".to_owned()
            };

            let mut issue_messages = BTreeSet::new();
            for issue in &document.issues {
                issue_messages.insert(issue.message.clone());
            }
            for issue in &bundle.issues {
                if issue.document_ids.iter().any(|value| value == &document.document_id) {
                    issue_messages.insert(issue.message.clone());
                }
            }

            DocumentSnapshotCard {
                document_id: document.document_id.clone(),
                filename: document.filename.clone(),
                kind: document.classification.kind.as_str().to_owned(),
                extraction_status: extraction_status_label(document.extraction_status).to_owned(),
                used_in_bundle,
                projected_to_filing,
                status_label,
                summary_fields: document_snapshot_fields(
                    document,
                    bundle
                        .expense_lines
                        .iter()
                        .find(|line| line.document_id == document.document_id),
                ),
                issue_messages: issue_messages.into_iter().collect(),
            }
        })
        .collect()
}

fn document_snapshot_fields(
    document: &crate::ExtractedDocumentFacts,
    line: Option<&crate::bundle_synthesis::CanonicalExpenseLine>,
) -> Vec<DocumentSnapshotField> {
    let mut fields = Vec::new();
    match &document.facts {
        DocumentFactsPayload::Receipt(facts) => {
            push_snapshot_field(
                &mut fields,
                "Merchant",
                facts.merchant_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Date",
                facts.transaction_date.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Total",
                facts.total_paid.as_ref().map(observed_money_amount_display),
            );
            if !facts.line_items.is_empty() {
                push_snapshot_field(
                    &mut fields,
                    "Line items",
                    Some(facts.line_items.len().to_string()),
                );
            }
        }
        DocumentFactsPayload::HotelFolio(facts) => {
            push_snapshot_field(
                &mut fields,
                "Property",
                facts.property_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Stay window",
                facts.stay_window.as_ref().map(|value| {
                    format!(
                        "{} to {}",
                        value.value.start_date, value.value.end_date
                    )
                }),
            );
            push_snapshot_field(
                &mut fields,
                "Total",
                facts.total_paid.as_ref().map(observed_money_amount_display),
            );
        }
        DocumentFactsPayload::FlightItinerary(facts) => {
            push_snapshot_field(
                &mut fields,
                "Traveler",
                facts.traveler_names.first().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Trip window",
                facts.trip_window.as_ref().map(|value| {
                    format!(
                        "{} to {}",
                        value.value.start_date, value.value.end_date
                    )
                }),
            );
            push_snapshot_field(
                &mut fields,
                "Total",
                facts.total_paid.as_ref().map(observed_money_amount_display),
            );
            if !facts.segments.is_empty() {
                push_snapshot_field(
                    &mut fields,
                    "Segments",
                    Some(facts.segments.len().to_string()),
                );
            }
        }
        DocumentFactsPayload::ConferenceRegistration(facts) => {
            push_snapshot_field(
                &mut fields,
                "Attendee",
                facts.attendee_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Event",
                facts.event_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Total",
                facts.total_paid.as_ref().map(observed_money_amount_display),
            );
        }
        DocumentFactsPayload::ConferenceProgram(facts) => {
            push_snapshot_field(
                &mut fields,
                "Event",
                facts.event_name.as_ref().map(|value| value.value.clone()),
            );
            if !facts.presentations.is_empty() {
                push_snapshot_field(
                    &mut fields,
                    "Presentations",
                    Some(facts.presentations.len().to_string()),
                );
            }
        }
        DocumentFactsPayload::CurrencyConversion(facts) => {
            push_snapshot_field(
                &mut fields,
                "Provider",
                facts.provider_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Exchange rate",
                facts.exchange_rate.as_ref().map(|value| value.value.clone()),
            );
        }
        DocumentFactsPayload::AirfarePriceComparison(facts) => {
            push_snapshot_field(
                &mut fields,
                "Selected fare",
                facts.selected_fare.as_ref().map(observed_money_amount_display),
            );
            push_snapshot_field(
                &mut fields,
                "Lowest logical fare",
                facts.lowest_logical_fare
                    .as_ref()
                    .map(observed_money_amount_display),
            );
        }
        DocumentFactsPayload::MissingReceiptDeclaration(facts) => {
            push_snapshot_field(
                &mut fields,
                "Merchant",
                facts.merchant_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Amount",
                facts.amount.as_ref().map(observed_money_amount_display),
            );
        }
        DocumentFactsPayload::StanfordExpenseSummary(facts) => {
            push_snapshot_field(
                &mut fields,
                "Payee",
                facts.payee_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Event",
                facts.event_name.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Reimbursement",
                facts.reimbursement_amount
                    .as_ref()
                    .map(observed_money_amount_display),
            );
        }
        DocumentFactsPayload::Unknown(facts) => {
            push_snapshot_field(
                &mut fields,
                "Title hint",
                facts.title_hint.as_ref().map(|value| value.value.clone()),
            );
            push_snapshot_field(
                &mut fields,
                "Text summary",
                facts.text_summary.as_ref().map(|value| value.value.clone()),
            );
        }
    }

    if let Some(line) = line {
        push_snapshot_field(
            &mut fields,
            "Total USD",
            line.line_amount_usd.as_ref().map(|value| value.value.clone()),
        );
        push_snapshot_field(
            &mut fields,
            "Exchange rate",
            line.exchange_rate.as_ref().map(|value| value.value.clone()),
        );
        push_snapshot_field(
            &mut fields,
            "Projected expense type",
            line.expense_type.as_ref().map(|value| value.value.clone()),
        );
    }

    fields
}

fn push_snapshot_field(
    fields: &mut Vec<DocumentSnapshotField>,
    label: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        fields.push(DocumentSnapshotField {
            label: label.to_owned(),
            value,
        });
    }
}

fn money_amount_display(amount: &crate::MoneyAmount) -> String {
    match amount.currency.as_deref() {
        Some(currency) if !currency.is_empty() => format!("{currency} {}", amount.amount),
        _ => amount.amount.clone(),
    }
}

fn observed_money_amount_display(amount: &crate::Observed<crate::MoneyAmount>) -> String {
    money_amount_display(&amount.value)
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

fn title_case_label(value: &str) -> String {
    value.split_whitespace()
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

fn extraction_status_label(status: ExtractionStatus) -> &'static str {
    match status {
        ExtractionStatus::Complete => "complete",
        ExtractionStatus::Partial => "partial",
        ExtractionStatus::NeedsReview => "needs_review",
        ExtractionStatus::Unsupported => "unsupported",
    }
}

fn push_optional_line(lines: &mut Vec<String>, prefix: &str, value: Option<&str>) {
    lines.push(format!("{}{}", prefix, value.unwrap_or("[missing]")));
}

#[cfg(test)]
mod tests {
    use super::{build_review_packet, render_review_packet_markdown, FilingStatus};
    use crate::bundle_synthesis::{
        synthesize_bundle, synthesize_bundle_projection, synthesize_bundle_projection_with_fx,
        CanonicalExpenseKind, StaticFxRateProvider,
    };
    use crate::document_facts::{
        DocumentClassification, DocumentFactsPayload, ExtractedDocumentFacts, ExtractionStatus,
        IssueSeverity, MoneyAmount, ReceiptFacts,
    };
    use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference};
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};

    fn synthetic_documents() -> Vec<crate::ExtractedDocumentFacts> {
        generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect()
    }

    fn sample_evidence(document_id: &str, filename: &str, quote: &str) -> Vec<EvidenceReference> {
        vec![EvidenceReference {
            kind: EvidenceKind::DocumentSpan,
            document_id: Some(document_id.to_owned()),
            filename: Some(filename.to_owned()),
            page: Some(1),
            quote: Some(quote.to_owned()),
            origin: None,
        }]
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
        assert_eq!(packet.document_snapshots.len(), 3);
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
                == "expense_report.transaction_lines[0].common.source_documents"
                && field.control == "structured_list"
                && field.collection_rows.len() == 1
                && field.collection_rows[0]
                    .values
                    .get("filename")
                    .map(String::as_str)
                    == Some("synthetic_flight_itinerary_baseline.md")));
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
        assert!(transaction_lines.instances[2]
            .fields
            .iter()
            .any(|field| field.path
                == "expense_report.transaction_lines[2].meal_details.attendees"
                && field.control == "structured_list"
                && field.required
                && field.collection_columns.len() == 2));
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
        assert!(rendered.contains("## Source Documents"));
        assert!(rendered.contains("## Attachment Checklist"));
        assert!(rendered.contains("user_input_required"));
    }

    #[test]
    fn review_packet_surfaces_unprojected_receipt_snapshots() {
        let document_id = "receipt_book_talk";
        let filename = "book_talk_receipt.png";
        let receipt = ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: DocumentClassification {
                kind: crate::DocumentKind::Receipt,
                confidence: ConfidenceLevel::Medium,
                evidence: sample_evidence(document_id, filename, "BOOK TALK"),
                flags: Vec::new(),
            },
            extraction_status: ExtractionStatus::Partial,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(crate::Observed::new(
                    "BOOK TALK".to_owned(),
                    ConfidenceLevel::Medium,
                    sample_evidence(document_id, filename, "BOOK TALK"),
                )),
                merchant_location: None,
                transaction_date: Some(crate::Observed::new(
                    "2019-01-11".to_owned(),
                    ConfidenceLevel::Medium,
                    sample_evidence(document_id, filename, "Date: 11/01/2019"),
                )),
                total_paid: Some(crate::Observed::new(
                    MoneyAmount {
                        amount: "80.90".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    ConfidenceLevel::Medium,
                    sample_evidence(document_id, filename, "Grand Total SGD 80.90"),
                )),
                subtotal: None,
                tax_amount: None,
                tip_amount: None,
                line_items: Vec::new(),
            }),
            issues: vec![crate::DocumentExtractionIssue {
                severity: IssueSeverity::Warning,
                code: "missing_line_items".to_owned(),
                message: "Receipt line items were not recovered".to_owned(),
                evidence: Vec::new(),
            }],
        };

        let bundle = synthesize_bundle(&[receipt]);
        assert!(bundle
            .expense_lines
            .iter()
            .any(|line| line.kind == CanonicalExpenseKind::GenericReceipt && !line.projection_supported));
        let projection = synthesize_bundle_projection(&bundle.documents);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");

        assert_eq!(packet.document_snapshots.len(), 1);
        assert_eq!(packet.document_snapshots[0].filename, filename);
        assert!(!packet.document_snapshots[0].projected_to_filing);
        assert_eq!(
            packet.document_snapshots[0].status_label,
            "parsed for bundle context only"
        );
        assert!(packet.document_snapshots[0]
            .summary_fields
            .iter()
            .any(|field| field.label == "Merchant" && field.value == "BOOK TALK"));
        assert!(packet.document_snapshots[0]
            .issue_messages
            .iter()
            .any(|message| message.contains("not yet projected") || message.contains("not projected")));
    }

    #[test]
    fn review_packet_surfaces_fx_enriched_totals_for_unprojected_receipts() {
        let document_id = "receipt_book_talk";
        let filename = "book_talk_receipt.png";
        let receipt = ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: DocumentClassification {
                kind: crate::DocumentKind::Receipt,
                confidence: ConfidenceLevel::Medium,
                evidence: sample_evidence(document_id, filename, "BOOK TALK"),
                flags: Vec::new(),
            },
            extraction_status: ExtractionStatus::Partial,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(crate::Observed::new(
                    "BOOK TALK".to_owned(),
                    ConfidenceLevel::Medium,
                    sample_evidence(document_id, filename, "BOOK TALK"),
                )),
                merchant_location: None,
                transaction_date: Some(crate::Observed::new(
                    "2019-01-11".to_owned(),
                    ConfidenceLevel::Medium,
                    sample_evidence(document_id, filename, "Date: 11/01/2019"),
                )),
                total_paid: Some(crate::Observed::new(
                    MoneyAmount {
                        amount: "80.90".to_owned(),
                        currency: Some("MYR".to_owned()),
                    },
                    ConfidenceLevel::Medium,
                    sample_evidence(document_id, filename, "Grand Total MYR 80.90"),
                )),
                subtotal: None,
                tax_amount: None,
                tip_amount: None,
                line_items: Vec::new(),
            }),
            issues: vec![crate::DocumentExtractionIssue {
                severity: IssueSeverity::Warning,
                code: "missing_line_items".to_owned(),
                message: "Receipt line items were not recovered".to_owned(),
                evidence: Vec::new(),
            }],
        };

        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&[receipt], &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");

        assert_eq!(packet.summary.report_total_usd.as_deref(), Some("19.42"));
        assert!(packet.document_snapshots[0]
            .summary_fields
            .iter()
            .any(|field| field.label == "Total USD" && field.value == "19.42"));
        assert!(packet.document_snapshots[0]
            .summary_fields
            .iter()
            .any(|field| field.label == "Exchange rate" && field.value == "0.24"));
    }
}
