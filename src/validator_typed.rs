//! M7.d.1 — typed validator that walks `ExpenseReport` directly.
//!
//! The existing `src/validator.rs` operates on `ReportValue` (untyped tree)
//! because it was written for the pre-redesign pipeline. This module reads
//! the typed `ExpenseReport` directly: walk the tree, look up the matching
//! `FieldRule` by path at each leaf, emit `ValidationIssue`s for violations.
//!
//! The minimal expression evaluator handles the two patterns that appear in
//! the current 22 conditional rules:
//!   <path> == <value>
//!   <path> in [<v1>, <v2>, ...]
//! One rule (`Must fall within trip date window`) is a free-text validation
//! description, not a mechanical expression — it's noted as a TODO.

use crate::expense_report_model::{
    ExpenseReport, ExpenseReportGeneralInformation, ExpenseReportGeneralInformationBusinessPurpose,
    ExpenseReportGeneralInformationPayee, ExpenseReportGeneralInformationStudentCertification,
    ExpenseReportTransactionLinesItem, ExpenseReportTransactionLinesItemCommon,
    ExpenseReportTransactionLinesItemGroundTransportDetails,
    ExpenseReportTransactionLinesItemLodgingDetails,
    ExpenseReportTransactionLinesItemMealDetails, ExpenseReportTransactionSummary,
};
use crate::meta::Wrapped;
use crate::validation_rules::{
    field_rule, ConditionalRule, ConditionalRuleType, FieldRule, CONDITIONAL_RULES,
};
use crate::validator::{
    ValidationIssue, ValidationIssueKind, ValidationReport, ValidationSeverity,
};

// ─── Public entry point ────────────────────────────────────────────────────

pub fn validate_typed(report: &ExpenseReport) -> ValidationReport {
    let mut issues = Vec::new();
    walk_general_information(
        &report.general_information,
        "expense_report.general_information",
        &mut issues,
    );
    walk_transaction_summary(
        &report.transaction_summary,
        "expense_report.transaction_summary",
        &mut issues,
    );
    walk_transaction_lines(report, &mut issues);
    check_conditional_rules(report, &mut issues);
    check_category_country_consistency(report, &mut issues);
    ValidationReport { issues }
}

// ─── Per-section walks ─────────────────────────────────────────────────────

fn walk_general_information(
    gi: &ExpenseReportGeneralInformation,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    check_wrapped(&join(base, "category"), &gi.category, issues);
    walk_payee(&gi.payee, &join(base, "payee"), issues);
    check_optional(&join(base, "rush_processing"), &gi.rush_processing, issues);
    check_wrapped(&join(base, "payment_method"), &gi.payment_method, issues);
    walk_business_purpose(
        &gi.business_purpose,
        &join(base, "business_purpose"),
        issues,
    );
    check_wrapped(&join(base, "event_name"), &gi.event_name, issues);
    walk_student_certification(
        &gi.student_certification,
        &join(base, "student_certification"),
        issues,
    );
    check_optional(&join(base, "authorized_by"), &gi.authorized_by, issues);
}

fn walk_payee(payee: &ExpenseReportGeneralInformationPayee, base: &str, issues: &mut Vec<ValidationIssue>) {
    check_wrapped(&join(base, "name"), &payee.name, issues);
    check_wrapped(&join(base, "affiliation"), &payee.affiliation, issues);
}

fn walk_business_purpose(
    bp: &ExpenseReportGeneralInformationBusinessPurpose,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    // Phase 5 re-tier: business_purpose sub-fields moved from T1
    // (Option<String>) to T2/T4 (Wrapped<String>). Walk via check_wrapped
    // so the per-field meta (synthesis confidence, derivation origin)
    // gets surfaced. T1 fallback when no conference docs is just
    // value=None on the Wrapped — same missing-required behavior.
    check_wrapped(&join(base, "who"), &bp.who, issues);
    check_wrapped(&join(base, "what"), &bp.what, issues);
    check_wrapped(&join(base, "when"), &bp.when, issues);
    check_wrapped(&join(base, "where"), &bp.r#where, issues);
    check_wrapped(&join(base, "why"), &bp.why, issues);
    check_wrapped(&join(base, "key_30char"), &bp.key_30char, issues);
}

fn walk_student_certification(
    sc: &ExpenseReportGeneralInformationStudentCertification,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    check_wrapped(&join(base, "supports_faculty_research"), &sc.supports_faculty_research, issues);
    check_wrapped(&join(base, "presenting_at_conference"), &sc.presenting_at_conference, issues);
    check_wrapped(&join(base, "integral_to_degree_work"), &sc.integral_to_degree_work, issues);
    check_wrapped(&join(base, "related_to_employment"), &sc.related_to_employment, issues);
    check_wrapped(&join(base, "other"), &sc.other, issues);
    check_wrapped(&join(base, "other_explanation"), &sc.other_explanation, issues);
}

fn walk_transaction_summary(
    ts: &ExpenseReportTransactionSummary,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    check_wrapped(&join(base, "transaction_number"), &ts.transaction_number, issues);
    check_wrapped(&join(base, "transaction_date"), &ts.transaction_date, issues);
    check_optional(&join(base, "status"), &ts.status, issues);
    check_wrapped(&join(base, "total_usd"), &ts.total_usd, issues);
}

fn walk_transaction_lines(report: &ExpenseReport, issues: &mut Vec<ValidationIssue>) {
    let lines = match report.transaction_lines.as_ref() {
        Some(v) => v,
        None => return,
    };
    for (idx, line) in lines.iter().enumerate() {
        let base = format!("expense_report.transaction_lines[{idx}]");
        walk_line_common(&line.common, &join(&base, "common"), issues);
        if let Some(meal) = &line.meal_details {
            walk_meal_details(meal, &join(&base, "meal_details"), issues);
        }
        if let Some(gt) = &line.ground_transport_details {
            walk_ground_transport_details(
                gt,
                &join(&base, "ground_transport_details"),
                issues,
            );
        }
        if let Some(lodging) = &line.lodging_details {
            walk_lodging_details(lodging, &join(&base, "lodging_details"), issues);
        }
        // Remaining detail blocks (airfare / car_rental /
        // conference_registration / gift / human_subject) intentionally
        // not walked yet — phases beyond Phase 3 add them when we have
        // real receipts to ground the schema in.
    }
}

fn walk_line_common(
    common: &ExpenseReportTransactionLinesItemCommon,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    check_wrapped(&join(base, "date"), &common.date, issues);
    check_wrapped(&join(base, "line_amount_usd"), &common.line_amount_usd, issues);
    check_wrapped(&join(base, "original_currency"), &common.original_currency, issues);
    check_wrapped(&join(base, "original_amount"), &common.original_amount, issues);
    check_wrapped(&join(base, "exchange_rate"), &common.exchange_rate, issues);
    check_wrapped(&join(base, "expense_type"), &common.expense_type, issues);
    check_wrapped(&join(base, "remarks"), &common.remarks, issues);
    check_wrapped(&join(base, "country_of_activity"), &common.country_of_activity, issues);
    check_wrapped(&join(base, "foreign_activity_type"), &common.foreign_activity_type, issues);
    // source_document is a non-Optional object (always structurally
    // present); validate its T1 leaves directly. Reduction always sets
    // filename from the input file; document_type is whatever the FA
    // chose at upload time.
    let sd_base = join(base, "source_document");
    check_optional(&join(&sd_base, "filename"), &common.source_document.filename, issues);
    check_optional(&join(&sd_base, "document_type"), &common.source_document.document_type, issues);
}

fn walk_meal_details(
    meal: &ExpenseReportTransactionLinesItemMealDetails,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    check_wrapped(&join(base, "venue_name"), &meal.venue_name, issues);
    if let Some(rule) = field_rule(&join(base, "attendees")) {
        if rule.required && meal.attendees.is_empty() {
            issues.push(missing_required(&join(base, "attendees"), rule));
        }
    }
    check_optional(&join(base, "meal_purpose"), &meal.meal_purpose, issues);
    check_wrapped(&join(base, "alcohol_amount"), &meal.alcohol_amount, issues);
    check_wrapped(&join(base, "tip_amount"), &meal.tip_amount, issues);
    check_wrapped(&join(base, "has_alcohol_on_receipt"), &meal.has_alcohol_on_receipt, issues);
}

fn walk_ground_transport_details(
    gt: &ExpenseReportTransactionLinesItemGroundTransportDetails,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    check_wrapped(&join(base, "origin"), &gt.origin, issues);
    check_wrapped(&join(base, "destination"), &gt.destination, issues);
    check_wrapped(&join(base, "service_provider"), &gt.service_provider, issues);
    // missing_receipt is T1 (FA-set; bare Option<bool>). The schema marks
    // it required, but it stays None in the per-receipt extraction — the
    // FA fills it in the workbench / portal. check_optional emits
    // MissingRequiredField when None on a required field.
    check_optional(&join(base, "missing_receipt"), &gt.missing_receipt, issues);
}

fn walk_lodging_details(
    lodging: &ExpenseReportTransactionLinesItemLodgingDetails,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    check_wrapped(&join(base, "hotel_name"), &lodging.hotel_name, issues);
    check_wrapped(&join(base, "location"), &lodging.location, issues);
    check_wrapped(&join(base, "check_in_date"), &lodging.check_in_date, issues);
    check_wrapped(&join(base, "check_out_date"), &lodging.check_out_date, issues);
    check_wrapped(&join(base, "number_of_nights"), &lodging.number_of_nights, issues);
    check_wrapped(&join(base, "daily_rate"), &lodging.daily_rate, issues);
    check_wrapped(&join(base, "booking_method"), &lodging.booking_method, issues);
    check_wrapped(&join(base, "is_shared_lodging"), &lodging.is_shared_lodging, issues);
    // shared_with_transaction_number is T1 (bare Option<String>),
    // conditionally required by an expression rule: "is_shared_lodging
    // == true". The expression rule walker (check_conditional_rules) will
    // surface the issue when applicable; here we only check unconditional
    // required-presence — which means: never error on it, since it's
    // only required conditionally.
    let _ = lodging.shared_with_transaction_number.as_ref();
    // personal_nights_excluded is T2 optional — derived later, not by
    // the extractor. Walking it produces no issues.
    check_wrapped(&join(base, "personal_nights_excluded"), &lodging.personal_nights_excluded, issues);
}

// ─── Leaf checks ───────────────────────────────────────────────────────────

fn check_wrapped<T>(path: &str, w: &Wrapped<T>, issues: &mut Vec<ValidationIssue>) {
    let Some(rule) = field_rule(path) else {
        return;
    };
    // Required-presence: only if the rule is unconditionally required.
    // Conditionally-required (rule.required_expression.is_some()) is handled
    // by check_conditional_rules below, not here.
    if rule.required && rule.required_expression.is_none() && w.value.is_none() {
        issues.push(missing_required(path, rule));
    }
}

fn check_optional<T>(path: &str, opt: &Option<T>, issues: &mut Vec<ValidationIssue>) {
    let Some(rule) = field_rule(path) else {
        return;
    };
    if rule.required && rule.required_expression.is_none() && opt.is_none() {
        issues.push(missing_required(path, rule));
    }
}

fn missing_required(path: &str, rule: &FieldRule) -> ValidationIssue {
    let _ = rule;
    ValidationIssue {
        severity: ValidationSeverity::Error,
        kind: ValidationIssueKind::MissingRequiredField,
        path: path.to_owned(),
        schema_path: path.to_owned(),
        message: "Required field is missing".to_owned(),
    }
}

// ─── Conditional rules ─────────────────────────────────────────────────────

fn check_conditional_rules(report: &ExpenseReport, issues: &mut Vec<ValidationIssue>) {
    for rule in CONDITIONAL_RULES {
        if rule.rule_type != ConditionalRuleType::RequiredWhen {
            // DependsOn rules are advisory; ValidationExpression is free-text
            // (e.g. "Must fall within trip date window") — TODO: implement
            // domain-specific checks for these once we wire trip-window
            // computation in reduction.
            continue;
        }
        let Some(expr_text) = rule.expression else {
            continue;
        };

        if rule.target_path.contains("[]") {
            check_per_line_conditional(report, rule, expr_text, issues);
        } else {
            check_root_conditional(report, rule, expr_text, issues);
        }
    }
}

fn check_per_line_conditional(
    report: &ExpenseReport,
    rule: &ConditionalRule,
    expr_text: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let lines = match report.transaction_lines.as_ref() {
        Some(v) => v,
        None => return,
    };
    let expr = match parse_expression(expr_text) {
        Some(e) => e,
        None => return,
    };
    for (idx, line) in lines.iter().enumerate() {
        let line_base = format!("expense_report.transaction_lines[{idx}]");
        if !evaluate_in_line_context(&expr, line, report) {
            continue;
        }
        // Substitute [] in the rule's target_path with the actual index.
        let target = rule.target_path.replacen("[]", &format!("[{idx}]"), 1);
        if !target_present_at(&target, line, report) {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                kind: ValidationIssueKind::MissingRequiredField,
                path: target.clone(),
                schema_path: rule.target_path.to_owned(),
                message: format!(
                    "Required when {expr_text} (in line {})",
                    idx + 1
                ),
            });
        }
        // Make sure the line_base variable is acknowledged as used.
        let _ = line_base;
    }
}

fn check_root_conditional(
    report: &ExpenseReport,
    rule: &ConditionalRule,
    expr_text: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let expr = match parse_expression(expr_text) {
        Some(e) => e,
        None => return,
    };
    if !evaluate_at_root(&expr, report) {
        return;
    }
    if !target_present_at_root(rule.target_path, report) {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            kind: ValidationIssueKind::MissingRequiredField,
            path: rule.target_path.to_owned(),
            schema_path: rule.target_path.to_owned(),
            message: format!("Required when {expr_text}"),
        });
    }
}

// ─── Expression parsing (minimal: == and in [...]) ────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Equals { path: Vec<String>, value: String },
    In { path: Vec<String>, values: Vec<String> },
}

fn parse_expression(text: &str) -> Option<Expr> {
    let text = text.trim();
    // " in [a, b, c]"
    if let Some((lhs, rest)) = text.split_once(" in ") {
        let path = parse_path(lhs.trim());
        let values = parse_value_list(rest.trim())?;
        return Some(Expr::In { path, values });
    }
    // "== rhs"
    if let Some((lhs, rhs)) = text.split_once("==") {
        let path = parse_path(lhs.trim());
        let value = rhs.trim().to_owned();
        return Some(Expr::Equals { path, value });
    }
    None
}

fn parse_path(s: &str) -> Vec<String> {
    s.split('.').map(|seg| seg.to_owned()).collect()
}

fn parse_value_list(s: &str) -> Option<Vec<String>> {
    let s = s.trim_start_matches('[').trim_end_matches(']');
    Some(s.split(',').map(|v| v.trim().to_owned()).collect())
}

// ─── Expression evaluation ────────────────────────────────────────────────

fn evaluate_in_line_context(
    expr: &Expr,
    line: &ExpenseReportTransactionLinesItem,
    report: &ExpenseReport,
) -> bool {
    match expr {
        Expr::Equals { path, value } => {
            let lhs = lookup_value(path, line, report);
            lhs.as_deref() == Some(value.as_str())
        }
        Expr::In { path, values } => {
            let lhs = lookup_value(path, line, report);
            lhs.is_some_and(|v| values.iter().any(|x| x == &v))
        }
    }
}

fn evaluate_at_root(expr: &Expr, report: &ExpenseReport) -> bool {
    match expr {
        Expr::Equals { path, value } => {
            let lhs = lookup_value_at_root(path, report);
            lhs.as_deref() == Some(value.as_str())
        }
        Expr::In { path, values } => {
            let lhs = lookup_value_at_root(path, report);
            lhs.is_some_and(|v| values.iter().any(|x| x == &v))
        }
    }
}

/// Resolve a path to a string value. Looks in the line's context first
/// (e.g. `expense_type` → line.common.expense_type), then falls back to
/// the report root for absolute paths (e.g. `general_information.category`).
fn lookup_value(
    path: &[String],
    line: &ExpenseReportTransactionLinesItem,
    report: &ExpenseReport,
) -> Option<String> {
    // Single-segment identifiers map to the line's common block.
    if path.len() == 1 {
        return lookup_in_line_common(&path[0], line);
    }
    // Two-segment paths inside a line: e.g. `lodging_details.is_shared_lodging`.
    if path.len() == 2 {
        if let Some(v) = lookup_two_segment_in_line(&path[0], &path[1], line) {
            return Some(v);
        }
    }
    // Otherwise treat as absolute (e.g. `general_information.category`).
    lookup_value_at_root(path, report)
}

fn lookup_in_line_common(field: &str, line: &ExpenseReportTransactionLinesItem) -> Option<String> {
    match field {
        "expense_type" => line.common.expense_type.value.as_ref().map(|v| v.as_str().to_owned()),
        _ => None,
    }
}

fn lookup_two_segment_in_line(
    block: &str,
    field: &str,
    line: &ExpenseReportTransactionLinesItem,
) -> Option<String> {
    match (block, field) {
        ("lodging_details", "is_shared_lodging") => line
            .lodging_details
            .as_ref()
            .and_then(|l| l.is_shared_lodging.value.map(|b| b.to_string())),
        _ => None,
    }
}

fn lookup_value_at_root(path: &[String], report: &ExpenseReport) -> Option<String> {
    let segments: Vec<&str> = path.iter().map(String::as_str).collect();
    match segments.as_slice() {
        ["general_information", "category"] => report
            .general_information
            .category
            .value
            .as_ref()
            .map(|c| c.as_str().to_owned()),
        ["allocation_and_approvers", "other_beneficiaries"] => report
            .allocation_and_approvers
            .other_beneficiaries
            .map(|b| b.to_string()),
        ["student_certification", "other"] => report
            .general_information
            .student_certification
            .other
            .value
            .map(|b| b.to_string()),
        _ => None,
    }
}

// ─── Target presence checks ────────────────────────────────────────────────

fn target_present_at(
    target: &str,
    line: &ExpenseReportTransactionLinesItem,
    _report: &ExpenseReport,
) -> bool {
    // Strip the report-rooted prefix, e.g.
    // "expense_report.transaction_lines[3].common.original_currency"
    // → "common.original_currency"
    let after_line = match target.split_once("].") {
        Some((_, rest)) => rest,
        None => return true,
    };
    let segments: Vec<&str> = after_line.split('.').collect();
    match segments.as_slice() {
        ["common", "original_currency"] => line.common.original_currency.value.is_some(),
        ["common", "original_amount"] => line.common.original_amount.value.is_some(),
        ["common", "exchange_rate"] => line.common.exchange_rate.value.is_some(),
        ["common", "country_of_activity"] => line.common.country_of_activity.value.is_some(),
        ["common", "foreign_activity_type"] => line.common.foreign_activity_type.value.is_some(),
        ["airfare_details"] => line.airfare_details.is_some(),
        ["lodging_details"] => line.lodging_details.is_some(),
        ["lodging_details", "shared_with_transaction_number"] => line
            .lodging_details
            .as_ref()
            .is_some_and(|l| l.shared_with_transaction_number.is_some()),
        ["ground_transport_details"] => line.ground_transport_details.is_some(),
        ["conference_registration_details"] => line.conference_registration_details.is_some(),
        ["meal_details"] => line.meal_details.is_some(),
        ["meal_details", "alcohol_amount"] => line
            .meal_details
            .as_ref()
            .is_some_and(|m| m.alcohol_amount.value.is_some()),
        ["car_rental_details"] => line.car_rental_details.is_some(),
        ["gift_details"] => line.gift_details.is_some(),
        ["human_subject_details"] => line.human_subject_details.is_some(),
        _ => true, // unknown target — be permissive rather than spurious-fail
    }
}

fn target_present_at_root(target: &str, report: &ExpenseReport) -> bool {
    match target {
        "expense_report.general_information.student_certification.other_explanation" => report
            .general_information
            .student_certification
            .other_explanation
            .value
            .is_some(),
        "expense_report.allocation_and_approvers.beneficiary_list" => report
            .allocation_and_approvers
            .beneficiary_list
            .as_ref()
            .is_some_and(|v| !v.is_empty()),
        "expense_report.per_diem_expenses[].foreign_activity_type" => {
            // Per-line per-diem rule but with no [] context — advisory pass.
            true
        }
        _ => true,
    }
}

// ─── Pass-2 cross-field rules ──────────────────────────────────────────────

/// Domestic-vs-foreign consistency between the report-level
/// `general_information.category` (FA-confirmed) and each line's
/// `common.country_of_activity` (extracted from the receipt's address).
///
/// Rules:
/// - `expenses_domestic`: every line with a known country must be
///   "United States". A foreign country on a domestic report is suspicious.
/// - `expenses_foreign`: at least one line must have a non-US country.
///   A foreign report with no foreign-country lines suggests the category
///   is wrong.
///
/// Lines with a null `country_of_activity` are ignored — they carry no
/// signal to compare against. Severity is **Warning**, not Error: the
/// FA might legitimately have a domestic taxi receipt during a foreign
/// trip, and we don't want to block the workflow on legitimate edge
/// cases. The issues panel surfaces them for FA review.
///
/// Other categories (`athletic_use_only`, `hr_use_only`, `human_subjects`,
/// `relocation`) don't carry a domestic/foreign claim, so they're
/// out of scope for this rule.
fn check_category_country_consistency(
    report: &ExpenseReport,
    issues: &mut Vec<ValidationIssue>,
) {
    use crate::expense_report_model::ExpenseReportGeneralInformationCategoryEnum::*;

    let Some(category) = report.general_information.category.value.as_ref() else {
        return;
    };
    let Some(lines) = report.transaction_lines.as_ref() else {
        return;
    };

    let category_path = "expense_report.general_information.category";

    match category {
        ExpensesDomestic => {
            for (idx, line) in lines.iter().enumerate() {
                let Some(country) = line.common.country_of_activity.value.as_ref() else {
                    continue;
                };
                if !country.eq_ignore_ascii_case("United States") {
                    let path = format!(
                        "expense_report.transaction_lines[{idx}].common.country_of_activity"
                    );
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Warning,
                        kind: ValidationIssueKind::ManualReviewRequired,
                        path,
                        schema_path: "expense_report.transaction_lines[].common.country_of_activity"
                            .to_owned(),
                        message: format!(
                            "Line {} country is {country:?} but the report category is \
                             expenses_domestic — confirm whether this should be a foreign report.",
                            idx + 1
                        ),
                    });
                }
            }
        }
        ExpensesForeign => {
            // A report is legitimately foreign if ANY line has either a
            // non-US country_of_activity OR a non-USD original_currency.
            // Both are independent signals: an Air India BOM→SFO ticket
            // is foreign by currency (INR) even though the model labels
            // its country as "United States" (the trip's purpose-end
            // destination, per the airfare prompt). Country alone is too
            // strict and false-flags those tickets.
            let any_foreign_signal = lines.iter().any(|line| {
                let has_foreign_country = line
                    .common
                    .country_of_activity
                    .value
                    .as_deref()
                    .is_some_and(|c| !c.eq_ignore_ascii_case("United States"));
                let has_foreign_currency = line
                    .common
                    .original_currency
                    .value
                    .as_deref()
                    .is_some_and(|c| !c.is_empty() && !c.eq_ignore_ascii_case("USD"));
                has_foreign_country || has_foreign_currency
            });
            if !any_foreign_signal {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    kind: ValidationIssueKind::ManualReviewRequired,
                    path: category_path.to_owned(),
                    schema_path: category_path.to_owned(),
                    message: "Report category is expenses_foreign but no line has a \
                              non-US country_of_activity or a non-USD original_currency \
                              — confirm whether this should be a domestic report."
                        .to_owned(),
                });
            }
        }
        _ => {
            // Non-domestic-foreign categories don't carry the claim;
            // nothing to check.
        }
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn join(base: &str, child: &str) -> String {
    format!("{base}.{child}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expense_report_model::{
        ExpenseReportGeneralInformationCategoryEnum, ExpenseReportTransactionLinesItemCommonExpenseTypeEnum,
        IsoDate,
    };
    use crate::extracted_receipt::{Extras, ExtractedReceipt};
    use crate::meta::FieldMetadata;
    use crate::reduce::reduce_to_expense_report;

    fn sample_meal_receipt(filename: &str, date: &str, amount: f64) -> ExtractedReceipt {
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.date = Wrapped {
            value: Some(IsoDate(date.to_owned())),
            meta: FieldMetadata::default(),
        };
        line.common.line_amount_usd = Wrapped {
            value: Some(amount),
            meta: FieldMetadata::default(),
        };
        line.common.expense_type = Wrapped {
            value: Some(ExpenseReportTransactionLinesItemCommonExpenseTypeEnum::BusinessMeal),
            meta: FieldMetadata::default(),
        };
        line.meal_details = Some(ExpenseReportTransactionLinesItemMealDetails::default());
        ExtractedReceipt {
            source_filename: filename.to_owned(),
            line,
            extras: Extras::default(),
        }
    }

    #[test]
    fn empty_report_yields_many_required_field_issues() {
        let report = ExpenseReport::default();
        let result = validate_typed(&report);
        // Empty report has no transaction_date, no payee, etc. — at least
        // some required fields should be flagged.
        assert!(
            !result.issues.is_empty(),
            "expected at least one issue on an empty report"
        );
        assert!(
            result.issues.iter().any(|i| i.kind == ValidationIssueKind::MissingRequiredField),
            "expected at least one MissingRequiredField issue"
        );
    }

    #[test]
    fn reduced_real_report_has_predictable_t1_gaps() {
        // Build a report from one synthetic receipt — the FA hasn't filled
        // any of the T1 general-information fields yet, so we expect those
        // specific issues but NOT issues about line-level extracted fields
        // (date / amount / venue are present in our typed model).
        let receipts = vec![sample_meal_receipt("a.jpeg", "2026-04-19", 100.00)];
        let report = reduce_to_expense_report(&receipts);
        let result = validate_typed(&report);

        let paths: Vec<&str> = result.issues.iter().map(|i| i.path.as_str()).collect();

        // The category was inferred (domestic) so it should NOT be missing.
        assert!(
            !paths.contains(&"expense_report.general_information.category"),
            "category should be filled by reduction; saw issues: {paths:?}"
        );
        // Payee name is T3 but the model didn't extract it from a meal
        // receipt → empty → required → should be flagged.
        assert!(
            paths.contains(&"expense_report.general_information.payee.name"),
            "payee.name should be flagged as missing; saw issues: {paths:?}"
        );
    }

    #[test]
    fn parses_equals_expression() {
        let e = parse_expression("expense_type == car_rental").unwrap();
        match e {
            Expr::Equals { path, value } => {
                assert_eq!(path, vec!["expense_type"]);
                assert_eq!(value, "car_rental");
            }
            _ => panic!("expected Equals"),
        }
    }

    #[test]
    fn parses_in_expression() {
        let e = parse_expression("expense_type in [airfare_domestic, airfare_foreign]").unwrap();
        match e {
            Expr::In { path, values } => {
                assert_eq!(path, vec!["expense_type"]);
                assert_eq!(values, vec!["airfare_domestic", "airfare_foreign"]);
            }
            _ => panic!("expected In"),
        }
    }

    #[test]
    fn meal_details_required_when_expense_type_matches() {
        let receipts = vec![sample_meal_receipt("a.jpeg", "2026-04-19", 100.00)];
        let report = reduce_to_expense_report(&receipts);
        let result = validate_typed(&report);
        let paths: Vec<&str> = result.issues.iter().map(|i| i.path.as_str()).collect();
        // meal_details is present (we attached it in the sample), so the
        // conditional rule "expense_type in [business_meal, ...] →
        // meal_details required" should NOT fire.
        assert!(
            !paths.iter().any(|p| p.ends_with(".meal_details")),
            "meal_details was present; conditional rule should not fire. saw: {paths:?}"
        );
    }

    #[test]
    fn airfare_details_required_when_expense_type_is_airfare() {
        // Build a line claiming to be an airfare line but without
        // airfare_details. Conditional rule should fire.
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.date = Wrapped { value: Some(IsoDate("2026-04-19".into())), meta: FieldMetadata::default() };
        line.common.line_amount_usd = Wrapped { value: Some(500.0), meta: FieldMetadata::default() };
        line.common.expense_type = Wrapped {
            value: Some(ExpenseReportTransactionLinesItemCommonExpenseTypeEnum::AirfareDomestic),
            meta: FieldMetadata::default(),
        };
        // line.airfare_details is None.
        let mut report = ExpenseReport::default();
        report.transaction_lines = Some(vec![line]);

        let result = validate_typed(&report);
        let paths: Vec<&str> = result.issues.iter().map(|i| i.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with(".airfare_details")),
            "expected airfare_details to be flagged; saw: {paths:?}"
        );
    }

    /// Helper for the country-consistency tests: build a meal line with
    /// the given country_of_activity (None for unknown).
    fn meal_line_with_country(amount: f64, country: Option<&str>) -> ExpenseReportTransactionLinesItem {
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.expense_type = Wrapped {
            value: Some(ExpenseReportTransactionLinesItemCommonExpenseTypeEnum::BusinessMeal),
            meta: FieldMetadata::default(),
        };
        line.common.line_amount_usd = Wrapped { value: Some(amount), meta: FieldMetadata::default() };
        line.common.country_of_activity = match country {
            Some(c) => Wrapped { value: Some(c.to_owned()), meta: FieldMetadata::default() },
            None => Wrapped::unknown(),
        };
        line
    }

    fn report_with(
        category: ExpenseReportGeneralInformationCategoryEnum,
        lines: Vec<ExpenseReportTransactionLinesItem>,
    ) -> ExpenseReport {
        let mut report = ExpenseReport::default();
        report.general_information.category = Wrapped {
            value: Some(category),
            meta: FieldMetadata::default(),
        };
        report.transaction_lines = Some(lines);
        report
    }

    fn country_consistency_warnings(report: &ExpenseReport) -> Vec<String> {
        validate_typed(report)
            .issues
            .into_iter()
            .filter(|i| i.severity == ValidationSeverity::Warning
                && i.kind == ValidationIssueKind::ManualReviewRequired)
            .map(|i| i.path)
            .collect()
    }

    #[test]
    fn category_country_consistency_domestic_with_us_lines_clean() {
        let report = report_with(
            ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic,
            vec![
                meal_line_with_country(100.0, Some("United States")),
                meal_line_with_country(200.0, Some("United States")),
            ],
        );
        assert!(country_consistency_warnings(&report).is_empty());
    }

    #[test]
    fn category_country_consistency_domestic_with_foreign_line_warns() {
        let report = report_with(
            ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic,
            vec![
                meal_line_with_country(100.0, Some("United States")),
                meal_line_with_country(50.0, Some("Canada")),
            ],
        );
        let paths = country_consistency_warnings(&report);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].contains("transaction_lines[1].common.country_of_activity"));
    }

    #[test]
    fn category_country_consistency_skips_null_countries() {
        // A line with no country signal must not trigger the rule
        // (e.g. extractor couldn't infer the country). Mixed with one
        // valid US line — should still be clean.
        let report = report_with(
            ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic,
            vec![
                meal_line_with_country(100.0, Some("United States")),
                meal_line_with_country(50.0, None),
            ],
        );
        assert!(country_consistency_warnings(&report).is_empty());
    }

    #[test]
    fn category_country_consistency_foreign_with_at_least_one_foreign_line_clean() {
        // Foreign trip with a domestic taxi line is fine (FA may have
        // had a US airport ride). At-least-one foreign line satisfies
        // the rule.
        let report = report_with(
            ExpenseReportGeneralInformationCategoryEnum::ExpensesForeign,
            vec![
                meal_line_with_country(50.0, Some("United States")),
                meal_line_with_country(200.0, Some("Singapore")),
            ],
        );
        assert!(country_consistency_warnings(&report).is_empty());
    }

    #[test]
    fn category_country_consistency_foreign_with_only_us_lines_warns() {
        // Category claims foreign but no line has any foreign signal
        // (neither non-US country nor non-USD currency) — surfaces a
        // category warning, not a per-line one.
        let report = report_with(
            ExpenseReportGeneralInformationCategoryEnum::ExpensesForeign,
            vec![
                meal_line_with_country(100.0, Some("United States")),
                meal_line_with_country(50.0, None),
            ],
        );
        let paths = country_consistency_warnings(&report);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], "expense_report.general_information.category");
    }

    #[test]
    fn category_country_consistency_foreign_with_non_usd_currency_clean() {
        // Production scenario from Phase 4 Stage 7: Air India BOM→SFO
        // ticket has country_of_activity="United States" (per the airfare
        // prompt: "country of FURTHEST destination") but original_currency
        // ="INR" — the report IS legitimately foreign by currency. The
        // check should accept currency as a foreign signal independently
        // of country, and NOT flag the category card.
        let mut foreign_currency_line = meal_line_with_country(970.0, Some("United States"));
        foreign_currency_line.common.original_currency = Wrapped {
            value: Some("INR".to_owned()),
            meta: FieldMetadata::default(),
        };
        let report = report_with(
            ExpenseReportGeneralInformationCategoryEnum::ExpensesForeign,
            vec![
                foreign_currency_line,
                meal_line_with_country(100.0, Some("United States")),
            ],
        );
        let paths = country_consistency_warnings(&report);
        assert!(
            !paths.iter().any(|p| p == "expense_report.general_information.category"),
            "category should NOT warn when at least one line has non-USD currency; saw: {paths:?}"
        );
    }

    #[test]
    fn original_currency_required_when_line_expense_type_is_foreign() {
        // Per-line scope: the conditional rule for original_currency is
        // `expense_type in [airfare_foreign, lodging_foreign, ...]`. A line
        // whose own expense_type is foreign must have original_currency
        // filled, regardless of the report-level category.
        let mut report = ExpenseReport::default();
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.expense_type = Wrapped {
            value: Some(ExpenseReportTransactionLinesItemCommonExpenseTypeEnum::AirfareForeign),
            meta: FieldMetadata::default(),
        };
        line.common.line_amount_usd = Wrapped { value: Some(100.0), meta: FieldMetadata::default() };
        // original_currency.value is None — should be flagged.
        report.transaction_lines = Some(vec![line]);

        let result = validate_typed(&report);
        let paths: Vec<&str> = result.issues.iter().map(|i| i.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.contains("original_currency")),
            "expected original_currency to be flagged on a foreign line; saw: {paths:?}"
        );
    }

    #[test]
    fn original_currency_not_required_on_domestic_line_in_foreign_report() {
        // The bug this regression-tests: previously the rule was scoped to
        // `general_information.category == expenses_foreign`, so a USD-
        // domestic line in a mixed-currency report (one foreign receipt
        // tipping the report's category to expenses_foreign) got falsely
        // flagged as missing original_currency. Per-line scope means the
        // domestic line's own expense_type drives the requirement, not the
        // report-wide category.
        let mut report = ExpenseReport::default();
        report.general_information.category = Wrapped {
            value: Some(ExpenseReportGeneralInformationCategoryEnum::ExpensesForeign),
            meta: FieldMetadata::default(),
        };
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.expense_type = Wrapped {
            value: Some(ExpenseReportTransactionLinesItemCommonExpenseTypeEnum::AirfareDomestic),
            meta: FieldMetadata::default(),
        };
        line.common.line_amount_usd = Wrapped { value: Some(100.0), meta: FieldMetadata::default() };
        // original_currency.value is None — but the LINE is domestic, so
        // it must NOT be flagged even though the report is foreign overall.
        report.transaction_lines = Some(vec![line]);

        let result = validate_typed(&report);
        let paths: Vec<&str> = result.issues.iter().map(|i| i.path.as_str()).collect();
        assert!(
            !paths.iter().any(|p| p.contains("original_currency")),
            "domestic line should not flag original_currency even in a foreign report; saw: {paths:?}"
        );
    }
}
