use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;

use crate::bundle_regression::{bundle_regression_cases, run_bundle_regression_case};
use crate::feedback::{
    FeedbackCategory, SubmissionFeedback, SubmissionFieldFeedback, SubmissionStatus,
};
use crate::ledger::{
    apply_review_revision, ingest_submission_feedback, initialize_review_submission_ledger,
    record_submission_attempt, render_review_submission_ledger_json_pretty, ActorRole,
    DraftRevisionInput, FieldEditInput, ReviewSubmissionLedger,
};
use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedgerScenario {
    AcceptedSubmission,
    ReturnedAndCorrected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRegressionCase {
    pub id: &'static str,
    pub bundle_case_id: &'static str,
    scenario: LedgerScenario,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRegressionFailure {
    pub case_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerRegressionVerificationReport {
    pub failures: Vec<LedgerRegressionFailure>,
}

impl LedgerRegressionVerificationReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn ledger_regression_root() -> PathBuf {
    repo_root().join("fixtures/ledger_regressions")
}

pub fn ledger_regression_cases() -> Vec<LedgerRegressionCase> {
    vec![
        LedgerRegressionCase {
            id: "curated_ledger_accepted",
            bundle_case_id: "curated_packet_demo_fx",
            scenario: LedgerScenario::AcceptedSubmission,
        },
        LedgerRegressionCase {
            id: "curated_alt_ledger_returned",
            bundle_case_id: "curated_alt_packet_demo_fx",
            scenario: LedgerScenario::ReturnedAndCorrected,
        },
    ]
}

pub fn verify_ledger_regressions() -> LedgerRegressionVerificationReport {
    let mut failures = Vec::new();

    for case in ledger_regression_cases() {
        match verify_ledger_regression_case(&case) {
            Ok(None) => {}
            Ok(Some(message)) => failures.push(LedgerRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
            Err(message) => failures.push(LedgerRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
        }
    }

    LedgerRegressionVerificationReport { failures }
}

pub fn export_ledger_regressions() -> Result<(), String> {
    for case in ledger_regression_cases() {
        let rendered = render_ledger_regression_case(&case)?;
        let path = repo_root().join(expected_ledger_relative_path(case.id));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        fs::write(&path, rendered)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    }

    let manifest_path = ledger_regression_root().join("manifest.json");
    let manifest = ledger_regression_cases()
        .into_iter()
        .map(|case| {
            serde_json::json!({
                "id": case.id,
                "ledger_path": expected_ledger_relative_path(case.id),
            })
        })
        .collect::<Vec<_>>();
    let rendered_manifest = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("failed to render ledger regression manifest: {err}"))?;
    fs::create_dir_all(ledger_regression_root())
        .map_err(|err| format!("failed to create ledger regression dir: {err}"))?;
    fs::write(&manifest_path, rendered_manifest)
        .map_err(|err| format!("failed to write {}: {err}", manifest_path.display()))?;

    Ok(())
}

pub fn run_ledger_regression_case(
    case: &LedgerRegressionCase,
) -> Result<ReviewSubmissionLedger, String> {
    let bundle_case = bundle_regression_cases()
        .into_iter()
        .find(|candidate| candidate.id == case.bundle_case_id)
        .ok_or_else(|| format!("missing bundle regression case {:?}", case.bundle_case_id))?;
    let projection = run_bundle_regression_case(&bundle_case)?;
    let mut ledger = initialize_review_submission_ledger(
        case.bundle_case_id,
        &projection.bundle,
        &projection.draft,
        &projection.validation,
    )
    .map_err(|err| format!("failed to initialize ledger: {err}"))?;

    let ready_revision = build_ready_revision(&ledger);
    let ready_version_id = apply_review_revision(&mut ledger, 1, ready_revision)
        .map_err(|err| format!("failed to apply ready revision: {err}"))?;
    let attempt_id = record_submission_attempt(
        &mut ledger,
        ready_version_id,
        Some("Automated regression submission".to_owned()),
    )
    .map_err(|err| format!("failed to record submission attempt: {err}"))?;

    match case.scenario {
        LedgerScenario::AcceptedSubmission => {
            ingest_submission_feedback(
                &mut ledger,
                attempt_id,
                SubmissionFeedback {
                    status: SubmissionStatus::Accepted,
                    message: Some("Oracle accepted the submission".to_owned()),
                    category: Some(FeedbackCategory::Other),
                    returned_fields: Vec::new(),
                },
                None,
            )
            .map_err(|err| format!("failed to ingest accepted feedback: {err}"))?;
        }
        LedgerScenario::ReturnedAndCorrected => {
            let returned_path = "expense_report.general_information.event_name".to_owned();
            let return_revision = build_return_revision(&ledger, &returned_path);
            ingest_submission_feedback(
                &mut ledger,
                attempt_id,
                SubmissionFeedback {
                    status: SubmissionStatus::Returned,
                    message: Some("Oracle returned the event name".to_owned()),
                    category: Some(FeedbackCategory::StanfordPolicyMismatch),
                    returned_fields: vec![SubmissionFieldFeedback {
                        path: Some(returned_path.clone()),
                        message: "Event title must be more specific".to_owned(),
                        category: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                    }],
                },
                Some(return_revision),
            )
            .map_err(|err| format!("failed to ingest returned feedback: {err}"))?;
        }
    }

    Ok(ledger)
}

fn verify_ledger_regression_case(case: &LedgerRegressionCase) -> Result<Option<String>, String> {
    let actual = parse_rendered_json(&render_ledger_regression_case(case)?)
        .map_err(|err| format!("failed to parse rendered ledger json: {err}"))?;
    let expected = load_expected_json(expected_ledger_relative_path(case.id))?;

    if expected == actual {
        Ok(None)
    } else {
        Ok(Some(format!(
            "ledger artifact mismatch\nEXPECTED:\n{}\nACTUAL:\n{}",
            render_json_value(&expected),
            render_json_value(&actual)
        )))
    }
}

fn render_ledger_regression_case(case: &LedgerRegressionCase) -> Result<String, String> {
    let ledger = run_ledger_regression_case(case)?;
    render_review_submission_ledger_json_pretty(&ledger)
        .map_err(|err| format!("failed to render ledger json: {err}"))
}

fn build_ready_revision(ledger: &ReviewSubmissionLedger) -> DraftRevisionInput {
    let current_version = current_version(ledger);
    let payee_name = value_text_at(
        &current_version.draft.report,
        "expense_report.general_information.payee.name",
    )
    .unwrap_or_else(|| "Traveler".to_owned());
    let event_name = value_text_at(
        &current_version.draft.report,
        "expense_report.general_information.event_name",
    )
    .unwrap_or_else(|| "Reviewed Event".to_owned());

    let mut field_edits = Vec::new();
    if value_text_at(
        &current_version.draft.report,
        "expense_report.general_information.payee.affiliation",
    )
    .is_none()
    {
        field_edits.push(FieldEditInput {
            path: "expense_report.general_information.payee.affiliation".to_owned(),
            value: ReportValue::from("stanford_staff"),
            reason: Some(FeedbackCategory::MissingRequiredField),
            note: Some("Synthetic FA filled affiliation".to_owned()),
            origin: "ledger_regression.ready.affiliation".to_owned(),
        });
    }
    if value_text_at(
        &current_version.draft.report,
        "expense_report.general_information.authorized_by",
    )
    .is_none()
    {
        field_edits.push(FieldEditInput {
            path: "expense_report.general_information.authorized_by".to_owned(),
            value: ReportValue::from("Dana Torres"),
            reason: Some(FeedbackCategory::MissingRequiredField),
            note: Some("Synthetic FA filled approver".to_owned()),
            origin: "ledger_regression.ready.authorized_by".to_owned(),
        });
    }
    if value_text_at(
        &current_version.draft.report,
        "expense_report.transaction_lines[2].meal_details.attendees",
    )
    .is_none()
    {
        field_edits.push(FieldEditInput {
            path: "expense_report.transaction_lines[2].meal_details.attendees".to_owned(),
            value: ReportValue::array([
                ReportValue::object([
                    ("name", ReportValue::from(payee_name.clone())),
                    ("affiliation", ReportValue::from("Stanford staff")),
                ]),
                ReportValue::object([
                    ("name", ReportValue::from("Collaborator A")),
                    ("affiliation", ReportValue::from("External collaborator")),
                ]),
            ]),
            reason: Some(FeedbackCategory::MissingRequiredField),
            note: Some("Synthetic FA added attendees".to_owned()),
            origin: "ledger_regression.ready.attendees".to_owned(),
        });
    }

    let confirmed_review_paths = current_version
        .readiness
        .issues
        .iter()
        .filter(|issue| issue.class == crate::ReadinessIssueClass::ManualReview)
        .map(|issue| issue.path.clone())
        .collect::<Vec<_>>();

    DraftRevisionInput {
        actor_role: ActorRole::FinancialAdministrator,
        label: format!("Prepare for submission: {event_name}"),
        field_edits,
        confirmed_review_paths,
        annotations: Vec::new(),
    }
}

fn build_return_revision(
    ledger: &ReviewSubmissionLedger,
    returned_path: &str,
) -> DraftRevisionInput {
    let current_version = current_version(ledger);
    let current_value = value_text_at(&current_version.draft.report, returned_path)
        .unwrap_or_else(|| "Returned Event".to_owned());
    let corrected_value = format!("{current_value} Reviewed");

    DraftRevisionInput {
        actor_role: ActorRole::FinancialAdministrator,
        label: "Correct returned field".to_owned(),
        field_edits: vec![FieldEditInput {
            path: returned_path.to_owned(),
            value: ReportValue::from(corrected_value),
            reason: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
            note: Some("Synthetic correction after Oracle return".to_owned()),
            origin: "ledger_regression.returned.correction".to_owned(),
        }],
        confirmed_review_paths: Vec::new(),
        annotations: vec![crate::CorrectionAnnotation {
            path: returned_path.to_owned(),
            reason: FeedbackCategory::StanfordSiteWorkflowMismatch,
            note: Some("Revised after site return".to_owned()),
        }],
    }
}

fn current_version(ledger: &ReviewSubmissionLedger) -> &crate::DraftVersionRecord {
    ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == ledger.summary.current_draft_version_id)
        .expect("current draft version should exist")
}

fn value_text_at(report: &ReportValue, path: &str) -> Option<String> {
    value_at(report, path).and_then(report_value_to_string)
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

fn load_expected_json(relative_path: String) -> Result<JsonValue, String> {
    let path = repo_root().join(relative_path);
    let value = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&value).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn parse_rendered_json(value: &str) -> Result<JsonValue, serde_json::Error> {
    serde_json::from_str(value)
}

fn render_json_value(value: &JsonValue) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|err| format!("unable to render json value: {err}"))
}

fn expected_ledger_relative_path(case_id: &str) -> String {
    format!("fixtures/ledger_regressions/{case_id}.ledger.json")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ledger_regression_cases, verify_ledger_regressions};

    #[test]
    fn ledger_regression_case_ids_are_unique() {
        let mut ids = BTreeSet::new();
        for case in ledger_regression_cases() {
            assert!(
                ids.insert(case.id),
                "duplicate ledger regression case id {:?}",
                case.id
            );
        }
    }

    #[test]
    fn ledger_regression_suite_is_clean() {
        let report = verify_ledger_regressions();
        assert!(
            report.is_clean(),
            "ledger regression failures: {:#?}",
            report.failures
        );
    }
}
