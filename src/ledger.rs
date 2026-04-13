use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::bundle_synthesis::CanonicalExpenseBundle;
use crate::draft::DraftReport;
use crate::feedback::{
    apply_user_input_override, capture_feedback, CorrectionAnnotation, FeedbackCapture,
    FeedbackCategory, SubmissionFeedback, SubmissionStatus,
};
use crate::readiness::{summarize_validation_readiness_with_confirmations, ReadinessReport};
use crate::review_packet::{
    build_review_packet_with_readiness, FilingStatus, ReviewPacket, ReviewPacketError,
};
use crate::validator::{validate_draft_report, ValidationReport};
use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerState {
    AutomationBlocked,
    UserInputRequired,
    ManualReviewRequired,
    ReadyToFile,
    Submitted,
    Accepted,
    Returned,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftVersionSource {
    MachineProjection,
    ReviewRevision,
    SiteReturnRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorRole {
    System,
    FinancialAdministrator,
    StanfordSite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewActionKind {
    FieldEdited,
    FieldConfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAttemptState {
    Submitted,
    Accepted,
    Returned,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerSummary {
    pub current_state: LedgerState,
    pub current_draft_version_id: u32,
    pub latest_submission_attempt_id: Option<u32>,
    pub document_count: usize,
    pub draft_version_count: usize,
    pub review_action_count: usize,
    pub submission_attempt_count: usize,
    pub source_filenames: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftVersionRecord {
    pub version_id: u32,
    pub parent_version_id: Option<u32>,
    pub source: DraftVersionSource,
    pub actor_role: ActorRole,
    pub label: String,
    pub confirmed_review_paths: Vec<String>,
    pub draft: DraftReport,
    pub validation: ValidationReport,
    pub readiness: ReadinessReport,
    pub review_packet: ReviewPacket,
    pub feedback_from_parent: Option<FeedbackCapture>,
    pub source_submission_attempt_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewActionRecord {
    pub action_id: u32,
    pub base_version_id: u32,
    pub resulting_version_id: u32,
    pub actor_role: ActorRole,
    pub kind: ReviewActionKind,
    pub path: String,
    pub previous_value: Option<String>,
    pub new_value: Option<String>,
    pub correction_reason: Option<FeedbackCategory>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionAttemptRecord {
    pub attempt_id: u32,
    pub draft_version_id: u32,
    pub status: SubmissionAttemptState,
    pub note: Option<String>,
    pub submission_feedback: Option<SubmissionFeedback>,
    pub resulting_version_id: Option<u32>,
    pub feedback_capture: Option<FeedbackCapture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSubmissionLedger {
    pub bundle_id: String,
    pub bundle: CanonicalExpenseBundle,
    pub summary: LedgerSummary,
    pub draft_versions: Vec<DraftVersionRecord>,
    pub review_actions: Vec<ReviewActionRecord>,
    pub submission_attempts: Vec<SubmissionAttemptRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEditInput {
    pub path: String,
    pub value: ReportValue,
    pub reason: Option<FeedbackCategory>,
    pub note: Option<String>,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRevisionInput {
    pub actor_role: ActorRole,
    pub label: String,
    pub field_edits: Vec<FieldEditInput>,
    pub confirmed_review_paths: Vec<String>,
    pub annotations: Vec<CorrectionAnnotation>,
}

#[derive(Debug)]
pub enum LedgerError {
    ReviewPacket(ReviewPacketError),
    UnknownDraftVersion(u32),
    UnknownSubmissionAttempt(u32),
    InvalidRevision(String),
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReviewPacket(err) => write!(f, "{err}"),
            Self::UnknownDraftVersion(version_id) => {
                write!(f, "unknown draft version {version_id}")
            }
            Self::UnknownSubmissionAttempt(attempt_id) => {
                write!(f, "unknown submission attempt {attempt_id}")
            }
            Self::InvalidRevision(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<ReviewPacketError> for LedgerError {
    fn from(value: ReviewPacketError) -> Self {
        Self::ReviewPacket(value)
    }
}

pub fn initialize_review_submission_ledger(
    bundle_id: impl Into<String>,
    bundle: &CanonicalExpenseBundle,
    draft: &DraftReport,
    validation: &ValidationReport,
) -> Result<ReviewSubmissionLedger, LedgerError> {
    let initial_version = build_version_record(
        1,
        None,
        DraftVersionSource::MachineProjection,
        ActorRole::System,
        "Initial machine draft".to_owned(),
        draft.clone(),
        BTreeSet::new(),
        validation.clone(),
        bundle,
        None,
        None,
    )?;

    let mut ledger = ReviewSubmissionLedger {
        bundle_id: bundle_id.into(),
        bundle: bundle.clone(),
        summary: LedgerSummary {
            current_state: state_from_filing_status(
                initial_version.review_packet.summary.filing_status,
            ),
            current_draft_version_id: initial_version.version_id,
            latest_submission_attempt_id: None,
            document_count: bundle.documents.len(),
            draft_version_count: 1,
            review_action_count: 0,
            submission_attempt_count: 0,
            source_filenames: bundle
                .documents
                .iter()
                .map(|document| document.filename.clone())
                .collect(),
        },
        draft_versions: vec![initial_version],
        review_actions: Vec::new(),
        submission_attempts: Vec::new(),
    };
    refresh_summary(&mut ledger);
    Ok(ledger)
}

pub fn apply_review_revision(
    ledger: &mut ReviewSubmissionLedger,
    base_version_id: u32,
    revision: DraftRevisionInput,
) -> Result<u32, LedgerError> {
    let version_id = next_version_id(ledger);
    let base_version = get_version(ledger, base_version_id)?.clone();
    let mut draft = base_version.draft.clone();
    let mut actions = Vec::new();

    for edit in &revision.field_edits {
        let previous_value = value_text_at(&draft.report, &edit.path);
        apply_user_input_override(&mut draft, &edit.path, edit.value.clone(), &edit.origin)
            .map_err(LedgerError::InvalidRevision)?;
        let new_value = value_text_at(&draft.report, &edit.path);
        actions.push(ReviewActionRecord {
            action_id: next_action_id(ledger, actions.len() as u32),
            base_version_id,
            resulting_version_id: version_id,
            actor_role: revision.actor_role,
            kind: ReviewActionKind::FieldEdited,
            path: edit.path.clone(),
            previous_value,
            new_value,
            correction_reason: edit.reason,
            note: edit.note.clone(),
        });
    }

    let mut confirmed_review_paths = base_version
        .confirmed_review_paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in &revision.confirmed_review_paths {
        confirmed_review_paths.insert(path.clone());
        actions.push(ReviewActionRecord {
            action_id: next_action_id(ledger, actions.len() as u32),
            base_version_id,
            resulting_version_id: version_id,
            actor_role: revision.actor_role,
            kind: ReviewActionKind::FieldConfirmed,
            path: path.clone(),
            previous_value: value_text_at(&base_version.draft.report, path),
            new_value: value_text_at(&draft.report, path),
            correction_reason: None,
            note: None,
        });
    }

    let validation = validate_draft_report(&draft);
    let feedback = capture_feedback(
        &base_version.draft,
        &draft,
        Some(&base_version.validation),
        &revision.annotations,
        None,
    );
    let version = build_version_record(
        version_id,
        Some(base_version_id),
        DraftVersionSource::ReviewRevision,
        revision.actor_role,
        revision.label,
        draft,
        confirmed_review_paths,
        validation,
        &ledger.bundle,
        Some(feedback),
        None,
    )?;

    ledger.review_actions.extend(actions);
    ledger.summary.current_draft_version_id = version_id;
    ledger.summary.current_state =
        state_from_filing_status(version.review_packet.summary.filing_status);
    ledger.draft_versions.push(version);
    refresh_summary(ledger);
    Ok(version_id)
}

pub fn record_submission_attempt(
    ledger: &mut ReviewSubmissionLedger,
    draft_version_id: u32,
    note: Option<String>,
) -> Result<u32, LedgerError> {
    get_version(ledger, draft_version_id)?;

    let attempt_id = next_attempt_id(ledger);
    ledger.submission_attempts.push(SubmissionAttemptRecord {
        attempt_id,
        draft_version_id,
        status: SubmissionAttemptState::Submitted,
        note,
        submission_feedback: None,
        resulting_version_id: None,
        feedback_capture: None,
    });
    ledger.summary.latest_submission_attempt_id = Some(attempt_id);
    ledger.summary.current_state = LedgerState::Submitted;
    refresh_summary(ledger);
    Ok(attempt_id)
}

pub fn ingest_submission_feedback(
    ledger: &mut ReviewSubmissionLedger,
    attempt_id: u32,
    feedback: SubmissionFeedback,
    corrected_revision: Option<DraftRevisionInput>,
) -> Result<Option<u32>, LedgerError> {
    let attempt_index = ledger
        .submission_attempts
        .iter()
        .position(|attempt| attempt.attempt_id == attempt_id)
        .ok_or(LedgerError::UnknownSubmissionAttempt(attempt_id))?;
    let attempt_draft_version_id = ledger.submission_attempts[attempt_index].draft_version_id;
    let base_version = get_version(ledger, attempt_draft_version_id)?.clone();
    let status = submission_attempt_state(feedback.status);

    let mut resulting_version_id = None;
    let mut attempt_feedback_capture = None;
    let current_state = if let Some(revision) = corrected_revision {
        let version_id = next_version_id(ledger);
        let mut draft = base_version.draft.clone();
        let mut actions = Vec::new();

        for edit in &revision.field_edits {
            let previous_value = value_text_at(&draft.report, &edit.path);
            apply_user_input_override(&mut draft, &edit.path, edit.value.clone(), &edit.origin)
                .map_err(LedgerError::InvalidRevision)?;
            let new_value = value_text_at(&draft.report, &edit.path);
            actions.push(ReviewActionRecord {
                action_id: next_action_id(ledger, actions.len() as u32),
                base_version_id: attempt_draft_version_id,
                resulting_version_id: version_id,
                actor_role: revision.actor_role,
                kind: ReviewActionKind::FieldEdited,
                path: edit.path.clone(),
                previous_value,
                new_value,
                correction_reason: edit.reason,
                note: edit.note.clone(),
            });
        }

        let mut confirmed_review_paths = base_version
            .confirmed_review_paths
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        for path in &revision.confirmed_review_paths {
            confirmed_review_paths.insert(path.clone());
            actions.push(ReviewActionRecord {
                action_id: next_action_id(ledger, actions.len() as u32),
                base_version_id: attempt_draft_version_id,
                resulting_version_id: version_id,
                actor_role: revision.actor_role,
                kind: ReviewActionKind::FieldConfirmed,
                path: path.clone(),
                previous_value: value_text_at(&base_version.draft.report, path),
                new_value: value_text_at(&draft.report, path),
                correction_reason: None,
                note: None,
            });
        }

        let validation = validate_draft_report(&draft);
        let feedback_capture = capture_feedback(
            &base_version.draft,
            &draft,
            Some(&base_version.validation),
            &revision.annotations,
            Some(feedback.clone()),
        );
        let version = build_version_record(
            version_id,
            Some(attempt_draft_version_id),
            DraftVersionSource::SiteReturnRevision,
            revision.actor_role,
            revision.label,
            draft,
            confirmed_review_paths,
            validation,
            &ledger.bundle,
            Some(feedback_capture.clone()),
            Some(attempt_id),
        )?;

        ledger.review_actions.extend(actions);
        ledger.summary.current_draft_version_id = version_id;
        let state = state_from_filing_status(version.review_packet.summary.filing_status);
        ledger.draft_versions.push(version);
        attempt_feedback_capture = Some(feedback_capture);
        resulting_version_id = Some(version_id);
        state
    } else {
        match feedback.status {
            SubmissionStatus::Accepted => LedgerState::Accepted,
            SubmissionStatus::Returned => LedgerState::Returned,
            SubmissionStatus::Rejected => LedgerState::Rejected,
        }
    };

    let attempt = &mut ledger.submission_attempts[attempt_index];
    attempt.status = status;
    attempt.submission_feedback = Some(feedback);
    attempt.resulting_version_id = resulting_version_id;
    attempt.feedback_capture = attempt_feedback_capture;

    ledger.summary.latest_submission_attempt_id = Some(attempt_id);
    ledger.summary.current_state = current_state;
    refresh_summary(ledger);
    Ok(resulting_version_id)
}

pub fn render_review_submission_ledger_json_pretty(
    ledger: &ReviewSubmissionLedger,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(ledger)
}

pub fn render_review_submission_ledger_markdown(ledger: &ReviewSubmissionLedger) -> String {
    let mut lines = vec![
        "# Review Submission Ledger".to_owned(),
        String::new(),
        "## Summary".to_owned(),
        format!("- Bundle ID: {}", ledger.bundle_id),
        format!(
            "- State: {}",
            ledger_state_name(ledger.summary.current_state)
        ),
        format!(
            "- Draft Versions: {}, Review Actions: {}, Submission Attempts: {}",
            ledger.summary.draft_version_count,
            ledger.summary.review_action_count,
            ledger.summary.submission_attempt_count
        ),
        format!(
            "- Current Draft Version: {}",
            ledger.summary.current_draft_version_id
        ),
    ];
    if let Some(attempt_id) = ledger.summary.latest_submission_attempt_id {
        lines.push(format!("- Latest Submission Attempt: {attempt_id}"));
    }
    lines.push(String::new());

    lines.push("## Draft Versions".to_owned());
    for version in &ledger.draft_versions {
        lines.push(format!(
            "- v{} [{}] {}",
            version.version_id,
            draft_version_source_name(version.source),
            version.label
        ));
        lines.push(format!(
            "  State: {}",
            filing_status_name(version.review_packet.summary.filing_status)
        ));
        lines.push(format!(
            "  Readiness: {} automation, {} user input, {} manual review",
            version.review_packet.summary.readiness.automation_gap_count,
            version.review_packet.summary.readiness.user_input_gap_count,
            version.review_packet.summary.readiness.manual_review_count
        ));
    }
    lines.push(String::new());

    lines.push("## Review Actions".to_owned());
    if ledger.review_actions.is_empty() {
        lines.push("- None".to_owned());
    } else {
        for action in &ledger.review_actions {
            lines.push(format!(
                "- [{}] {} ({})",
                review_action_kind_name(action.kind),
                action.path,
                actor_role_name(action.actor_role)
            ));
            if let Some(new_value) = action.new_value.as_deref() {
                lines.push(format!(
                    "  {} -> {}",
                    action.previous_value.as_deref().unwrap_or("[missing]"),
                    new_value
                ));
            }
        }
    }
    lines.push(String::new());

    lines.push("## Submission Attempts".to_owned());
    if ledger.submission_attempts.is_empty() {
        lines.push("- None".to_owned());
    } else {
        for attempt in &ledger.submission_attempts {
            lines.push(format!(
                "- Attempt {} [{}] draft v{}",
                attempt.attempt_id,
                submission_attempt_state_name(attempt.status),
                attempt.draft_version_id
            ));
            if let Some(feedback) = attempt.submission_feedback.as_ref() {
                if let Some(message) = feedback.message.as_deref() {
                    lines.push(format!("  Message: {message}"));
                }
            }
            if let Some(version_id) = attempt.resulting_version_id {
                lines.push(format!("  Resulting draft version: {version_id}"));
            }
        }
    }

    lines.join("\n")
}

fn build_version_record(
    version_id: u32,
    parent_version_id: Option<u32>,
    source: DraftVersionSource,
    actor_role: ActorRole,
    label: String,
    draft: DraftReport,
    confirmed_review_paths: BTreeSet<String>,
    validation: ValidationReport,
    bundle: &CanonicalExpenseBundle,
    feedback_from_parent: Option<FeedbackCapture>,
    source_submission_attempt_id: Option<u32>,
) -> Result<DraftVersionRecord, LedgerError> {
    let readiness =
        summarize_validation_readiness_with_confirmations(&validation, &confirmed_review_paths);
    let review_packet = build_review_packet_with_readiness(bundle, &draft, &readiness)?;
    Ok(DraftVersionRecord {
        version_id,
        parent_version_id,
        source,
        actor_role,
        label,
        confirmed_review_paths: confirmed_review_paths.into_iter().collect(),
        draft,
        validation,
        readiness,
        review_packet,
        feedback_from_parent,
        source_submission_attempt_id,
    })
}

fn get_version(
    ledger: &ReviewSubmissionLedger,
    version_id: u32,
) -> Result<&DraftVersionRecord, LedgerError> {
    ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == version_id)
        .ok_or(LedgerError::UnknownDraftVersion(version_id))
}

fn next_version_id(ledger: &ReviewSubmissionLedger) -> u32 {
    ledger
        .draft_versions
        .last()
        .map_or(1, |version| version.version_id + 1)
}

fn next_action_id(ledger: &ReviewSubmissionLedger, offset: u32) -> u32 {
    ledger
        .review_actions
        .last()
        .map_or(1, |action| action.action_id + 1)
        + offset
}

fn next_attempt_id(ledger: &ReviewSubmissionLedger) -> u32 {
    ledger
        .submission_attempts
        .last()
        .map_or(1, |attempt| attempt.attempt_id + 1)
}

fn refresh_summary(ledger: &mut ReviewSubmissionLedger) {
    ledger.summary.document_count = ledger.bundle.documents.len();
    ledger.summary.draft_version_count = ledger.draft_versions.len();
    ledger.summary.review_action_count = ledger.review_actions.len();
    ledger.summary.submission_attempt_count = ledger.submission_attempts.len();
    ledger.summary.source_filenames = ledger
        .bundle
        .documents
        .iter()
        .map(|document| document.filename.clone())
        .collect();
}

fn state_from_filing_status(status: FilingStatus) -> LedgerState {
    match status {
        FilingStatus::AutomationBlocked => LedgerState::AutomationBlocked,
        FilingStatus::UserInputRequired => LedgerState::UserInputRequired,
        FilingStatus::ManualReviewRequired => LedgerState::ManualReviewRequired,
        FilingStatus::ReadyToFile => LedgerState::ReadyToFile,
    }
}

fn submission_attempt_state(status: SubmissionStatus) -> SubmissionAttemptState {
    match status {
        SubmissionStatus::Accepted => SubmissionAttemptState::Accepted,
        SubmissionStatus::Returned => SubmissionAttemptState::Returned,
        SubmissionStatus::Rejected => SubmissionAttemptState::Rejected,
    }
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
    if normalized_path.is_empty() {
        return Some(value);
    }

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

fn ledger_state_name(state: LedgerState) -> &'static str {
    match state {
        LedgerState::AutomationBlocked => "automation_blocked",
        LedgerState::UserInputRequired => "user_input_required",
        LedgerState::ManualReviewRequired => "manual_review_required",
        LedgerState::ReadyToFile => "ready_to_file",
        LedgerState::Submitted => "submitted",
        LedgerState::Accepted => "accepted",
        LedgerState::Returned => "returned",
        LedgerState::Rejected => "rejected",
    }
}

fn draft_version_source_name(source: DraftVersionSource) -> &'static str {
    match source {
        DraftVersionSource::MachineProjection => "machine_projection",
        DraftVersionSource::ReviewRevision => "review_revision",
        DraftVersionSource::SiteReturnRevision => "site_return_revision",
    }
}

fn actor_role_name(role: ActorRole) -> &'static str {
    match role {
        ActorRole::System => "system",
        ActorRole::FinancialAdministrator => "financial_administrator",
        ActorRole::StanfordSite => "stanford_site",
    }
}

fn review_action_kind_name(kind: ReviewActionKind) -> &'static str {
    match kind {
        ReviewActionKind::FieldEdited => "field_edited",
        ReviewActionKind::FieldConfirmed => "field_confirmed",
    }
}

fn submission_attempt_state_name(status: SubmissionAttemptState) -> &'static str {
    match status {
        SubmissionAttemptState::Submitted => "submitted",
        SubmissionAttemptState::Accepted => "accepted",
        SubmissionAttemptState::Returned => "returned",
        SubmissionAttemptState::Rejected => "rejected",
    }
}

fn filing_status_name(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "automation blocked",
        FilingStatus::UserInputRequired => "user input required",
        FilingStatus::ManualReviewRequired => "manual review required",
        FilingStatus::ReadyToFile => "ready to file",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_review_revision, ingest_submission_feedback, initialize_review_submission_ledger,
        record_submission_attempt, render_review_submission_ledger_markdown, ActorRole,
        DraftRevisionInput, FeedbackCategory, FieldEditInput, LedgerState,
    };
    use crate::bundle_synthesis::{synthesize_bundle_projection_with_fx, StaticFxRateProvider};
    use crate::feedback::{SubmissionFeedback, SubmissionFieldFeedback, SubmissionStatus};
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
    fn ledger_initializes_from_machine_projection() {
        let projection = synthetic_projection();
        let ledger = initialize_review_submission_ledger(
            "synthetic-bundle",
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("ledger should initialize");

        assert_eq!(ledger.summary.current_draft_version_id, 1);
        assert_eq!(ledger.summary.current_state, LedgerState::UserInputRequired);
        assert_eq!(ledger.draft_versions.len(), 1);
        assert_eq!(
            ledger.draft_versions[0]
                .review_packet
                .summary
                .readiness
                .user_input_gap_count,
            4
        );
    }

    #[test]
    fn review_revision_creates_new_version_and_can_resolve_manual_review_by_confirmation() {
        let projection = synthetic_projection();
        let mut ledger = initialize_review_submission_ledger(
            "synthetic-bundle",
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("ledger should initialize");

        let version_id = apply_review_revision(
            &mut ledger,
            1,
            DraftRevisionInput {
                actor_role: ActorRole::FinancialAdministrator,
                label: "FA completed required fields".to_owned(),
                field_edits: vec![
                    FieldEditInput {
                        path: "expense_report.general_information.payee.affiliation".to_owned(),
                        value: ReportValue::from("stanford_staff"),
                        reason: Some(FeedbackCategory::MissingRequiredField),
                        note: None,
                        origin: "ledger.test.affiliation".to_owned(),
                    },
                    FieldEditInput {
                        path: "expense_report.general_information.authorized_by".to_owned(),
                        value: ReportValue::from("Dana Torres"),
                        reason: Some(FeedbackCategory::MissingRequiredField),
                        note: None,
                        origin: "ledger.test.authorized_by".to_owned(),
                    },
                    FieldEditInput {
                        path: "expense_report.transaction_lines[2].meal_details.attendees"
                            .to_owned(),
                        value: ReportValue::array([
                            ReportValue::object([
                                ("name", ReportValue::from("Olivia Park")),
                                ("affiliation", ReportValue::from("Stanford staff")),
                            ]),
                            ReportValue::object([
                                ("name", ReportValue::from("Collaborator A")),
                                ("affiliation", ReportValue::from("External collaborator")),
                            ]),
                        ]),
                        reason: Some(FeedbackCategory::MissingRequiredField),
                        note: None,
                        origin: "ledger.test.attendees".to_owned(),
                    },
                ],
                confirmed_review_paths: vec![
                    "expense_report.transaction_lines[0].common.date".to_owned(),
                    "expense_report.transaction_lines[1].common.date".to_owned(),
                    "expense_report.transaction_lines[2].common.date".to_owned(),
                ],
                annotations: Vec::new(),
            },
        )
        .expect("revision should apply");

        let latest = ledger
            .draft_versions
            .iter()
            .find(|version| version.version_id == version_id)
            .expect("version should exist");
        assert_eq!(ledger.summary.current_state, LedgerState::ReadyToFile);
        assert_eq!(
            latest.review_packet.summary.readiness.user_input_gap_count,
            0
        );
        assert_eq!(
            latest.review_packet.summary.readiness.manual_review_count,
            0
        );
        assert_eq!(ledger.review_actions.len(), 6);
        assert!(latest.feedback_from_parent.is_some());
    }

    #[test]
    fn returned_submission_can_spawn_a_corrected_follow_up_version() {
        let projection = synthetic_projection();
        let mut ledger = initialize_review_submission_ledger(
            "synthetic-bundle",
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("ledger should initialize");

        let ready_version = apply_review_revision(
            &mut ledger,
            1,
            DraftRevisionInput {
                actor_role: ActorRole::FinancialAdministrator,
                label: "FA prepared for submission".to_owned(),
                field_edits: vec![
                    FieldEditInput {
                        path: "expense_report.general_information.payee.affiliation".to_owned(),
                        value: ReportValue::from("stanford_staff"),
                        reason: Some(FeedbackCategory::MissingRequiredField),
                        note: None,
                        origin: "ledger.test.affiliation".to_owned(),
                    },
                    FieldEditInput {
                        path: "expense_report.general_information.authorized_by".to_owned(),
                        value: ReportValue::from("Dana Torres"),
                        reason: Some(FeedbackCategory::MissingRequiredField),
                        note: None,
                        origin: "ledger.test.authorized_by".to_owned(),
                    },
                    FieldEditInput {
                        path: "expense_report.transaction_lines[2].meal_details.attendees"
                            .to_owned(),
                        value: ReportValue::array([
                            ReportValue::object([
                                ("name", ReportValue::from("Olivia Park")),
                                ("affiliation", ReportValue::from("Stanford staff")),
                            ]),
                            ReportValue::object([
                                ("name", ReportValue::from("Collaborator A")),
                                ("affiliation", ReportValue::from("External collaborator")),
                            ]),
                        ]),
                        reason: Some(FeedbackCategory::MissingRequiredField),
                        note: None,
                        origin: "ledger.test.attendees".to_owned(),
                    },
                ],
                confirmed_review_paths: vec![
                    "expense_report.transaction_lines[0].common.date".to_owned(),
                    "expense_report.transaction_lines[1].common.date".to_owned(),
                    "expense_report.transaction_lines[2].common.date".to_owned(),
                ],
                annotations: Vec::new(),
            },
        )
        .expect("revision should apply");
        let attempt_id = record_submission_attempt(
            &mut ledger,
            ready_version,
            Some("First submission".to_owned()),
        )
        .expect("attempt should record");
        assert_eq!(ledger.summary.current_state, LedgerState::Submitted);

        let corrected_version = ingest_submission_feedback(
            &mut ledger,
            attempt_id,
            SubmissionFeedback {
                status: SubmissionStatus::Returned,
                message: Some("Oracle returned one classification field".to_owned()),
                category: Some(FeedbackCategory::StanfordPolicyMismatch),
                returned_fields: vec![SubmissionFieldFeedback {
                    path: Some("expense_report.general_information.event_name".to_owned()),
                    message: "Event title must be more specific".to_owned(),
                    category: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                }],
            },
            Some(DraftRevisionInput {
                actor_role: ActorRole::FinancialAdministrator,
                label: "FA corrected returned draft".to_owned(),
                field_edits: vec![FieldEditInput {
                    path: "expense_report.general_information.event_name".to_owned(),
                    value: ReportValue::from("Singapore Research Travel"),
                    reason: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                    note: None,
                    origin: "ledger.test.returned_event_name".to_owned(),
                }],
                confirmed_review_paths: Vec::new(),
                annotations: vec![crate::CorrectionAnnotation {
                    path: "expense_report.general_information.event_name".to_owned(),
                    reason: FeedbackCategory::StanfordSiteWorkflowMismatch,
                    note: Some("Updated after Oracle return".to_owned()),
                }],
            }),
        )
        .expect("feedback should ingest")
        .expect("corrected version should exist");

        assert_eq!(ledger.summary.current_draft_version_id, corrected_version);
        assert_eq!(ledger.summary.current_state, LedgerState::ReadyToFile);
        let attempt = ledger
            .submission_attempts
            .iter()
            .find(|attempt| attempt.attempt_id == attempt_id)
            .expect("attempt should exist");
        assert_eq!(attempt.status, super::SubmissionAttemptState::Returned);
        assert_eq!(attempt.resulting_version_id, Some(corrected_version));
        assert!(attempt.feedback_capture.is_some());

        let rendered = render_review_submission_ledger_markdown(&ledger);
        assert!(rendered.contains("Review Submission Ledger"));
        assert!(rendered.contains("Attempt 1 [returned]"));
    }
}
