use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::bundle_synthesis::{CanonicalExpenseKind, StaticFxRateProvider};
use crate::document_extract::extract_document_facts;
use crate::feedback::{
    FeedbackCategory, SubmissionFeedback, SubmissionFieldFeedback, SubmissionStatus,
};
use crate::ledger::{
    apply_review_revision, ingest_submission_feedback, initialize_review_submission_ledger,
    record_submission_attempt, render_review_submission_ledger_markdown, ActorRole,
    DraftRevisionInput, FieldEditInput, LedgerState, ReviewSubmissionLedger,
};
use crate::readiness::ReadinessIssueClass;
use crate::review_packet::{build_review_packet, FilingStatus};
use crate::review_workbench::render_review_workbench_html;
use crate::synthetic_corpus::{generate_synthetic_corpus, SyntheticLedgerScenario};
use crate::transcribe::{TranscribedDocument, TranscribedPage, TranscriptionEngine};
use crate::value::ReportValue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntheticCorpusEvaluationFailure {
    pub packet_id: String,
    pub stage: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntheticCorpusEvaluationSummary {
    pub packet_count: usize,
    pub document_count: usize,
    pub exact_fact_match_count: usize,
    pub extraction_contract_success_count: usize,
    pub bundle_projection_count: usize,
    pub review_packet_count: usize,
    pub workbench_render_count: usize,
    pub ledger_initialized_count: usize,
    pub ready_to_file_count: usize,
    pub accepted_submission_count: usize,
    pub returned_correction_count: usize,
    pub final_state_counts: BTreeMap<String, usize>,
    pub filing_status_counts: BTreeMap<String, usize>,
    pub destination_counts: BTreeMap<String, usize>,
    pub currency_counts: BTreeMap<String, usize>,
    pub scenario_counts: BTreeMap<String, usize>,
    pub variant_counts: BTreeMap<String, usize>,
    pub trip_window_mode_counts: BTreeMap<String, usize>,
    pub stay_window_mode_counts: BTreeMap<String, usize>,
    pub hotel_total_mode_counts: BTreeMap<String, usize>,
    pub receipt_total_mode_counts: BTreeMap<String, usize>,
    pub alcohol_receipt_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntheticCorpusEvaluationReport {
    pub summary: SyntheticCorpusEvaluationSummary,
    pub failures: Vec<SyntheticCorpusEvaluationFailure>,
}

impl SyntheticCorpusEvaluationReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn evaluate_synthetic_corpus(packet_count: usize) -> SyntheticCorpusEvaluationReport {
    let packets = generate_synthetic_corpus(packet_count);
    let fx_provider = StaticFxRateProvider::demo();
    let mut summary = SyntheticCorpusEvaluationSummary::default();
    let mut failures = Vec::new();

    for packet in packets {
        summary.packet_count += 1;
        *summary
            .destination_counts
            .entry(packet.destination_city.clone())
            .or_insert(0) += 1;
        *summary
            .currency_counts
            .entry(packet.currency.clone())
            .or_insert(0) += 1;
        *summary
            .scenario_counts
            .entry(packet.scenario.as_str().to_owned())
            .or_insert(0) += 1;
        *summary
            .variant_counts
            .entry(packet.variant.clone())
            .or_insert(0) += 1;
        *summary
            .trip_window_mode_counts
            .entry(packet.trip_window_mode.as_str().to_owned())
            .or_insert(0) += 1;
        *summary
            .stay_window_mode_counts
            .entry(packet.stay_window_mode.as_str().to_owned())
            .or_insert(0) += 1;
        *summary
            .hotel_total_mode_counts
            .entry(packet.hotel_total_mode.as_str().to_owned())
            .or_insert(0) += 1;
        *summary
            .receipt_total_mode_counts
            .entry(packet.receipt_total_mode.as_str().to_owned())
            .or_insert(0) += 1;
        if packet.alcohol_receipt {
            summary.alcohol_receipt_count += 1;
        }

        let mut extracted_documents = Vec::new();
        let mut packet_failed = false;

        for fixture in &packet.fixtures {
            summary.document_count += 1;
            let transcribed = fixture_to_transcribed_document(&packet.packet_id, fixture);
            let actual = extract_document_facts(&transcribed);
            if actual != fixture.expected_facts {
                failures.push(SyntheticCorpusEvaluationFailure {
                    packet_id: packet.packet_id.clone(),
                    stage: format!("extract:{}", fixture.kind.as_str()),
                    message: format!(
                        "extracted facts did not match expected facts for {}",
                        fixture.filename
                    ),
                });
                packet_failed = true;
                continue;
            }
            summary.exact_fact_match_count += 1;

            if let Err(err) = actual.validate_contract() {
                failures.push(SyntheticCorpusEvaluationFailure {
                    packet_id: packet.packet_id.clone(),
                    stage: format!("contract:{}", fixture.kind.as_str()),
                    message: format!("fact contract validation failed: {err}"),
                });
                packet_failed = true;
                continue;
            }
            summary.extraction_contract_success_count += 1;
            extracted_documents.push(actual);
        }

        if packet_failed {
            continue;
        }

        let projection = crate::bundle_synthesis::synthesize_bundle_projection_with_fx(
            &extracted_documents,
            &fx_provider,
        );
        summary.bundle_projection_count += 1;

        let review_packet = match build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        ) {
            Ok(packet_view) => packet_view,
            Err(err) => {
                failures.push(SyntheticCorpusEvaluationFailure {
                    packet_id: packet.packet_id.clone(),
                    stage: "review_packet".to_owned(),
                    message: format!("failed to build review packet: {err}"),
                });
                continue;
            }
        };
        summary.review_packet_count += 1;
        *summary
            .filing_status_counts
            .entry(filing_status_name(review_packet.summary.filing_status).to_owned())
            .or_insert(0) += 1;

        let rendered_workbench = render_review_workbench_html(&review_packet);
        if !rendered_workbench.contains("<!DOCTYPE html>") {
            failures.push(SyntheticCorpusEvaluationFailure {
                packet_id: packet.packet_id.clone(),
                stage: "review_workbench".to_owned(),
                message: "rendered workbench did not contain an html doctype".to_owned(),
            });
            continue;
        }
        summary.workbench_render_count += 1;

        let mut ledger = match initialize_review_submission_ledger(
            &packet.packet_id,
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        ) {
            Ok(ledger) => ledger,
            Err(err) => {
                failures.push(SyntheticCorpusEvaluationFailure {
                    packet_id: packet.packet_id.clone(),
                    stage: "ledger_init".to_owned(),
                    message: format!("failed to initialize ledger: {err}"),
                });
                continue;
            }
        };
        summary.ledger_initialized_count += 1;

        let ready_revision = build_ready_revision(&ledger);
        let ready_version_id = match apply_review_revision(&mut ledger, 1, ready_revision) {
            Ok(version_id) => version_id,
            Err(err) => {
                failures.push(SyntheticCorpusEvaluationFailure {
                    packet_id: packet.packet_id.clone(),
                    stage: "ledger_ready_revision".to_owned(),
                    message: format!("failed to apply ready revision: {err}"),
                });
                continue;
            }
        };

        if ledger.summary.current_state != LedgerState::ReadyToFile {
            failures.push(SyntheticCorpusEvaluationFailure {
                packet_id: packet.packet_id.clone(),
                stage: "ledger_ready_state".to_owned(),
                message: format!(
                    "expected ready_to_file after FA revision, found {:?}",
                    ledger.summary.current_state
                ),
            });
            continue;
        }
        summary.ready_to_file_count += 1;

        let attempt_id = match record_submission_attempt(
            &mut ledger,
            ready_version_id,
            Some("Synthetic corpus submission".to_owned()),
        ) {
            Ok(attempt_id) => attempt_id,
            Err(err) => {
                failures.push(SyntheticCorpusEvaluationFailure {
                    packet_id: packet.packet_id.clone(),
                    stage: "ledger_submit".to_owned(),
                    message: format!("failed to record submission attempt: {err}"),
                });
                continue;
            }
        };

        let submission_result = match packet.scenario {
            SyntheticLedgerScenario::Accepted => ingest_submission_feedback(
                &mut ledger,
                attempt_id,
                SubmissionFeedback {
                    status: SubmissionStatus::Accepted,
                    message: Some("Synthetic site accepted submission".to_owned()),
                    category: Some(FeedbackCategory::Other),
                    returned_fields: Vec::new(),
                },
                None,
            ),
            SyntheticLedgerScenario::ReturnedAndCorrected => {
                let returned_path = "expense_report.general_information.event_name".to_owned();
                let correction = build_return_revision(&ledger, &returned_path);
                ingest_submission_feedback(
                    &mut ledger,
                    attempt_id,
                    SubmissionFeedback {
                        status: SubmissionStatus::Returned,
                        message: Some(
                            "Synthetic site requested a more specific event name".to_owned(),
                        ),
                        category: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                        returned_fields: vec![SubmissionFieldFeedback {
                            path: Some(returned_path.clone()),
                            message: "Event name needs a more specific description".to_owned(),
                            category: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                        }],
                    },
                    Some(correction),
                )
            }
        };

        if let Err(err) = submission_result {
            failures.push(SyntheticCorpusEvaluationFailure {
                packet_id: packet.packet_id.clone(),
                stage: "ledger_feedback".to_owned(),
                message: format!("failed to ingest submission feedback: {err}"),
            });
            continue;
        }

        match packet.scenario {
            SyntheticLedgerScenario::Accepted => {
                if ledger.summary.current_state != LedgerState::Accepted {
                    failures.push(SyntheticCorpusEvaluationFailure {
                        packet_id: packet.packet_id.clone(),
                        stage: "ledger_final_state".to_owned(),
                        message: format!(
                            "expected accepted state, found {:?}",
                            ledger.summary.current_state
                        ),
                    });
                    continue;
                }
                summary.accepted_submission_count += 1;
            }
            SyntheticLedgerScenario::ReturnedAndCorrected => {
                if ledger.summary.current_state != LedgerState::ReadyToFile {
                    failures.push(SyntheticCorpusEvaluationFailure {
                        packet_id: packet.packet_id.clone(),
                        stage: "ledger_return_correction_state".to_owned(),
                        message: format!(
                            "expected ready_to_file after returned correction, found {:?}",
                            ledger.summary.current_state
                        ),
                    });
                    continue;
                }
                let latest_attempt = ledger
                    .submission_attempts
                    .last()
                    .expect("attempt should exist after submission");
                if latest_attempt.feedback_capture.is_none() {
                    failures.push(SyntheticCorpusEvaluationFailure {
                        packet_id: packet.packet_id.clone(),
                        stage: "ledger_return_feedback_capture".to_owned(),
                        message: "returned submission did not capture feedback details".to_owned(),
                    });
                    continue;
                }
                summary.returned_correction_count += 1;
            }
        }

        let markdown = render_review_submission_ledger_markdown(&ledger);
        if !markdown.contains(&packet.packet_id) {
            failures.push(SyntheticCorpusEvaluationFailure {
                packet_id: packet.packet_id.clone(),
                stage: "ledger_markdown".to_owned(),
                message: "rendered ledger markdown did not contain packet id".to_owned(),
            });
            continue;
        }

        *summary
            .final_state_counts
            .entry(ledger_state_name(ledger.summary.current_state).to_owned())
            .or_insert(0) += 1;
    }

    SyntheticCorpusEvaluationReport { summary, failures }
}

pub fn render_synthetic_corpus_evaluation_json_pretty(
    report: &SyntheticCorpusEvaluationReport,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn render_synthetic_corpus_evaluation_markdown(
    report: &SyntheticCorpusEvaluationReport,
) -> String {
    let mut lines = vec![
        "# Synthetic Corpus Evaluation".to_owned(),
        String::new(),
        "## Summary".to_owned(),
        format!("- Packets: {}", report.summary.packet_count),
        format!("- Documents: {}", report.summary.document_count),
        format!(
            "- Exact fact matches: {} / {}",
            report.summary.exact_fact_match_count, report.summary.document_count
        ),
        format!(
            "- Review packets: {}, Workbenches: {}, Ledgers initialized: {}",
            report.summary.review_packet_count,
            report.summary.workbench_render_count,
            report.summary.ledger_initialized_count
        ),
        format!(
            "- Ready to file after FA revision: {}",
            report.summary.ready_to_file_count
        ),
        format!(
            "- Accepted submissions: {}, Returned-and-corrected submissions: {}",
            report.summary.accepted_submission_count, report.summary.returned_correction_count
        ),
        format!("- Failures: {}", report.failures.len()),
        String::new(),
        "## Coverage".to_owned(),
        format!(
            "- Destinations: {}",
            render_counts(&report.summary.destination_counts)
        ),
        format!(
            "- Currencies: {}",
            render_counts(&report.summary.currency_counts)
        ),
        format!(
            "- Variants: {}",
            render_counts(&report.summary.variant_counts)
        ),
        format!(
            "- Scenarios: {}",
            render_counts(&report.summary.scenario_counts)
        ),
        format!(
            "- Trip window modes: {}",
            render_counts(&report.summary.trip_window_mode_counts)
        ),
        format!(
            "- Stay window modes: {}",
            render_counts(&report.summary.stay_window_mode_counts)
        ),
        format!(
            "- Hotel total modes: {}",
            render_counts(&report.summary.hotel_total_mode_counts)
        ),
        format!(
            "- Receipt total modes: {}",
            render_counts(&report.summary.receipt_total_mode_counts)
        ),
        format!(
            "- Final states: {}",
            render_counts(&report.summary.final_state_counts)
        ),
    ];

    if !report.failures.is_empty() {
        lines.push(String::new());
        lines.push("## Failures".to_owned());
        for failure in &report.failures {
            lines.push(format!(
                "- `{}` [{}] {}",
                failure.packet_id, failure.stage, failure.message
            ));
        }
    }

    lines.join("\n")
}

fn fixture_to_transcribed_document(
    packet_id: &str,
    fixture: &crate::synthetic_documents::SyntheticDocumentFixture,
) -> TranscribedDocument {
    TranscribedDocument {
        document_id: fixture.expected_facts.document_id.clone(),
        filename: fixture.filename.clone(),
        source_path: PathBuf::from(format!("{packet_id}/{}", fixture.filename)),
        engine: TranscriptionEngine::PlainText,
        pages: vec![TranscribedPage {
            page_number: 1,
            text: fixture.markdown.clone(),
        }],
    }
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
            origin: "corpus_eval.ready.affiliation".to_owned(),
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
            origin: "corpus_eval.ready.authorized_by".to_owned(),
        });
    }
    if let Some(attendees_path) = meal_attendees_path(ledger) {
        if value_text_at(&current_version.draft.report, &attendees_path).is_none() {
            field_edits.push(FieldEditInput {
                path: attendees_path,
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
                origin: "corpus_eval.ready.attendees".to_owned(),
            });
        }
    }

    let confirmed_review_paths = current_version
        .readiness
        .issues
        .iter()
        .filter(|issue| issue.class == ReadinessIssueClass::ManualReview)
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
            note: Some("Synthetic correction after site return".to_owned()),
            origin: "corpus_eval.returned.correction".to_owned(),
        }],
        confirmed_review_paths: Vec::new(),
        annotations: vec![crate::feedback::CorrectionAnnotation {
            path: returned_path.to_owned(),
            reason: FeedbackCategory::StanfordSiteWorkflowMismatch,
            note: Some("Revised after synthetic site return".to_owned()),
        }],
    }
}

fn current_version(ledger: &ReviewSubmissionLedger) -> &crate::ledger::DraftVersionRecord {
    ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == ledger.summary.current_draft_version_id)
        .expect("current draft version should exist")
}

fn meal_attendees_path(ledger: &ReviewSubmissionLedger) -> Option<String> {
    let mut projected_index = 0usize;
    for line in &ledger.bundle.expense_lines {
        if !line.projection_supported {
            continue;
        }
        if line.kind == CanonicalExpenseKind::Meal {
            return Some(format!(
                "expense_report.transaction_lines[{projected_index}].meal_details.attendees"
            ));
        }
        projected_index += 1;
    }
    None
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

fn filing_status_name(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "automation_blocked",
        FilingStatus::UserInputRequired => "user_input_required",
        FilingStatus::ManualReviewRequired => "manual_review_required",
        FilingStatus::ReadyToFile => "ready_to_file",
    }
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

fn render_counts(counts: &BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{evaluate_synthetic_corpus, render_synthetic_corpus_evaluation_markdown};

    #[test]
    fn synthetic_corpus_evaluation_is_clean_for_large_sample() {
        let report = evaluate_synthetic_corpus(128);
        assert!(
            report.is_clean(),
            "synthetic corpus failures: {:#?}",
            report.failures
        );
        assert_eq!(report.summary.packet_count, 128);
        assert_eq!(report.summary.document_count, 384);
        assert_eq!(report.summary.accepted_submission_count, 64);
        assert_eq!(report.summary.returned_correction_count, 64);
    }

    #[test]
    fn evaluation_markdown_includes_coverage_counts() {
        let report = evaluate_synthetic_corpus(12);
        let rendered = render_synthetic_corpus_evaluation_markdown(&report);
        assert!(rendered.contains("Synthetic Corpus Evaluation"));
        assert!(rendered.contains("Destinations:"));
        assert!(rendered.contains("Final states:"));
    }
}
