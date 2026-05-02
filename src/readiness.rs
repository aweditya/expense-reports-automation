use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::validation_rules::{field_rule, SourceTier};
use crate::validator::{
    ValidationIssue, ValidationIssueKind, ValidationReport, ValidationSeverity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessIssueClass {
    AutomationGap,
    UserInputRequired,
    ManualReview,
    OtherWarning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessIssue {
    pub class: ReadinessIssueClass,
    pub severity: ValidationSeverity,
    pub kind: ValidationIssueKind,
    pub path: String,
    pub schema_path: String,
    pub source: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessReport {
    pub issues: Vec<ReadinessIssue>,
}

impl ReadinessReport {
    pub fn automation_gap_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.class == ReadinessIssueClass::AutomationGap)
            .count()
    }

    pub fn user_input_required_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.class == ReadinessIssueClass::UserInputRequired)
            .count()
    }

    pub fn manual_review_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.class == ReadinessIssueClass::ManualReview)
            .count()
    }

    pub fn other_warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.class == ReadinessIssueClass::OtherWarning)
            .count()
    }
}

pub fn summarize_validation_readiness(validation: &ValidationReport) -> ReadinessReport {
    summarize_validation_readiness_with_confirmations(validation, &BTreeSet::new())
}

pub fn summarize_validation_readiness_with_confirmations(
    validation: &ValidationReport,
    confirmed_paths: &BTreeSet<String>,
) -> ReadinessReport {
    ReadinessReport {
        issues: validation
            .issues
            .iter()
            .filter_map(classify_validation_issue)
            .filter(|issue| !issue_is_confirmed(issue, confirmed_paths))
            .collect(),
    }
}

fn issue_is_confirmed(issue: &ReadinessIssue, confirmed_paths: &BTreeSet<String>) -> bool {
    matches!(
        issue.kind,
        ValidationIssueKind::ManualReviewRequired | ValidationIssueKind::LowConfidenceWithoutReview
    ) && confirmed_paths
        .iter()
        .any(|path| issue.path == *path || issue.schema_path == *path)
}

fn classify_validation_issue(issue: &ValidationIssue) -> Option<ReadinessIssue> {
    let class = match issue.kind {
        ValidationIssueKind::ManualReviewRequired
        | ValidationIssueKind::LowConfidenceWithoutReview => ReadinessIssueClass::ManualReview,
        ValidationIssueKind::MissingRequiredField | ValidationIssueKind::MissingDependency => {
            if issue_is_user_input(issue) {
                ReadinessIssueClass::UserInputRequired
            } else {
                ReadinessIssueClass::AutomationGap
            }
        }
        _ if issue.severity == ValidationSeverity::Error => ReadinessIssueClass::AutomationGap,
        _ => ReadinessIssueClass::OtherWarning,
    };

    let source = field_rule(&issue.schema_path)
        .and_then(|rule| rule.effective_source)
        .map(source_tier_name)
        .map(str::to_owned);

    Some(ReadinessIssue {
        class,
        severity: issue.severity,
        kind: issue.kind,
        path: issue.path.clone(),
        schema_path: issue.schema_path.clone(),
        source,
        message: issue.message.clone(),
    })
}

fn issue_is_user_input(issue: &ValidationIssue) -> bool {
    if contextual_user_input_schema_path(&issue.schema_path) {
        return true;
    }

    let Some(rule) = field_rule(&issue.schema_path) else {
        return false;
    };

    if rule_is_user_input(rule.effective_source, rule.infer_from, rule.description) {
        return true;
    }

    issue.kind == ValidationIssueKind::MissingDependency
        && !rule.depends_on.is_empty()
        && rule.depends_on.iter().all(|dependency| {
            field_rule(&absolute_schema_path(dependency)).is_some_and(|dependency_rule| {
                rule_is_user_input(
                    dependency_rule.effective_source,
                    dependency_rule.infer_from,
                    dependency_rule.description,
                )
            })
        })
}

fn contextual_user_input_schema_path(path: &str) -> bool {
    matches!(
        path,
        "expense_report.general_information.category"
            | "expense_report.general_information.payee"
            | "expense_report.general_information.payee.name"
            | "expense_report.general_information.business_purpose"
            | "expense_report.general_information.business_purpose.who"
            | "expense_report.general_information.business_purpose.what"
            | "expense_report.general_information.business_purpose.where"
            | "expense_report.general_information.business_purpose.why"
            | "expense_report.general_information.event_name"
    )
}

fn rule_is_user_input(
    source: Option<SourceTier>,
    infer_from: Option<&str>,
    description: Option<&str>,
) -> bool {
    match source {
        Some(SourceTier::T1) => true,
        Some(SourceTier::T2) => false,
        Some(SourceTier::T3) => {
            infer_from.is_some_and(text_has_user_input_hint)
                || description.is_some_and(text_has_user_input_hint)
        }
        None => false,
    }
}

fn text_has_user_input_hint(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    normalized.contains("fa input")
        || normalized.contains("only the payee knows")
        || normalized.contains("approver")
        || normalized.contains("beneficiar")
}

fn absolute_schema_path(path: &str) -> String {
    if path.starts_with("expense_report.") {
        path.to_owned()
    } else {
        format!("expense_report.{path}")
    }
}

fn source_tier_name(source: SourceTier) -> &'static str {
    match source {
        SourceTier::T1 => "t1",
        SourceTier::T2 => "t2",
        SourceTier::T3 => "t3",
    }
}

#[cfg(test)]
mod tests {
    use super::{summarize_validation_readiness, ReadinessIssueClass};
    use crate::bundle_synthesis::{
        synthesize_bundle_projection, synthesize_bundle_projection_with_fx, StaticFxRateProvider,
    };
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};
    use crate::validator::{ValidationIssue, ValidationIssueKind, ValidationReport, ValidationSeverity};

    fn synthetic_documents() -> Vec<crate::ExtractedDocumentFacts> {
        generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect()
    }

    #[test]
    fn fx_projection_readiness_distinguishes_user_input_from_automation_gaps() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let readiness = summarize_validation_readiness(&projection.validation);

        assert_eq!(readiness.automation_gap_count(), 0);
        assert_eq!(readiness.user_input_required_count(), 4);
        assert_eq!(readiness.manual_review_count(), 3);
        assert_eq!(
            readiness
                .issues
                .iter()
                .filter(|issue| issue.class == ReadinessIssueClass::UserInputRequired)
                .map(|issue| issue.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "expense_report.general_information.payee.affiliation",
                "expense_report.general_information",
                "expense_report.general_information.authorized_by",
                "expense_report.transaction_lines[2].meal_details.attendees",
            ]
        );
    }

    #[test]
    fn no_fx_projection_keeps_conversion_and_total_failures_as_automation_gaps() {
        let projection = synthesize_bundle_projection(&synthetic_documents());
        let readiness = summarize_validation_readiness(&projection.validation);

        assert!(readiness.automation_gap_count() >= 3);
        assert!(readiness
            .issues
            .iter()
            .any(|issue| issue.class == ReadinessIssueClass::AutomationGap
                && issue.path == "expense_report.transaction_summary.total_usd"));
        assert!(readiness
            .issues
            .iter()
            .any(|issue| issue.class == ReadinessIssueClass::AutomationGap
                && issue.path == "expense_report.transaction_lines[1].common.line_amount_usd"));
        assert!(readiness.issues.iter().any(|issue| issue.class
            == ReadinessIssueClass::UserInputRequired
            && issue.path == "expense_report.general_information.authorized_by"));
    }

    #[test]
    fn receipt_only_context_fields_become_user_input() {
        let validation = ValidationReport {
            issues: vec![
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.category".to_owned(),
                    schema_path: "expense_report.general_information.category".to_owned(),
                    message: "Required field is missing".to_owned(),
                },
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.payee".to_owned(),
                    schema_path: "expense_report.general_information.payee".to_owned(),
                    message: "Required field is missing".to_owned(),
                },
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.business_purpose".to_owned(),
                    schema_path: "expense_report.general_information.business_purpose".to_owned(),
                    message: "Required field is missing".to_owned(),
                },
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.business_purpose.who".to_owned(),
                    schema_path: "expense_report.general_information.business_purpose.who"
                        .to_owned(),
                    message: "Required field is missing".to_owned(),
                },
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.business_purpose.what".to_owned(),
                    schema_path: "expense_report.general_information.business_purpose.what"
                        .to_owned(),
                    message: "Required field is missing".to_owned(),
                },
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.event_name".to_owned(),
                    schema_path: "expense_report.general_information.event_name".to_owned(),
                    message: "Required field is missing".to_owned(),
                },
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.business_purpose.key_30char"
                        .to_owned(),
                    schema_path:
                        "expense_report.general_information.business_purpose.key_30char"
                            .to_owned(),
                    message: "Required field is missing".to_owned(),
                },
                ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.transaction_summary.transaction_type".to_owned(),
                    schema_path: "expense_report.transaction_summary.transaction_type"
                        .to_owned(),
                    message: "Required field is missing".to_owned(),
                },
            ],
        };

        let readiness = summarize_validation_readiness(&validation);

        assert_eq!(readiness.automation_gap_count(), 0);
        assert_eq!(readiness.user_input_required_count(), 8);
        assert!(readiness
            .issues
            .iter()
            .all(|issue| issue.class == ReadinessIssueClass::UserInputRequired));
    }
}
