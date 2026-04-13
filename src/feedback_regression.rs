use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;

use crate::bundle_regression::{bundle_regression_cases, run_bundle_regression_case};
use crate::feedback::{
    apply_user_input_override, capture_feedback, render_feedback_capture_json_pretty,
    CorrectionAnnotation, FeedbackCapture, FeedbackCategory, SubmissionFeedback,
    SubmissionFieldFeedback, SubmissionStatus,
};
use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedbackScenario {
    AcceptedCompletion,
    ReturnedClassificationCorrection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackRegressionCase {
    pub id: &'static str,
    pub bundle_case_id: &'static str,
    scenario: FeedbackScenario,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackRegressionFailure {
    pub case_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedbackRegressionVerificationReport {
    pub failures: Vec<FeedbackRegressionFailure>,
}

impl FeedbackRegressionVerificationReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn feedback_regression_root() -> PathBuf {
    repo_root().join("fixtures/feedback_regressions")
}

pub fn feedback_regression_cases() -> Vec<FeedbackRegressionCase> {
    vec![
        FeedbackRegressionCase {
            id: "curated_feedback_acceptance",
            bundle_case_id: "curated_packet_demo_fx",
            scenario: FeedbackScenario::AcceptedCompletion,
        },
        FeedbackRegressionCase {
            id: "curated_alt_feedback_returned",
            bundle_case_id: "curated_alt_packet_demo_fx",
            scenario: FeedbackScenario::ReturnedClassificationCorrection,
        },
    ]
}

pub fn verify_feedback_regressions() -> FeedbackRegressionVerificationReport {
    let mut failures = Vec::new();

    for case in feedback_regression_cases() {
        match verify_feedback_regression_case(&case) {
            Ok(None) => {}
            Ok(Some(message)) => failures.push(FeedbackRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
            Err(message) => failures.push(FeedbackRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
        }
    }

    FeedbackRegressionVerificationReport { failures }
}

pub fn export_feedback_regressions() -> Result<(), String> {
    for case in feedback_regression_cases() {
        let rendered = render_feedback_regression_case(&case)?;
        let path = repo_root().join(expected_feedback_relative_path(case.id));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        fs::write(&path, rendered)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    }

    let manifest_path = feedback_regression_root().join("manifest.json");
    let manifest = feedback_regression_cases()
        .into_iter()
        .map(|case| {
            serde_json::json!({
                "id": case.id,
                "feedback_path": expected_feedback_relative_path(case.id),
            })
        })
        .collect::<Vec<_>>();
    let rendered_manifest = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("failed to render feedback regression manifest: {err}"))?;
    fs::create_dir_all(feedback_regression_root())
        .map_err(|err| format!("failed to create feedback regression dir: {err}"))?;
    fs::write(&manifest_path, rendered_manifest)
        .map_err(|err| format!("failed to write {}: {err}", manifest_path.display()))?;

    Ok(())
}

pub fn run_feedback_regression_case(case: &FeedbackRegressionCase) -> Result<FeedbackCapture, String> {
    let bundle_case = bundle_regression_cases()
        .into_iter()
        .find(|candidate| candidate.id == case.bundle_case_id)
        .ok_or_else(|| format!("missing bundle regression case {:?}", case.bundle_case_id))?;
    let projection = run_bundle_regression_case(&bundle_case)?;
    let mut corrected = projection.draft.clone();

    let (annotations, submission_feedback) = match case.scenario {
        FeedbackScenario::AcceptedCompletion => {
            apply_user_input_override(
                &mut corrected,
                "expense_report.general_information.payee.affiliation",
                ReportValue::from("stanford_staff"),
                "feedback_regression.acceptance.affiliation",
            )?;
            apply_user_input_override(
                &mut corrected,
                "expense_report.general_information.authorized_by",
                ReportValue::from("Dana Torres"),
                "feedback_regression.acceptance.authorized_by",
            )?;
            apply_user_input_override(
                &mut corrected,
                "expense_report.general_information.event_name",
                ReportValue::from("Singapore Research Travel"),
                "feedback_regression.acceptance.event_name",
            )?;
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
                "feedback_regression.acceptance.attendees",
            )?;

            (
                vec![
                    CorrectionAnnotation {
                        path: "expense_report.general_information.payee.affiliation".to_owned(),
                        reason: FeedbackCategory::MissingRequiredField,
                        note: Some("FA filled the missing affiliation".to_owned()),
                    },
                    CorrectionAnnotation {
                        path: "expense_report.general_information.authorized_by".to_owned(),
                        reason: FeedbackCategory::MissingRequiredField,
                        note: Some("Approver confirmed during review".to_owned()),
                    },
                ],
                Some(SubmissionFeedback {
                    status: SubmissionStatus::Accepted,
                    message: Some("Submitted successfully".to_owned()),
                    category: Some(FeedbackCategory::Other),
                    returned_fields: Vec::new(),
                }),
            )
        }
        FeedbackScenario::ReturnedClassificationCorrection => {
            apply_user_input_override(
                &mut corrected,
                "expense_report.general_information.payee.affiliation",
                ReportValue::from("stanford_student"),
                "feedback_regression.returned.affiliation",
            )?;
            apply_user_input_override(
                &mut corrected,
                "expense_report.general_information.authorized_by",
                ReportValue::from("Jamie Lin"),
                "feedback_regression.returned.authorized_by",
            )?;
            apply_user_input_override(
                &mut corrected,
                "expense_report.transaction_lines[2].common.expense_type",
                ReportValue::from("group_travel_meal"),
                "feedback_regression.returned.expense_type",
            )?;
            apply_user_input_override(
                &mut corrected,
                "expense_report.transaction_lines[2].meal_details.attendees",
                ReportValue::array([
                    ReportValue::object([
                        ("name", ReportValue::from("Olivia Park")),
                        ("affiliation", ReportValue::from("Stanford student")),
                    ]),
                    ReportValue::object([
                        ("name", ReportValue::from("Jamie Lin")),
                        ("affiliation", ReportValue::from("Stanford faculty")),
                    ]),
                ]),
                "feedback_regression.returned.attendees",
            )?;

            (
                vec![
                    CorrectionAnnotation {
                        path: "expense_report.transaction_lines[2].common.expense_type".to_owned(),
                        reason: FeedbackCategory::WrongExpenseTypeClassification,
                        note: Some("Receipt should have been grouped as a team meal".to_owned()),
                    },
                    CorrectionAnnotation {
                        path: "expense_report.general_information.authorized_by".to_owned(),
                        reason: FeedbackCategory::MissingRequiredField,
                        note: Some("Approver was missing from the original draft".to_owned()),
                    },
                ],
                Some(SubmissionFeedback {
                    status: SubmissionStatus::Returned,
                    message: Some("Oracle returned two fields".to_owned()),
                    category: Some(FeedbackCategory::StanfordPolicyMismatch),
                    returned_fields: vec![
                        SubmissionFieldFeedback {
                            path: Some(
                                "expense_report.general_information.authorized_by".to_owned(),
                            ),
                            message: "Approver must be delegated for this PTA".to_owned(),
                            category: Some(FeedbackCategory::StanfordPolicyMismatch),
                        },
                        SubmissionFieldFeedback {
                            path: Some(
                                "expense_report.transaction_lines[2].common.expense_type"
                                    .to_owned(),
                            ),
                            message: "Meal should be filed as a group-travel meal".to_owned(),
                            category: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                        },
                    ],
                }),
            )
        }
    };

    Ok(capture_feedback(
        &projection.draft,
        &corrected,
        Some(&projection.validation),
        &annotations,
        submission_feedback,
    ))
}

fn verify_feedback_regression_case(case: &FeedbackRegressionCase) -> Result<Option<String>, String> {
    let actual = parse_rendered_json(&render_feedback_regression_case(case)?)
        .map_err(|err| format!("failed to parse rendered feedback json: {err}"))?;
    let expected = load_expected_json(expected_feedback_relative_path(case.id))?;

    if expected == actual {
        Ok(None)
    } else {
        Ok(Some(format!(
            "feedback artifact mismatch\nEXPECTED:\n{}\nACTUAL:\n{}",
            render_json_value(&expected),
            render_json_value(&actual)
        )))
    }
}

fn render_feedback_regression_case(case: &FeedbackRegressionCase) -> Result<String, String> {
    let capture = run_feedback_regression_case(case)?;
    render_feedback_capture_json_pretty(&capture)
        .map_err(|err| format!("failed to render feedback capture json: {err}"))
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

fn expected_feedback_relative_path(case_id: &str) -> String {
    format!("fixtures/feedback_regressions/{case_id}.feedback.json")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{feedback_regression_cases, verify_feedback_regressions};

    #[test]
    fn feedback_regression_case_ids_are_unique() {
        let mut ids = BTreeSet::new();
        for case in feedback_regression_cases() {
            assert!(
                ids.insert(case.id),
                "duplicate feedback regression case id {:?}",
                case.id
            );
        }
    }

    #[test]
    fn feedback_regression_suite_is_clean() {
        let report = verify_feedback_regressions();
        assert!(
            report.is_clean(),
            "feedback regression failures: {:#?}",
            report.failures
        );
    }
}
