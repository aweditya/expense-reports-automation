//! FA-entered values from the upload-form fieldset (FA Input — Phase 5
//! scope completion; see docs/fa-input-plan.md).
//!
//! The Flask app writes `fa_input.json` alongside each upload's files;
//! `reduce_extractions --fa-input <path>` parses it and applies the
//! values to the report's `general_information` block, then propagates
//! `foreign_activity_type` to every foreign-typed transaction line.
//!
//! All fields are optional in the JSON — the FA may have skipped
//! optional fields, or this entry point may not exist at all (the
//! reducer treats `--fa-input` as optional and falls back to the
//! existing no-FA-input flow).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::expense_report_model::{
    ExpenseReport, ExpenseReportGeneralInformationPayeeAffiliationEnum,
    ExpenseReportGeneralInformationRushProcessingEnum,
    ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum,
};
use crate::meta::{ConfidenceLevel, EvidenceKind, EvidenceReference, FieldMetadata, Wrapped};

/// Origin string used in every `user_input` evidence entry FA-input
/// produces. The workbench reads this to badge the field as "FA-entered."
pub const FA_ORIGIN: &str = "fa_upload_form";

/// What the Flask form's POST handler writes to `fa_input.json`.
/// Field names match the form's HTML `name=` attributes; values are
/// strings since the form has no type information at HTTP time.
/// Enum-like fields (`payee_affiliation`, `rush_processing`,
/// `foreign_activity_type`) are validated + parsed in `apply_to_report`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FaInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee_sunet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee_affiliation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business_purpose_who: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business_purpose_what: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business_purpose_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business_purpose_where: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business_purpose_why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rush_processing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_activity_type: Option<String>,
}

/// Parse `fa_input.json` from disk. Returns the typed struct or a
/// human-readable error string suitable for printing to stderr in the
/// reducer binary.
pub fn parse_path(path: &Path) -> Result<FaInput, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Apply FA-entered values to the report's `general_information` block
/// and propagate `foreign_activity_type` to every foreign transaction
/// line.
///
/// - Each FA field with `Some(value)` overwrites the corresponding
///   report field with `kind: user_input` evidence.
/// - FA fields with `None` are left as-is (the field keeps whatever
///   value the reducer / extractor produced — typically the empty
///   `Wrapped::default()`).
/// - Enum-like fields (affiliation, rush_processing, foreign_activity_type)
///   parse leniently: unknown strings are silently ignored so a typo in
///   the form doesn't crash the pipeline.
pub fn apply_to_report(report: &mut ExpenseReport, fa: &FaInput) {
    let gi = &mut report.general_information;

    if let Some(v) = &fa.payee_name {
        gi.payee.name = fa_wrapped(v.clone());
    }
    if let Some(v) = &fa.payee_sunet {
        // T1 source → bare Option<String>; codegen pattern for FA-only
        // fields (same as authorized_by). No Wrapped/_meta wrapping.
        gi.payee.sunet = Some(v.clone());
    }
    if let Some(s) = &fa.payee_affiliation {
        if let Some(e) = parse_payee_affiliation(s) {
            gi.payee.affiliation = fa_wrapped(e);
        }
    }
    if let Some(v) = &fa.event_name {
        gi.event_name = fa_wrapped(v.clone());
    }
    if let Some(v) = &fa.business_purpose_who {
        gi.business_purpose.who = fa_wrapped(v.clone());
    }
    if let Some(v) = &fa.business_purpose_what {
        gi.business_purpose.what = fa_wrapped(v.clone());
    }
    if let Some(v) = &fa.business_purpose_when {
        gi.business_purpose.when = fa_wrapped(v.clone());
    }
    if let Some(v) = &fa.business_purpose_where {
        gi.business_purpose.r#where = fa_wrapped(v.clone());
    }
    if let Some(v) = &fa.business_purpose_why {
        gi.business_purpose.why = fa_wrapped(v.clone());
    }
    // authorized_by is a bare Option<String> in the codegen'd model —
    // no metadata wrapper. FA-entered renders as label+value only in
    // the workbench (no "FA-entered" badge). Same for rush_processing.
    // Future schema work could promote these to Wrapped<T>; not in scope
    // for this stage. See docs/fa-input-plan.md §6.
    if let Some(v) = &fa.authorized_by {
        gi.authorized_by = Some(v.clone());
    }
    if let Some(s) = &fa.rush_processing {
        if let Some(e) = parse_rush_processing(s) {
            gi.rush_processing = Some(e);
        }
    }
    if let Some(v) = &fa.payment_method {
        gi.payment_method = fa_wrapped(v.clone());
    }

    // foreign_activity_type propagates to every transaction line whose
    // expense_type identifies it as foreign. Domestic lines stay None
    // (correct per the schema's conditional rule that only foreign
    // lines require this field).
    if let Some(s) = &fa.foreign_activity_type {
        if let Some(fat) = parse_foreign_activity_type(s) {
            if let Some(lines) = report.transaction_lines.as_mut() {
                for line in lines.iter_mut() {
                    if line_is_foreign(line) {
                        line.common.foreign_activity_type = fa_wrapped(fat);
                    }
                }
            }
        }
    }
}

/// Construct a `Wrapped<T>` with `kind: user_input` evidence and an
/// origin string identifying the FA upload form.
fn fa_wrapped<T>(value: T) -> Wrapped<T> {
    Wrapped {
        value: Some(value),
        meta: FieldMetadata {
            confidence: ConfidenceLevel::High,
            evidence: vec![EvidenceReference {
                kind: EvidenceKind::UserInput,
                document_id: None,
                filename: None,
                page: None,
                quote: None,
                origin: Some(FA_ORIGIN.to_owned()),
                bboxes: None,
                token_ids: None,
            }],
            needs_review: false,
            flags: Vec::new(),
            confidence_reason: Some("Provided by FA on upload form.".to_owned()),
        },
    }
}

/// String -> enum, accepting both the canonical snake_case form
/// (matches `as_str()` round-trip) AND the dropdown's human form
/// (e.g. "Stanford Student"). Unknown strings return None so the
/// reducer can leave the field unset.
fn parse_payee_affiliation(s: &str) -> Option<ExpenseReportGeneralInformationPayeeAffiliationEnum> {
    use ExpenseReportGeneralInformationPayeeAffiliationEnum as A;
    match normalize(s).as_str() {
        "stanford_student" | "student" => Some(A::StanfordStudent),
        "stanford_postdoc" | "postdoc" => Some(A::StanfordPostdoc),
        "stanford_faculty" | "faculty" => Some(A::StanfordFaculty),
        "stanford_staff" | "staff" => Some(A::StanfordStaff),
        "other" => Some(A::Other),
        _ => None,
    }
}

fn parse_rush_processing(s: &str) -> Option<ExpenseReportGeneralInformationRushProcessingEnum> {
    use ExpenseReportGeneralInformationRushProcessingEnum as R;
    match normalize(s).as_str() {
        "yes" | "true" => Some(R::Yes),
        "no" | "false" => Some(R::No),
        _ => None,
    }
}

fn parse_foreign_activity_type(
    s: &str,
) -> Option<ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum> {
    use ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum as F;
    match normalize(s).as_str() {
        "conference" => Some(F::Conference),
        "research_collaboration" => Some(F::ResearchCollaboration),
        "fieldwork" => Some(F::Fieldwork),
        "other" => Some(F::Other),
        _ => None,
    }
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase().replace(' ', "_")
}

/// Round-trips the canonical `business_purpose.when` string produced by
/// `scripts/local_app_simple.py::write_fa_input` into `(start, end)`.
/// Accepts `"YYYY-MM-DD"` (single day → `(d, d)`) or
/// `"YYYY-MM-DD to YYYY-MM-DD"` (range). Returns `None` for any other
/// shape, including pre-calendar freeform strings the FA might have
/// typed before the date-picker landed — validator skips the
/// date-window check in that case rather than guessing.
pub fn parse_when_window(s: &str) -> Option<(String, String)> {
    let trimmed = s.trim();
    if let Some((from, to)) = trimmed.split_once(" to ") {
        let f = from.trim();
        let t = to.trim();
        if is_iso_date(f) && is_iso_date(t) {
            return Some((f.to_owned(), t.to_owned()));
        }
        return None;
    }
    if is_iso_date(trimmed) {
        return Some((trimmed.to_owned(), trimmed.to_owned()));
    }
    None
}

fn is_iso_date(s: &str) -> bool {
    // Cheap shape check — 10 chars, YYYY-MM-DD. Doesn't validate Feb 30
    // etc.; the lexicographic compare in the validator stays correct
    // either way (Feb-30 just sorts as a date that doesn't exist).
    s.len() == 10
        && s.as_bytes()[4] == b'-'
        && s.as_bytes()[7] == b'-'
        && s[..4].chars().all(|c| c.is_ascii_digit())
        && s[5..7].chars().all(|c| c.is_ascii_digit())
        && s[8..10].chars().all(|c| c.is_ascii_digit())
}

/// True when this transaction line's expense_type identifies it as a
/// foreign-categorized expense. Uses the enum's snake_case
/// representation ending in `_foreign` to be future-proof against new
/// foreign-typed expense kinds added to the schema. Matches what the
/// schema's conditional rule on `foreign_activity_type` requires.
fn line_is_foreign(line: &crate::expense_report_model::ExpenseReportTransactionLinesItem) -> bool {
    line.common
        .expense_type
        .value
        .as_ref()
        .map(|e| e.as_str().ends_with("_foreign"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expense_report_model::{
        ExpenseReport, ExpenseReportGeneralInformationPayeeAffiliationEnum as Aff,
        ExpenseReportGeneralInformationRushProcessingEnum as Rush,
        ExpenseReportTransactionLinesItemCommonExpenseTypeEnum as ET,
        ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum as FAT,
    };

    fn full_fa_input() -> FaInput {
        FaInput {
            payee_name: Some("Test Researcher".to_owned()),
            payee_sunet: Some("testresearcher".to_owned()),
            payee_affiliation: Some("stanford_postdoc".to_owned()),
            event_name: Some("ASPLOS 2026".to_owned()),
            business_purpose_who: Some("Payee + collaborators".to_owned()),
            business_purpose_what: Some("Presenting research".to_owned()),
            business_purpose_when: Some("March 14-19 2026".to_owned()),
            business_purpose_where: Some("Pittsburgh, PA".to_owned()),
            business_purpose_why: Some("Conference participation".to_owned()),
            authorized_by: Some("Advisor".to_owned()),
            rush_processing: Some("no".to_owned()),
            payment_method: Some("PCard".to_owned()),
            foreign_activity_type: Some("conference".to_owned()),
        }
    }

    #[test]
    fn applies_general_info_fields_with_user_input_evidence() {
        let mut report = ExpenseReport::default();
        apply_to_report(&mut report, &full_fa_input());
        let gi = &report.general_information;
        assert_eq!(gi.payee.name.value.as_deref(), Some("Test Researcher"));
        assert_eq!(gi.payee.affiliation.value, Some(Aff::StanfordPostdoc));
        assert_eq!(gi.event_name.value.as_deref(), Some("ASPLOS 2026"));
        assert_eq!(gi.business_purpose.who.value.as_deref(), Some("Payee + collaborators"));
        assert_eq!(gi.authorized_by.as_deref(), Some("Advisor"));
        assert_eq!(gi.rush_processing, Some(Rush::No));
        assert_eq!(gi.payment_method.value.as_deref(), Some("PCard"));

        // Every Wrapped<T> FA field should carry user_input evidence
        // (the bare Options — authorized_by, rush_processing — have no
        // metadata to check).
        let ev = &gi.payee.name.meta.evidence[0];
        assert_eq!(ev.kind, EvidenceKind::UserInput);
        assert_eq!(ev.origin.as_deref(), Some(FA_ORIGIN));
        assert_eq!(gi.payee.name.meta.confidence, ConfidenceLevel::High);
    }

    #[test]
    fn missing_fa_fields_leave_report_unchanged() {
        let mut report = ExpenseReport::default();
        apply_to_report(&mut report, &FaInput::default());
        // Wrapped<T> stays at default (None value, default meta).
        assert_eq!(report.general_information.payee.name.value, None);
        assert!(report.general_information.payee.name.meta.evidence.is_empty());
        // bare Option<T> stays None.
        assert!(report.general_information.authorized_by.is_none());
        assert!(report.general_information.rush_processing.is_none());
    }

    #[test]
    fn unknown_enum_strings_are_silently_ignored() {
        let mut report = ExpenseReport::default();
        let fa = FaInput {
            payee_affiliation: Some("not_a_real_affiliation".to_owned()),
            rush_processing: Some("definitely_maybe".to_owned()),
            foreign_activity_type: Some("typo_here".to_owned()),
            ..Default::default()
        };
        apply_to_report(&mut report, &fa);
        // None of the enum fields should have been set.
        assert_eq!(report.general_information.payee.affiliation.value, None);
        assert_eq!(report.general_information.rush_processing, None);
    }

    #[test]
    fn enum_parser_accepts_both_snake_case_and_human_form() {
        assert_eq!(
            parse_payee_affiliation("stanford_faculty"),
            Some(Aff::StanfordFaculty)
        );
        assert_eq!(parse_payee_affiliation("Faculty"), Some(Aff::StanfordFaculty));
        assert_eq!(parse_payee_affiliation("Stanford Faculty"), Some(Aff::StanfordFaculty));
        assert_eq!(parse_payee_affiliation("  staff  "), Some(Aff::StanfordStaff));
        assert_eq!(parse_payee_affiliation("bogus"), None);
    }

    #[test]
    fn foreign_activity_type_propagates_only_to_foreign_lines() {
        use crate::expense_report_model::{
            ExpenseReportTransactionLinesItem,
            ExpenseReportTransactionLinesItemCommon,
        };
        let mut report = ExpenseReport::default();
        report.transaction_lines = Some(vec![
            ExpenseReportTransactionLinesItem {
                common: ExpenseReportTransactionLinesItemCommon {
                    expense_type: Wrapped {
                        value: Some(ET::AirfareForeign),
                        meta: FieldMetadata::default(),
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            ExpenseReportTransactionLinesItem {
                common: ExpenseReportTransactionLinesItemCommon {
                    expense_type: Wrapped {
                        value: Some(ET::BusinessMeal),  // domestic-ish
                        meta: FieldMetadata::default(),
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            ExpenseReportTransactionLinesItem {
                common: ExpenseReportTransactionLinesItemCommon {
                    expense_type: Wrapped {
                        value: Some(ET::LodgingForeign),
                        meta: FieldMetadata::default(),
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        ]);

        let fa = FaInput {
            foreign_activity_type: Some("conference".to_owned()),
            ..Default::default()
        };
        apply_to_report(&mut report, &fa);

        let lines = report.transaction_lines.as_ref().unwrap();
        // Foreign airfare: set
        assert_eq!(lines[0].common.foreign_activity_type.value, Some(FAT::Conference));
        // Domestic business meal: NOT set
        assert_eq!(lines[1].common.foreign_activity_type.value, None);
        // Foreign lodging: set
        assert_eq!(lines[2].common.foreign_activity_type.value, Some(FAT::Conference));
    }

    #[test]
    fn serde_round_trip_preserves_all_fields() {
        // Non-negotiable #5: tests don't write to /tmp (env::temp_dir
        // resolves to /var/folders on macOS — same prohibition).
        // Pure in-memory round-trip via serde_json::to/from_string.
        let fa = full_fa_input();
        let json = serde_json::to_string(&fa).expect("serialize");
        let parsed: FaInput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, fa);
    }

    #[test]
    fn missing_optional_fields_deserialize_as_none() {
        let json = r#"{"payee_name": "Solo Name"}"#;
        let fa: FaInput = serde_json::from_str(json).expect("deserialize");
        assert_eq!(fa.payee_name.as_deref(), Some("Solo Name"));
        assert_eq!(fa.event_name, None);
        assert_eq!(fa.foreign_activity_type, None);
    }

    #[test]
    fn parse_when_window_handles_range() {
        let got = parse_when_window("2026-03-15 to 2026-03-19");
        assert_eq!(got, Some(("2026-03-15".to_owned(), "2026-03-19".to_owned())));
    }

    #[test]
    fn parse_when_window_collapses_single_day_to_same_endpoints() {
        let got = parse_when_window("2026-03-15");
        assert_eq!(got, Some(("2026-03-15".to_owned(), "2026-03-15".to_owned())));
    }

    #[test]
    fn parse_when_window_rejects_freeform_text() {
        // Pre-calendar FA input. Validator must skip the date check
        // rather than fabricate a window from a guess.
        assert_eq!(parse_when_window("March 14-19 2026"), None);
        assert_eq!(parse_when_window("June 2024"), None);
        assert_eq!(parse_when_window("2026/03/15"), None);
        assert_eq!(parse_when_window(""), None);
    }

    #[test]
    fn parse_when_window_rejects_partial_iso_dates() {
        assert_eq!(parse_when_window("2026-3-15"), None);
        assert_eq!(parse_when_window("2026-03-1"), None);
        assert_eq!(parse_when_window("2026-03-15 to 2026/03/19"), None);
    }
}
