use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ledger::{
    apply_review_revision, render_review_submission_ledger_json_pretty, DraftRevisionInput,
    DraftVersionRecord, ReviewSubmissionLedger,
};
use crate::render::{render_draft_report_yaml, RenderDraftReportError};
use crate::review_packet::FilingStatus;
use crate::review_packet::render_review_packet_json_pretty;
use crate::review_workbench::render_review_workbench_html;
use crate::validator::render_validation_report_json_pretty;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSessionSaveResult {
    pub version_id: u32,
    pub ledger_state: String,
    pub filing_status: String,
    pub automation_gap_count: usize,
    pub user_input_gap_count: usize,
    pub manual_review_count: usize,
    pub other_warning_count: usize,
    pub issue_count: usize,
}

#[derive(Debug)]
pub enum ReviewSessionError {
    Io(std::io::Error),
    Json(serde_json::Error),
    RenderDraft(RenderDraftReportError),
    Ledger(String),
    MissingCurrentVersion(u32),
}

impl fmt::Display for ReviewSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::RenderDraft(err) => write!(f, "{err}"),
            Self::Ledger(message) => write!(f, "{message}"),
            Self::MissingCurrentVersion(version_id) => {
                write!(f, "current draft version {version_id} was not found in ledger")
            }
        }
    }
}

impl std::error::Error for ReviewSessionError {}

impl From<std::io::Error> for ReviewSessionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ReviewSessionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<RenderDraftReportError> for ReviewSessionError {
    fn from(value: RenderDraftReportError) -> Self {
        Self::RenderDraft(value)
    }
}

pub fn load_review_submission_ledger_path(
    path: impl AsRef<Path>,
) -> Result<ReviewSubmissionLedger, ReviewSessionError> {
    let path = path.as_ref();
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn apply_review_revision_at_artifacts_dir(
    artifacts_dir: impl AsRef<Path>,
    revision: DraftRevisionInput,
    base_version_id: Option<u32>,
) -> Result<ReviewSessionSaveResult, ReviewSessionError> {
    let artifacts_dir = artifacts_dir.as_ref();
    let ledger_path = artifacts_dir.join("ledger.json");
    let mut ledger = load_review_submission_ledger_path(&ledger_path)?;
    let base_version_id = base_version_id.unwrap_or(ledger.summary.current_draft_version_id);
    let version_id = apply_review_revision(&mut ledger, base_version_id, revision)
        .map_err(|err| ReviewSessionError::Ledger(err.to_string()))?;
    write_current_review_artifacts(artifacts_dir, &ledger)?;
    Ok(summary_for_version(&ledger, version_id)?)
}

pub fn write_current_review_artifacts(
    artifacts_dir: impl AsRef<Path>,
    ledger: &ReviewSubmissionLedger,
) -> Result<(), ReviewSessionError> {
    let artifacts_dir = artifacts_dir.as_ref();
    fs::create_dir_all(artifacts_dir)?;
    let current = current_version(ledger)?;
    write_review_version_artifacts(artifacts_dir, current, ledger)?;

    let version_dir = artifacts_dir
        .join("review_versions")
        .join(format!("v{}", current.version_id));
    fs::create_dir_all(&version_dir)?;
    write_review_version_artifacts(&version_dir, current, ledger)?;
    Ok(())
}

fn write_review_version_artifacts(
    output_dir: &Path,
    version: &DraftVersionRecord,
    ledger: &ReviewSubmissionLedger,
) -> Result<(), ReviewSessionError> {
    fs::write(output_dir.join("draft.yaml"), render_draft_report_yaml(&version.draft)?)?;
    fs::write(
        output_dir.join("validation.json"),
        render_validation_report_json_pretty(&version.validation)?,
    )?;
    fs::write(
        output_dir.join("readiness.json"),
        serde_json::to_string_pretty(&version.readiness)?,
    )?;
    fs::write(
        output_dir.join("review_packet.json"),
        render_review_packet_json_pretty(&version.review_packet)
            .map_err(|err| ReviewSessionError::Ledger(err.to_string()))?,
    )?;
    fs::write(
        output_dir.join("review_workbench.html"),
        render_review_workbench_html(&version.review_packet),
    )?;
    fs::write(
        output_dir.join("ledger.json"),
        render_review_submission_ledger_json_pretty(ledger)?,
    )?;
    Ok(())
}

fn current_version(ledger: &ReviewSubmissionLedger) -> Result<&DraftVersionRecord, ReviewSessionError> {
    ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == ledger.summary.current_draft_version_id)
        .ok_or(ReviewSessionError::MissingCurrentVersion(
            ledger.summary.current_draft_version_id,
        ))
}

fn summary_for_version(
    ledger: &ReviewSubmissionLedger,
    version_id: u32,
) -> Result<ReviewSessionSaveResult, ReviewSessionError> {
    let version = ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == version_id)
        .ok_or(ReviewSessionError::MissingCurrentVersion(version_id))?;
    Ok(ReviewSessionSaveResult {
        version_id,
        ledger_state: ledger_state_name(ledger.summary.current_state).to_owned(),
        filing_status: filing_status_name(version.review_packet.summary.filing_status).to_owned(),
        automation_gap_count: version.readiness.automation_gap_count(),
        user_input_gap_count: version.readiness.user_input_required_count(),
        manual_review_count: version.readiness.manual_review_count(),
        other_warning_count: version.readiness.other_warning_count(),
        issue_count: version.review_packet.issues_queue.len(),
    })
}

fn ledger_state_name(state: crate::ledger::LedgerState) -> &'static str {
    match state {
        crate::ledger::LedgerState::AutomationBlocked => "automation_blocked",
        crate::ledger::LedgerState::UserInputRequired => "user_input_required",
        crate::ledger::LedgerState::ManualReviewRequired => "manual_review_required",
        crate::ledger::LedgerState::ReadyToFile => "ready_to_file",
        crate::ledger::LedgerState::Submitted => "submitted",
        crate::ledger::LedgerState::Accepted => "accepted",
        crate::ledger::LedgerState::Returned => "returned",
        crate::ledger::LedgerState::Rejected => "rejected",
    }
}

fn filing_status_name(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "automation_blocked",
        FilingStatus::UserInputRequired => "user_input_required",
        FilingStatus::ManualReviewRequired => "manual_review_required",
        FilingStatus::ReadyToFile => "ready_to_file",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::bundle_synthesis::{synthesize_bundle_projection_with_fx, StaticFxRateProvider};
    use crate::ledger::{initialize_review_submission_ledger, ActorRole, DraftRevisionInput, FieldEditInput};
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};
    use crate::value::ReportValue;

    use super::{apply_review_revision_at_artifacts_dir, write_current_review_artifacts};

    fn synthetic_ledger() -> crate::ReviewSubmissionLedger {
        let documents = generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect::<Vec<_>>();
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&documents, &provider);
        initialize_review_submission_ledger(
            "review-session-test",
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("ledger should initialize")
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should advance")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}_{nanos}"));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    #[test]
    fn writing_current_review_artifacts_emits_latest_aliases_and_versioned_copy() {
        let ledger = synthetic_ledger();
        let temp_dir = unique_temp_dir("review_session_artifacts");
        write_current_review_artifacts(&temp_dir, &ledger).expect("artifacts should be written");

        assert!(temp_dir.join("draft.yaml").exists());
        assert!(temp_dir.join("review_packet.json").exists());
        assert!(temp_dir.join("review_workbench.html").exists());
        assert!(temp_dir.join("ledger.json").exists());
        assert!(temp_dir.join("review_versions/v1/draft.yaml").exists());
    }

    #[test]
    fn apply_review_revision_updates_current_aliases() {
        let ledger = synthetic_ledger();
        let temp_dir = unique_temp_dir("review_session_apply");
        write_current_review_artifacts(&temp_dir, &ledger).expect("artifacts should be written");

        let revision = DraftRevisionInput {
            actor_role: ActorRole::FinancialAdministrator,
            label: "FA saved revision".to_owned(),
            field_edits: vec![FieldEditInput {
                path: "expense_report.general_information.authorized_by".to_owned(),
                value: ReportValue::from("Dana Torres"),
                reason: None,
                note: Some("Filled during review".to_owned()),
                origin: "local_app.review_save".to_owned(),
            }],
            confirmed_review_paths: Vec::new(),
            annotations: Vec::new(),
        };

        let summary = apply_review_revision_at_artifacts_dir(&temp_dir, revision, None)
            .expect("revision should apply");

        assert_eq!(summary.version_id, 2);
        let ledger_value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(temp_dir.join("ledger.json")).expect("ledger should exist"))
                .expect("ledger json should parse");
        assert_eq!(ledger_value["summary"]["current_draft_version_id"], 2);
        assert!(temp_dir.join("review_versions/v2/review_workbench.html").exists());
    }
}
