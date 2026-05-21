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

/// Render the report's transaction lines as a CSV string matching
/// Stanford's live "Expense Lines Upload" portal column order (per
/// the screenshot the FA shared 2026-05-20):
///
///   Line | Expense Date | Expense Currency | Expense Amount |
///   USD Amount | Expense Type | Remarks
///
/// Line numbers auto-increment from 1. Date format is DD-MMM-YYYY
/// (e.g. `02-Sep-2024`). Expense Currency is the ISO 4217 code +
/// full name (e.g. `BRL - Brazilian Real`); USD-only lines emit
/// `USD - US Dollar`. Expense Amount is the receipt's original
/// currency value; USD Amount is the converted value. For
/// USD-printed receipts these are the same.
///
/// Lines with missing data still emit (with blanks in those
/// columns) so the FA sees/fixes them in Excel rather than silently
/// dropping data.
pub fn report_to_lines_csv(report: &ExpenseReport) -> String {
    let mut out = String::from(
        "Line,Expense Date,Expense Currency,Expense Amount,USD Amount,Expense Type,Remarks\n",
    );
    let lines = match report.transaction_lines.as_ref() {
        Some(v) => v,
        None => return out,
    };
    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let date = line
            .common
            .date
            .value
            .as_ref()
            .map(|d| format_portal_date(&d.0))
            .unwrap_or_default();
        let original_currency = line
            .common
            .original_currency
            .value
            .as_deref();
        let usd_amount = line.common.line_amount_usd.value;
        // Foreign lines have original_currency + original_amount; USD
        // lines leave original_* null. Portal wants both columns
        // populated even for USD — emit USD code + the USD amount.
        let (currency_display, expense_amount) = if let Some(code) = original_currency {
            let amt = line
                .common
                .original_amount
                .value
                .map(|v| format!("{v:.2}"))
                .unwrap_or_default();
            (currency_code_to_full(code), amt)
        } else {
            (
                currency_code_to_full("USD"),
                usd_amount.map(|v| format!("{v:.2}")).unwrap_or_default(),
            )
        };
        let usd_str = usd_amount.map(|v| format!("{v:.2}")).unwrap_or_default();
        let expense_type = map_expense_type(line);
        let remarks = line.common.remarks.value.as_deref().unwrap_or("");
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            line_no,
            csv_field(&date),
            csv_field(&currency_display),
            csv_field(&expense_amount),
            csv_field(&usd_str),
            csv_field(expense_type),
            csv_field(remarks),
        ));
    }
    out
}

/// Convert ISO 8601 date `YYYY-MM-DD` → portal format `DD-MMM-YYYY`
/// (e.g. `2024-09-02` → `02-Sep-2024`). Falls back to passthrough
/// for malformed input so a bad date doesn't lose the row entirely
/// (FA can fix in Excel).
fn format_portal_date(iso: &str) -> String {
    if iso.len() != 10 || &iso[4..5] != "-" || &iso[7..8] != "-" {
        return iso.to_owned();
    }
    let Ok(month_idx) = iso[5..7].parse::<usize>() else {
        return iso.to_owned();
    };
    if !(1..=12).contains(&month_idx) {
        return iso.to_owned();
    }
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{}-{}-{}", &iso[8..10], MONTHS[month_idx - 1], &iso[0..4])
}

/// ISO 4217 currency code → portal display string (`<code> - <name>`).
/// Covers the ~40 currencies a Stanford research traveller realistically
/// encounters. Unknown codes fall back to `<code> - <code>` so the row
/// still emits with a recognizable currency column the FA can correct.
fn currency_code_to_full(code: &str) -> String {
    let name = match code {
        "USD" => "US Dollar",
        "EUR" => "Euro",
        "GBP" => "British Pound",
        "JPY" => "Japanese Yen",
        "CAD" => "Canadian Dollar",
        "AUD" => "Australian Dollar",
        "CHF" => "Swiss Franc",
        "CNY" => "Chinese Yuan",
        "SGD" => "Singapore Dollar",
        "INR" => "Indian Rupee",
        "BRL" => "Brazilian Real",
        "MXN" => "Mexican Peso",
        "ARS" => "Argentine Peso",
        "KRW" => "South Korean Won",
        "HKD" => "Hong Kong Dollar",
        "TWD" => "Taiwan Dollar",
        "THB" => "Thai Baht",
        "IDR" => "Indonesian Rupiah",
        "ZAR" => "South African Rand",
        "TRY" => "Turkish Lira",
        "ILS" => "Israeli Shekel",
        "AED" => "UAE Dirham",
        "SAR" => "Saudi Riyal",
        "NZD" => "New Zealand Dollar",
        "SEK" => "Swedish Krona",
        "NOK" => "Norwegian Krone",
        "DKK" => "Danish Krone",
        "PLN" => "Polish Zloty",
        "CZK" => "Czech Koruna",
        "HUF" => "Hungarian Forint",
        "RON" => "Romanian Leu",
        "VND" => "Vietnamese Dong",
        "PHP" => "Philippine Peso",
        "MYR" => "Malaysian Ringgit",
        "CLP" => "Chilean Peso",
        "COP" => "Colombian Peso",
        "PEN" => "Peruvian Sol",
        "EGP" => "Egyptian Pound",
        unknown => return format!("{unknown} - {unknown}"),
    };
    format!("{code} - {name}")
}

/// Concatenate the business_purpose sub-fields into one labeled text
/// blob the FA can paste into Stanford's report-level Business Purpose
/// box. Missing fields are skipped (rather than emitting `Who: \n`)
/// so the blob stays readable.
///
/// Now consumed by the workbench renderer, which embeds the result
/// as a single click-to-copy field card alongside the 6 individual
/// sub-cards (Stage 3 of the 2026-05-20 FA feedback round) — the
/// download-then-copy .txt flow was bad UX.
pub fn business_purpose_to_text(
    bp: &crate::expense_report_model::ExpenseReportGeneralInformationBusinessPurpose,
) -> String {
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
/// string the FA's Stanford portal expects.
///
/// **Updated 2026-05-20 against the live portal screenshot** the FA
/// shared during the meeting (`.scratch/img_1132.jpg`). Where the
/// screenshot directly showed a value (`Lodging - Foreign and
/// Domestic`, `Airfare - Foreign and Domestic`) we use that exact
/// string. Other types that have fore/dom variants in our schema get
/// the same suffix by symmetry (Ground Transportation); types with
/// only one variant or non-territorial types keep their .xlsm
/// taxonomy string. FA review at next upload catches any wrong ones;
/// the mapping is one function so updates are one edit.
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
        // Screenshot-confirmed: portal value is "Airfare - Foreign and Domestic".
        ExpenseType::AirfareDomestic | ExpenseType::AirfareForeign => {
            "Airfare - Foreign and Domestic"
        }
        ExpenseType::AncillaryAirlineFee => "Ancillary Airline Fee",
        ExpenseType::BusinessMeal => {
            if alcohol { "Business Meal with Alcohol" } else { "Business Meal" }
        }
        ExpenseType::CarRental => "Car Rental",
        ExpenseType::ConferenceRegistration => "Conference Registration",
        ExpenseType::GiftCardEmployeeForeign => "Gift Card - Employee",
        ExpenseType::GiftsForeignActivity => "Gifts",
        // Symmetry with Airfare + Lodging — same fore/dom collapse pattern.
        ExpenseType::GroundTransportationDomestic
        | ExpenseType::GroundTransportationForeign => "Ground Transportation - Foreign and Domestic",
        ExpenseType::GroupTravelMeal => {
            if alcohol { "Group Travel Meal with Alcohol" } else { "Group Travel Meal" }
        }
        ExpenseType::HumanSubjectIncentive => "Human Subject Incentive",
        // Screenshot-confirmed: portal value is "Lodging - Foreign and Domestic".
        ExpenseType::LodgingDomestic | ExpenseType::LodgingForeign => {
            "Lodging - Foreign and Domestic"
        }
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
        ExpenseReportGeneralInformationBusinessPurpose,
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

    fn line_with_currency(
        date: &str,
        usd: f64,
        original_currency: Option<&str>,
        original_amount: Option<f64>,
        kind: ExpenseType,
        remarks: &str,
    ) -> ExpenseReportTransactionLinesItem {
        let mut line = line_with(date, usd, kind, remarks);
        if let Some(code) = original_currency {
            line.common.original_currency = Wrapped {
                value: Some(code.to_owned()),
                meta: FieldMetadata::default(),
            };
        }
        if let Some(amt) = original_amount {
            line.common.original_amount = Wrapped {
                value: Some(amt),
                meta: FieldMetadata::default(),
            };
        }
        line
    }

    #[test]
    fn csv_emits_seven_column_header_and_portal_date_format() {
        let mut report = ExpenseReport::default();
        report.transaction_lines = Some(vec![
            line_with("2024-09-02", 970.75, ExpenseType::AirfareForeign, "BOM to SFO"),
            line_with("2024-09-04", 79.59, ExpenseType::BusinessMeal, "Dinner"),
        ]);
        let csv = report_to_lines_csv(&report);
        let expected = "\
Line,Expense Date,Expense Currency,Expense Amount,USD Amount,Expense Type,Remarks
1,02-Sep-2024,USD - US Dollar,970.75,970.75,Airfare - Foreign and Domestic,BOM to SFO
2,04-Sep-2024,USD - US Dollar,79.59,79.59,Business Meal,Dinner
";
        assert_eq!(csv, expected);
    }

    #[test]
    fn csv_foreign_currency_emits_original_amount_and_full_currency_name() {
        let mut report = ExpenseReport::default();
        report.transaction_lines = Some(vec![line_with_currency(
            "2026-04-23",
            104.18,
            Some("BRL"),
            Some(519.20),
            ExpenseType::LodgingForeign,
            "Early check-in charge",
        )]);
        let csv = report_to_lines_csv(&report);
        // The portal screenshot row 1: BRL - Brazilian Real, 519.20, 104.18.
        assert!(
            csv.contains("1,23-Apr-2026,BRL - Brazilian Real,519.20,104.18,Lodging - Foreign and Domestic,"),
            "csv:\n{csv}"
        );
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
        // Empty fields, no expense_type → Miscellaneous fallback. Line still emits
        // (FA can fix in Excel) with line number 1, USD currency default, blanks
        // elsewhere.
        assert_eq!(
            csv,
            "Line,Expense Date,Expense Currency,Expense Amount,USD Amount,Expense Type,Remarks\n\
             1,,USD - US Dollar,,,Miscellaneous,\n"
        );
    }

    #[test]
    fn csv_header_only_when_no_lines() {
        let report = ExpenseReport::default();
        let csv = report_to_lines_csv(&report);
        assert_eq!(
            csv,
            "Line,Expense Date,Expense Currency,Expense Amount,USD Amount,Expense Type,Remarks\n"
        );
    }

    #[test]
    fn portal_date_format_round_trip() {
        assert_eq!(format_portal_date("2024-09-02"), "02-Sep-2024");
        assert_eq!(format_portal_date("2026-04-23"), "23-Apr-2026");
        assert_eq!(format_portal_date("2026-01-01"), "01-Jan-2026");
        assert_eq!(format_portal_date("2026-12-31"), "31-Dec-2026");
    }

    #[test]
    fn portal_date_format_passes_through_malformed_input() {
        // Don't lose the row if the date is garbage — FA fixes in Excel.
        assert_eq!(format_portal_date(""), "");
        assert_eq!(format_portal_date("April 23 2026"), "April 23 2026");
        assert_eq!(format_portal_date("2026-13-01"), "2026-13-01");
        assert_eq!(format_portal_date("2026/04/23"), "2026/04/23");
    }

    #[test]
    fn currency_code_known_returns_code_dash_name() {
        assert_eq!(currency_code_to_full("USD"), "USD - US Dollar");
        assert_eq!(currency_code_to_full("BRL"), "BRL - Brazilian Real");
        assert_eq!(currency_code_to_full("JPY"), "JPY - Japanese Yen");
        assert_eq!(currency_code_to_full("EGP"), "EGP - Egyptian Pound");
    }

    #[test]
    fn currency_code_unknown_falls_back_to_code_dash_code() {
        // Don't drop the row for an unknown 3-letter code — emit it twice so
        // the FA sees what we received and can correct.
        assert_eq!(currency_code_to_full("XYZ"), "XYZ - XYZ");
        assert_eq!(currency_code_to_full(""), " - ");
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
        let mut bp = ExpenseReportGeneralInformationBusinessPurpose::default();
        bp.who = Wrapped { value: Some("Jane Doe".to_owned()), meta: FieldMetadata::default() };
        bp.what = Wrapped { value: Some("Presented research".to_owned()), meta: FieldMetadata::default() };
        bp.when = Wrapped { value: Some("2024-09-01 to 2024-09-05".to_owned()), meta: FieldMetadata::default() };
        bp.r#where = Wrapped { value: Some("Buenos Aires".to_owned()), meta: FieldMetadata::default() };
        bp.why = Wrapped { value: Some("Disseminate".to_owned()), meta: FieldMetadata::default() };
        bp.key_30char = Wrapped { value: Some("ISCA 2024".to_owned()), meta: FieldMetadata::default() };

        let text = business_purpose_to_text(&bp);
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
        let mut bp = ExpenseReportGeneralInformationBusinessPurpose::default();
        bp.who = Wrapped { value: Some("Jane".to_owned()), meta: FieldMetadata::default() };
        // Leave the other 5 as None.
        let text = business_purpose_to_text(&bp);
        assert_eq!(text, "Who: Jane\n");
    }

    #[test]
    fn business_purpose_text_empty_when_no_bp_filled() {
        let bp = ExpenseReportGeneralInformationBusinessPurpose::default();
        let text = business_purpose_to_text(&bp);
        assert_eq!(text, "");
    }
}
