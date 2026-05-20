//! CSV + business-purpose-text exports for the FA's downstream workflow.
//!
//! The Stanford expense-report portal has two distinct entry surfaces:
//!   1. **Expense Lines Upload** — bulk CSV import of line items.
//!      Columns: `Date | Amount | Expense Type | Remarks`. Schema
//!      from the FA-supplied `Copy of ERS Template.xlsm`.
//!   2. **Business Purpose** — a single multi-line text box on the
//!      report-level form.
//!
//! Our internal schema separates these (per-line `common.*` for the
//! CSV; report-level `general_information.business_purpose.{who,what,
//! when,where,why,key_30char}` for the text box). This module produces
//! both artifacts as plain strings; the render binary writes them to
//! disk alongside `workbench.html`, and the workbench links to them.
//!
//! Expense Type mapping is best-effort: our internal enum doesn't 1:1
//! match Stanford's taxonomy (e.g. we have one `business_meal` vs
//! Stanford's `Business Meal` / `Travel Meal - Single Meal` distinction).
//! The mapping lives in one function for easy FA-policy updates; the
//! workbench's per-line review surfaces any wrong mapping for the FA
//! to fix in Excel before uploading.

use crate::expense_report_model::{
    ExpenseReport,
    ExpenseReportTransactionLinesItem,
    ExpenseReportTransactionLinesItemCommonExpenseTypeEnum as ExpenseType,
};

/// Render the report's transaction lines as a CSV string matching the
/// Stanford ERS Template column order. Always includes the header row.
/// Lines with missing date or amount still emit (with blanks) so the
/// FA can see/fix them in Excel rather than silently dropping data.
pub fn report_to_lines_csv(report: &ExpenseReport) -> String {
    let mut out = String::from("Date,Amount,Expense Type,Remarks\n");
    let lines = match report.transaction_lines.as_ref() {
        Some(v) => v,
        None => return out,
    };
    for line in lines {
        let date = line
            .common
            .date
            .value
            .as_ref()
            .map(|d| d.0.as_str())
            .unwrap_or("");
        let amount = line
            .common
            .line_amount_usd
            .value
            .map(|v| format!("{v:.2}"))
            .unwrap_or_default();
        let expense_type = map_expense_type(line);
        let remarks = line
            .common
            .remarks
            .value
            .as_deref()
            .unwrap_or("");
        out.push_str(&format!(
            "{},{},{},{}\n",
            csv_field(date),
            csv_field(&amount),
            csv_field(expense_type),
            csv_field(remarks),
        ));
    }
    out
}

/// Concatenate the report's business_purpose sub-fields into one
/// labeled text blob the FA can paste into Stanford's report-level
/// Business Purpose box. Missing fields are skipped (rather than
/// emitting `Who: \n`) so the blob stays readable.
pub fn report_to_business_purpose_text(report: &ExpenseReport) -> String {
    let bp = &report.general_information.business_purpose;
    let mut out = String::new();
    let pairs: [(&str, Option<&String>); 6] = [
        ("Who", bp.who.value.as_ref()),
        ("What", bp.what.value.as_ref()),
        ("When", bp.when.value.as_ref()),
        ("Where", bp.r#where.value.as_ref()),
        ("Why", bp.why.value.as_ref()),
        ("Key (≤30 chars)", bp.key_30char.value.as_ref()),
    ];
    for (label, value) in pairs {
        if let Some(v) = value {
            if !v.is_empty() {
                out.push_str(&format!("{label}: {v}\n"));
            }
        }
    }
    out
}

/// Internal-enum → Stanford-taxonomy mapping. Returns the literal
/// string the FA's Stanford portal expects (must match the dropdown
/// values in `ERS Template.xlsm` exactly; misspellings will be
/// rejected at upload time).
///
/// Takes `&ExpenseReportTransactionLinesItem` so meal lines can
/// promote to "with Alcohol" variants when `meal_details.
/// has_alcohol_on_receipt == true`.
fn map_expense_type(line: &ExpenseReportTransactionLinesItem) -> &'static str {
    let Some(kind) = line.common.expense_type.value.as_ref() else {
        return "Miscellaneous";
    };
    let alcohol = line
        .meal_details
        .as_ref()
        .and_then(|m| m.has_alcohol_on_receipt.value)
        .unwrap_or(false);
    match kind {
        ExpenseType::AdjustedPerDiem => "Adjusted Per Diem",
        ExpenseType::AirfareDomestic | ExpenseType::AirfareForeign => "Airfare",
        ExpenseType::AncillaryAirlineFee => "Ancillary Airline Fee",
        ExpenseType::BusinessMeal => {
            if alcohol { "Business Meal with Alcohol" } else { "Business Meal" }
        }
        ExpenseType::CarRental => "Car Rental",
        ExpenseType::ConferenceRegistration => "Conference Registration",
        ExpenseType::GiftCardEmployeeForeign => "Gift Card - Employee",
        ExpenseType::GiftsForeignActivity => "Gifts",
        ExpenseType::GroundTransportationDomestic
        | ExpenseType::GroundTransportationForeign => "Ground Transportation",
        ExpenseType::GroupTravelMeal => {
            if alcohol { "Group Travel Meal with Alcohol" } else { "Group Travel Meal" }
        }
        ExpenseType::HumanSubjectIncentive => "Human Subject Incentive",
        ExpenseType::LodgingDomestic | ExpenseType::LodgingForeign => "Lodging",
        ExpenseType::OtherBusinessExpense => "Miscellaneous",
    }
}

/// RFC 4180 field quoting: a field needs quoting if it contains a
/// comma, quote, or newline. Embedded quotes are doubled.
fn csv_field(s: &str) -> String {
    let needs_quote = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');
    if !needs_quote {
        return s.to_owned();
    }
    let escaped = s.replace('"', "\"\"");
    format!("\"{escaped}\"")
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::expense_report_model::{
        ExpenseReportGeneralInformationBusinessPurpose, ExpenseReportTransactionLinesItemCommon,
        ExpenseReportTransactionLinesItemMealDetails, IsoDate,
    };
    use crate::meta::{FieldMetadata, Wrapped};

    fn line_with(date: &str, amount: f64, kind: ExpenseType, remarks: &str) -> ExpenseReportTransactionLinesItem {
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
            value: Some(kind),
            meta: FieldMetadata::default(),
        };
        line.common.remarks = Wrapped {
            value: Some(remarks.to_owned()),
            meta: FieldMetadata::default(),
        };
        line
    }

    fn with_alcohol(mut line: ExpenseReportTransactionLinesItem, alcohol: bool) -> ExpenseReportTransactionLinesItem {
        let mut md = ExpenseReportTransactionLinesItemMealDetails::default();
        md.has_alcohol_on_receipt = Wrapped {
            value: Some(alcohol),
            meta: FieldMetadata::default(),
        };
        line.meal_details = Some(md);
        line
    }

    #[test]
    fn csv_emits_header_then_rows_in_template_order() {
        let mut report = ExpenseReport::default();
        report.transaction_lines = Some(vec![
            line_with("2024-09-02", 1234.56, ExpenseType::AirfareForeign, "BOM to SFO"),
            line_with("2024-09-04", 79.59, ExpenseType::BusinessMeal, "Dinner"),
        ]);
        let csv = report_to_lines_csv(&report);
        let expected = "\
Date,Amount,Expense Type,Remarks
2024-09-02,1234.56,Airfare,BOM to SFO
2024-09-04,79.59,Business Meal,Dinner
";
        assert_eq!(csv, expected);
    }

    #[test]
    fn csv_promotes_business_meal_with_alcohol() {
        let mut report = ExpenseReport::default();
        let line = with_alcohol(
            line_with("2024-09-02", 100.00, ExpenseType::BusinessMeal, ""),
            true,
        );
        report.transaction_lines = Some(vec![line]);
        let csv = report_to_lines_csv(&report);
        assert!(csv.contains("Business Meal with Alcohol"),
                "CSV did not promote with-alcohol meal:\n{csv}");
    }

    #[test]
    fn csv_quotes_remarks_with_commas_and_quotes() {
        let mut report = ExpenseReport::default();
        report.transaction_lines = Some(vec![
            line_with("2024-09-02", 50.00, ExpenseType::BusinessMeal, "Lunch with A, B, C"),
            line_with("2024-09-03", 60.00, ExpenseType::BusinessMeal, "Dinner at \"The Spot\""),
            line_with("2024-09-04", 70.00, ExpenseType::BusinessMeal, "Multi\nline\nremark"),
        ]);
        let csv = report_to_lines_csv(&report);
        assert!(csv.contains("\"Lunch with A, B, C\""), "missing comma-quoted: {csv}");
        assert!(csv.contains("\"Dinner at \"\"The Spot\"\"\""), "missing double-quote escape: {csv}");
        assert!(csv.contains("\"Multi\nline\nremark\""), "missing newline-quoted: {csv}");
    }

    #[test]
    fn csv_emits_blank_fields_rather_than_dropping_lines() {
        let mut report = ExpenseReport::default();
        let mut line = ExpenseReportTransactionLinesItem::default();
        // No date, no amount, no expense_type, no remarks set.
        line.common.expense_type = Wrapped::default();
        report.transaction_lines = Some(vec![line]);
        let csv = report_to_lines_csv(&report);
        // Empty fields, no expense_type → Miscellaneous fallback.
        assert_eq!(csv, "Date,Amount,Expense Type,Remarks\n,,Miscellaneous,\n");
    }

    #[test]
    fn csv_header_only_when_no_lines() {
        let report = ExpenseReport::default();
        let csv = report_to_lines_csv(&report);
        assert_eq!(csv, "Date,Amount,Expense Type,Remarks\n");
    }

    #[test]
    fn expense_type_mapping_covers_every_enum_variant() {
        // Compile-time guard: this match must be exhaustive. If a new
        // variant is added to ExpenseType and not mapped, this test
        // won't even compile. Catches schema additions that need an
        // FA-policy decision.
        use ExpenseType::*;
        for kind in [
            AdjustedPerDiem, AirfareDomestic, AirfareForeign,
            AncillaryAirlineFee, BusinessMeal, CarRental,
            ConferenceRegistration, GiftCardEmployeeForeign,
            GiftsForeignActivity, GroundTransportationDomestic,
            GroundTransportationForeign, GroupTravelMeal,
            HumanSubjectIncentive, LodgingDomestic, LodgingForeign,
            OtherBusinessExpense,
        ] {
            let mut line = ExpenseReportTransactionLinesItem::default();
            line.common.expense_type = Wrapped { value: Some(kind), meta: FieldMetadata::default() };
            let mapped = map_expense_type(&line);
            assert!(!mapped.is_empty(), "empty mapping for {kind:?}");
        }
    }

    #[test]
    fn business_purpose_text_concatenates_with_labels() {
        let mut report = ExpenseReport::default();
        let mut bp = ExpenseReportGeneralInformationBusinessPurpose::default();
        bp.who = Wrapped { value: Some("Jane Doe".to_owned()), meta: FieldMetadata::default() };
        bp.what = Wrapped { value: Some("Presented research".to_owned()), meta: FieldMetadata::default() };
        bp.when = Wrapped { value: Some("2024-09-01 to 2024-09-05".to_owned()), meta: FieldMetadata::default() };
        bp.r#where = Wrapped { value: Some("Buenos Aires".to_owned()), meta: FieldMetadata::default() };
        bp.why = Wrapped { value: Some("Disseminate".to_owned()), meta: FieldMetadata::default() };
        bp.key_30char = Wrapped { value: Some("ISCA 2024".to_owned()), meta: FieldMetadata::default() };
        report.general_information.business_purpose = bp;

        let text = report_to_business_purpose_text(&report);
        let expected = "\
Who: Jane Doe
What: Presented research
When: 2024-09-01 to 2024-09-05
Where: Buenos Aires
Why: Disseminate
Key (≤30 chars): ISCA 2024
";
        assert_eq!(text, expected);
    }

    #[test]
    fn business_purpose_text_skips_missing_fields() {
        let mut report = ExpenseReport::default();
        let mut bp = ExpenseReportGeneralInformationBusinessPurpose::default();
        bp.who = Wrapped { value: Some("Jane".to_owned()), meta: FieldMetadata::default() };
        // Leave the other 5 as None.
        report.general_information.business_purpose = bp;
        let text = report_to_business_purpose_text(&report);
        assert_eq!(text, "Who: Jane\n");
    }

    #[test]
    fn business_purpose_text_empty_when_no_bp_filled() {
        let report = ExpenseReport::default();
        let text = report_to_business_purpose_text(&report);
        assert_eq!(text, "");
    }
}
