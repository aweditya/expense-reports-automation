use std::process::ExitCode;

use expense_report_schema::{
    apply_review_revision, ingest_submission_feedback, initialize_review_submission_ledger,
    parse_document_facts_json_path, record_submission_attempt, render_review_submission_ledger_json_pretty,
    render_review_submission_ledger_markdown, synthesize_bundle_projection,
    synthesize_bundle_projection_with_fx, ActorRole, DraftRevisionInput, FeedbackCategory,
    FieldEditInput, StaticFxRateProvider, SubmissionFeedback, SubmissionStatus, SubmissionFieldFeedback,
};
use expense_report_schema::ReadinessIssueClass;
use expense_report_schema::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Markdown,
    Json,
}

impl OutputFormat {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "markdown" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FxMode {
    None,
    Demo,
}

impl FxMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "demo" => Some(Self::Demo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Initial,
    Ready,
    Accepted,
    Returned,
}

impl Scenario {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "initial" => Some(Self::Initial),
            "ready" => Some(Self::Ready),
            "accepted" => Some(Self::Accepted),
            "returned" => Some(Self::Returned),
            _ => None,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut output_format = OutputFormat::Markdown;
    let mut fx_mode = FxMode::None;
    let mut scenario = Scenario::Initial;
    let mut bundle_id = "cli-bundle".to_owned();
    let mut input_paths = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --output".to_owned())?;
                output_format = OutputFormat::parse(&value)
                    .ok_or_else(|| "output format must be one of markdown | json".to_owned())?;
            }
            "--fx" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --fx".to_owned())?;
                fx_mode = FxMode::parse(&value)
                    .ok_or_else(|| "FX mode must be one of none | demo".to_owned())?;
            }
            "--scenario" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --scenario".to_owned())?;
                scenario = Scenario::parse(&value)
                    .ok_or_else(|| "scenario must be one of initial | ready | accepted | returned".to_owned())?;
            }
            "--bundle-id" => {
                bundle_id = args
                    .next()
                    .ok_or_else(|| "missing value after --bundle-id".to_owned())?;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: build_review_submission_ledger_from_facts [--output markdown|json] [--fx none|demo] [--scenario initial|ready|accepted|returned] [--bundle-id <id>] <facts.json>..."
                        .to_owned(),
                );
            }
            other => input_paths.push(other.to_owned()),
        }
    }

    if input_paths.is_empty() {
        return Err(
            "usage: build_review_submission_ledger_from_facts [--output markdown|json] [--fx none|demo] [--scenario initial|ready|accepted|returned] [--bundle-id <id>] <facts.json>..."
                .to_owned(),
        );
    }

    let mut documents = Vec::new();
    for path in &input_paths {
        documents.push(
            parse_document_facts_json_path(path)
                .map_err(|err| format!("failed to parse {path}: {err}"))?,
        );
    }

    let demo_fx_provider = StaticFxRateProvider::demo();
    let projection = match fx_mode {
        FxMode::None => synthesize_bundle_projection(&documents),
        FxMode::Demo => synthesize_bundle_projection_with_fx(&documents, &demo_fx_provider),
    };
    let mut ledger = initialize_review_submission_ledger(
        bundle_id,
        &projection.bundle,
        &projection.draft,
        &projection.validation,
    )
    .map_err(|err| format!("failed to initialize ledger: {err}"))?;

    match scenario {
        Scenario::Initial => {}
        Scenario::Ready => {
            let ready_revision = build_ready_revision(&ledger);
            apply_review_revision(&mut ledger, 1, ready_revision)
                .map_err(|err| format!("failed to apply ready revision: {err}"))?;
        }
        Scenario::Accepted => {
            let ready_revision = build_ready_revision(&ledger);
            let ready_version = apply_review_revision(&mut ledger, 1, ready_revision)
                .map_err(|err| format!("failed to apply ready revision: {err}"))?;
            let attempt_id = record_submission_attempt(&mut ledger, ready_version, Some("CLI submission".to_owned()))
                .map_err(|err| format!("failed to record submission attempt: {err}"))?;
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
        Scenario::Returned => {
            let ready_revision = build_ready_revision(&ledger);
            let ready_version = apply_review_revision(&mut ledger, 1, ready_revision)
                .map_err(|err| format!("failed to apply ready revision: {err}"))?;
            let attempt_id = record_submission_attempt(&mut ledger, ready_version, Some("CLI submission".to_owned()))
                .map_err(|err| format!("failed to record submission attempt: {err}"))?;
            let return_revision = build_return_revision(&ledger);
            ingest_submission_feedback(
                &mut ledger,
                attempt_id,
                SubmissionFeedback {
                    status: SubmissionStatus::Returned,
                    message: Some("Oracle returned the event name".to_owned()),
                    category: Some(FeedbackCategory::StanfordPolicyMismatch),
                    returned_fields: vec![SubmissionFieldFeedback {
                        path: Some("expense_report.general_information.event_name".to_owned()),
                        message: "Event title must be more specific".to_owned(),
                        category: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
                    }],
                },
                Some(return_revision),
            )
            .map_err(|err| format!("failed to ingest returned feedback: {err}"))?;
        }
    }

    let rendered = match output_format {
        OutputFormat::Markdown => render_review_submission_ledger_markdown(&ledger),
        OutputFormat::Json => render_review_submission_ledger_json_pretty(&ledger)
            .map_err(|err| format!("failed to render ledger json: {err}"))?,
    };

    println!("{rendered}");
    Ok(())
}

fn build_ready_revision(ledger: &expense_report_schema::ReviewSubmissionLedger) -> DraftRevisionInput {
    let current_version = ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == ledger.summary.current_draft_version_id)
        .expect("current draft version should exist");
    let payee_name = value_text_at(
        &current_version.draft.report,
        "expense_report.general_information.payee.name",
    )
    .unwrap_or_else(|| "Traveler".to_owned());

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
            note: Some("CLI synthetic fill".to_owned()),
            origin: "ledger_cli.ready.affiliation".to_owned(),
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
            note: Some("CLI synthetic fill".to_owned()),
            origin: "ledger_cli.ready.authorized_by".to_owned(),
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
                    ("name", ReportValue::from(payee_name)),
                    ("affiliation", ReportValue::from("Stanford staff")),
                ]),
                ReportValue::object([
                    ("name", ReportValue::from("Collaborator A")),
                    ("affiliation", ReportValue::from("External collaborator")),
                ]),
            ]),
            reason: Some(FeedbackCategory::MissingRequiredField),
            note: Some("CLI synthetic fill".to_owned()),
            origin: "ledger_cli.ready.attendees".to_owned(),
        });
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
        label: "CLI ready revision".to_owned(),
        field_edits,
        confirmed_review_paths,
        annotations: Vec::new(),
    }
}

fn build_return_revision(ledger: &expense_report_schema::ReviewSubmissionLedger) -> DraftRevisionInput {
    let current_version = ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == ledger.summary.current_draft_version_id)
        .expect("current draft version should exist");
    let current_value = value_text_at(
        &current_version.draft.report,
        "expense_report.general_information.event_name",
    )
    .unwrap_or_else(|| "Reviewed Event".to_owned());

    DraftRevisionInput {
        actor_role: ActorRole::FinancialAdministrator,
        label: "CLI returned correction".to_owned(),
        field_edits: vec![FieldEditInput {
            path: "expense_report.general_information.event_name".to_owned(),
            value: ReportValue::from(format!("{current_value} Reviewed")),
            reason: Some(FeedbackCategory::StanfordSiteWorkflowMismatch),
            note: Some("CLI returned correction".to_owned()),
            origin: "ledger_cli.returned.event_name".to_owned(),
        }],
        confirmed_review_paths: Vec::new(),
        annotations: vec![expense_report_schema::CorrectionAnnotation {
            path: "expense_report.general_information.event_name".to_owned(),
            reason: FeedbackCategory::StanfordSiteWorkflowMismatch,
            note: Some("Updated after Oracle return".to_owned()),
        }],
    }
}

fn value_text_at(report: &ReportValue, path: &str) -> Option<String> {
    value_at(report, path).and_then(|value| match value {
        ReportValue::Null => None,
        ReportValue::String(value) | ReportValue::Number(value) | ReportValue::Date(value) => {
            Some(value.clone())
        }
        ReportValue::Bool(value) => Some(value.to_string()),
        ReportValue::Array(_) | ReportValue::Object(_) => None,
    })
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

#[cfg(test)]
mod tests {
    use super::{FxMode, OutputFormat, Scenario};

    #[test]
    fn parses_output_formats() {
        assert_eq!(OutputFormat::parse("markdown"), Some(OutputFormat::Markdown));
        assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("yaml"), None);
    }

    #[test]
    fn parses_fx_modes() {
        assert_eq!(FxMode::parse("none"), Some(FxMode::None));
        assert_eq!(FxMode::parse("demo"), Some(FxMode::Demo));
        assert_eq!(FxMode::parse("live"), None);
    }

    #[test]
    fn parses_scenarios() {
        assert_eq!(Scenario::parse("initial"), Some(Scenario::Initial));
        assert_eq!(Scenario::parse("ready"), Some(Scenario::Ready));
        assert_eq!(Scenario::parse("accepted"), Some(Scenario::Accepted));
        assert_eq!(Scenario::parse("returned"), Some(Scenario::Returned));
        assert_eq!(Scenario::parse("other"), None);
    }
}
