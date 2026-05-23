//! CSV + business-purpose-text exports for the FA's downstream workflow.
//!
//! Stanford's expense-report portal has **two distinct upload pages**, each
//! with its own column layout and its own Expense Type dropdown vocabulary.
//! We emit one CSV per page, every time:
//!
//!   * `lines-domestic.csv` — 7 columns matching the live "Expense Lines
//!     Upload" page (per the FA's 2026-05-20 screenshot). Plain expense
//!     type strings (`Airfare`, `Lodging`, `Ground Transportation`, …).
//!   * `lines-foreign.csv` — 20 columns matching the foreign-page template
//!     (per `reference/ers-template-foreign.xlsx` Sheet1). Suffixed
//!     expense type strings (`Airfare - Foreign and Domestic`,
//!     `Ground Transportation-Foreign`, `Travel Meal-SingleMealwAlcohl`, …
//!     including the missing-`o` typo Stanford really has).
//!
//! Per-line routing: each transaction line goes to exactly one CSV. Lines
//! whose `expense_type` is an explicit foreign variant
//! (`*_foreign`) go to the foreign CSV; explicit domestic variants go to
//! the domestic CSV; neutral types (`business_meal`, `car_rental`, …)
//! fall through to the report's `general_information.category` (foreign
//! report ⇒ foreign CSV; anything else ⇒ domestic CSV).
//!
//! Stanford reports are typically all-domestic or all-foreign — a mixed
//! report is rare. So one of the two CSVs is usually header-only; the
//! workbench hero hides empty downloads.
//!
//! Source-of-truth artifacts (checked in under `reference/`):
//!   * `ers-template-foreign.xlsx` — column layout + 25 valid foreign
//!     expense types + all foreign dropdown values (currencies,
//!     affiliations, booking methods, ticket classes, …).
//!   * `ers-expense-type-dropdown-domestic.png` — the 26 domestic
//!     dropdown values, screenshotted from the live portal.
//!
//! Strings here are pasted verbatim from those artifacts (typos and all,
//! e.g. `Travel Meal-SingleMealwAlcohl` and `Adminstrative: Academic
//! Support`). Stanford's dropdown is case- and character-sensitive.

use crate::airport_codes::airport_code_to_full;
use crate::expense_report_model::{
    ExpenseReport,
    ExpenseReportGeneralInformationCategoryEnum as Category,
    ExpenseReportGeneralInformationPayeeAffiliationEnum as Affiliation,
    ExpenseReportTransactionLinesItem,
    ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum as AirfareBookingMethod,
    ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum as ClassOfTicket,
    ExpenseReportTransactionLinesItemCommonExpenseTypeEnum as ExpenseType,
    ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum as ForeignActivityType,
    ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum as LodgingBookingMethod,
};

// ─── Public emitters ────────────────────────────────────────────────────────

/// Render the domestic-page CSV (7 columns). Includes only the lines
/// routed to the domestic page per [`route_to_foreign`].
///
///   Line | Expense Date | Expense Currency | Expense Amount | USD Amount
///        | Expense Type | Remarks
///
/// Line numbers auto-increment from 1 within this file.
pub fn report_to_domestic_csv(report: &ExpenseReport) -> String {
    let mut out = String::from(
        "Line,Expense Date,Expense Currency,Expense Amount,USD Amount,Expense Type,Remarks\n",
    );
    let lines = match report.transaction_lines.as_ref() {
        Some(v) => v,
        None => return out,
    };
    let category = report.general_information.category.value.as_ref();
    let mut line_no = 0usize;
    for line in lines {
        if route_to_foreign(line, category) {
            continue;
        }
        line_no += 1;
        let date = line.common.date.value.as_ref()
            .map(|d| format_portal_date(&d.0))
            .unwrap_or_default();
        let usd_amount = line.common.line_amount_usd.value;
        let (currency_display, expense_amount) =
            if let Some(code) = line.common.original_currency.value.as_deref() {
                let amt = line.common.original_amount.value
                    .map(|v| format!("{v:.2}")).unwrap_or_default();
                (currency_code_to_full(code), amt)
            } else {
                (currency_code_to_full("USD"),
                 usd_amount.map(|v| format!("{v:.2}")).unwrap_or_default())
            };
        let usd_str = usd_amount.map(|v| format!("{v:.2}")).unwrap_or_default();
        let expense_type = map_expense_type_domestic(line);
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

/// Render the foreign-page CSV (20 columns). Includes only the lines
/// routed to the foreign page per [`route_to_foreign`].
///
/// Column layout from `reference/ers-template-foreign.xlsx` Sheet1 r2.
/// Note: no Line column, no USD Amount column (foreign page wants the
/// original-currency amount only).
pub fn report_to_foreign_csv(report: &ExpenseReport) -> String {
    let mut out = String::from(
        // Exact header strings from xlsx Sheet1 r2 — preserve the snake_case,
        // the lowercase "country of activity" / "activity type", the two
        // spaces in "ticket_number  (not required)", and the Title-Case
        // "Conference Hotel".
        "category,expense_date,expense_currency,expense_amount,expense_type,remarks,\
         country of activity,activity type,affiliation,traveler_sunet,traveler_name,\
         ticket_number  (not required),travel_booking_method,airline,class_of_ticket,\
         departure_airport,destination_airport,number_of_nights,location,Conference Hotel\n",
    );
    let lines = match report.transaction_lines.as_ref() {
        Some(v) => v,
        None => return out,
    };
    let category = report.general_information.category.value.as_ref();
    let payee = &report.general_information.payee;
    let affiliation_str = payee.affiliation.value.as_ref()
        .map(|a| map_affiliation_foreign(*a))
        .unwrap_or("");
    let traveler_name = payee.name.value.as_deref().unwrap_or("");
    // payee.sunet is Option<String> (T1 source = bare option, not Wrapped).
    let traveler_sunet = payee.sunet.as_deref().unwrap_or("");

    for line in lines {
        if !route_to_foreign(line, category) {
            continue;
        }
        // Stanford requires affiliation + traveler_sunet + traveler_name
        // ONLY on Airfare rows (per ers-template-foreign-example-filled.xlsx,
        // which leaves these cells blank on lodging + meal rows). Emitting
        // them on non-airfare rows is harmless but inconsistent and may
        // trigger Stanford-side validation; gate them on expense_type.
        let is_airfare = matches!(
            line.common.expense_type.value.as_ref(),
            Some(ExpenseType::AirfareDomestic) | Some(ExpenseType::AirfareForeign)
        );
        // A — category. The xlsx hint at A1 is "Expenses (Foreign)" and
        // that's what every row gets on the foreign page.
        let col_a = "Expenses (Foreign)";
        // B — expense_date
        let col_b = line.common.date.value.as_ref()
            .map(|d| format_portal_date(&d.0))
            .unwrap_or_default();
        // C — expense_currency. Foreign page wants the original-currency
        // code+name (`BRL - Brazilian Real`). USD-only lines emit
        // `USD - US Dollar`.
        let currency_code = line.common.original_currency.value.as_deref()
            .unwrap_or("USD");
        let col_c = currency_code_to_full(currency_code);
        // D — expense_amount (ORIGINAL currency, not USD). For USD-only
        // lines this is the USD amount.
        let col_d = if line.common.original_currency.value.is_some() {
            line.common.original_amount.value
                .map(|v| format!("{v:.2}")).unwrap_or_default()
        } else {
            line.common.line_amount_usd.value
                .map(|v| format!("{v:.2}")).unwrap_or_default()
        };
        // E — expense_type
        let col_e = map_expense_type_foreign(line);
        // F — remarks
        let col_f = line.common.remarks.value.as_deref().unwrap_or("");
        // G — country of activity (string passthrough; blank if absent)
        let col_g = line.common.country_of_activity.value.as_deref().unwrap_or("");
        // H — activity type (enum mapped)
        let col_h = line.common.foreign_activity_type.value.as_ref()
            .map(|a| map_foreign_activity_type(*a))
            .unwrap_or("");
        // I — affiliation (only on airfare rows per Stanford's filled example)
        let col_i = if is_airfare { affiliation_str } else { "" };
        // J — traveler_sunet (only on airfare rows)
        let col_j = if is_airfare { traveler_sunet } else { "" };
        // K — traveler_name (only on airfare rows)
        let col_k = if is_airfare { traveler_name } else { "" };
        // L–Q — airfare details (blank when not an airfare line).
        // Airport cols (P, Q) use the Stanford-portal full display
        // string ("SFO - San Francisco International (San Francisco, ...)")
        // looked up from generated/airport_codes.json. Unknown codes
        // fall back to the raw IATA so the row still emits.
        let (col_l, col_m, col_n, col_o, col_p, col_q) =
            if let Some(af) = line.airfare_details.as_ref() {
                let departure = af.departure_airport.value.as_deref().unwrap_or("");
                let destination = af.destination_airport.value.as_deref().unwrap_or("");
                (
                    af.ticket_number.value.as_deref().unwrap_or("").to_owned(),
                    af.booking_method.value.as_ref()
                        .map(|b| map_airfare_booking_method_foreign(*b).to_owned())
                        .unwrap_or_default(),
                    af.airline.value.as_deref().unwrap_or("").to_owned(),
                    af.class_of_ticket.value.as_ref()
                        .map(|c| map_class_of_ticket_foreign(*c).to_owned())
                        .unwrap_or_default(),
                    airport_code_to_full(departure).unwrap_or_else(|| departure.to_owned()),
                    airport_code_to_full(destination).unwrap_or_else(|| destination.to_owned()),
                )
            } else {
                (String::new(), String::new(), String::new(),
                 String::new(), String::new(), String::new())
            };
        // R–T — lodging details (blank if not a lodging line)
        let (col_r, col_s, col_t) =
            if let Some(ld) = line.lodging_details.as_ref() {
                let nights = ld.number_of_nights.value
                    .map(|n| format!("{n}")).unwrap_or_default();
                let location = ld.location.value.as_deref().unwrap_or("").to_owned();
                // T — Conference Hotel: "Yes" if lodging booking_method is
                // conference_hotel; else "No". Blank only when lodging_details
                // is absent (handled by outer else).
                let hotel = ld.booking_method.value.as_ref()
                    .map(|b| if matches!(b, LodgingBookingMethod::ConferenceHotel)
                             { "Yes" } else { "No" })
                    .unwrap_or("No");
                (nights, location, hotel.to_owned())
            } else {
                (String::new(), String::new(), String::new())
            };

        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_field(col_a),
            csv_field(&col_b),
            csv_field(&col_c),
            csv_field(&col_d),
            csv_field(col_e),
            csv_field(col_f),
            csv_field(col_g),
            csv_field(col_h),
            csv_field(col_i),
            csv_field(col_j),
            csv_field(col_k),
            csv_field(&col_l),
            csv_field(&col_m),
            csv_field(&col_n),
            csv_field(&col_o),
            csv_field(&col_p),
            csv_field(&col_q),
            csv_field(&col_r),
            csv_field(&col_s),
            csv_field(&col_t),
        ));
    }
    out
}

/// Count how many lines route to each CSV. Used by the workbench hero to
/// hide an empty-CSV download link.
pub fn line_counts(report: &ExpenseReport) -> (usize, usize) {
    let Some(lines) = report.transaction_lines.as_ref() else { return (0, 0); };
    let category = report.general_information.category.value.as_ref();
    let (mut domestic, mut foreign) = (0usize, 0usize);
    for line in lines {
        if route_to_foreign(line, category) { foreign += 1; } else { domestic += 1; }
    }
    (domestic, foreign)
}

// ─── Routing ───────────────────────────────────────────────────────────────

/// True iff this line belongs in the foreign CSV (vs the domestic CSV).
/// Explicit foreign variants always go foreign; explicit domestic
/// variants always go domestic; neutral types tie-break on the report's
/// category.
fn route_to_foreign(
    line: &ExpenseReportTransactionLinesItem,
    report_category: Option<&Category>,
) -> bool {
    let Some(kind) = line.common.expense_type.value.as_ref() else {
        return matches_foreign_category(report_category);
    };
    match kind {
        ExpenseType::AirfareForeign
        | ExpenseType::LodgingForeign
        | ExpenseType::GroundTransportationForeign
        | ExpenseType::GiftCardEmployeeForeign
        | ExpenseType::GiftsForeignActivity => true,
        ExpenseType::AirfareDomestic
        | ExpenseType::LodgingDomestic
        | ExpenseType::GroundTransportationDomestic => false,
        // Neutral (BusinessMeal, ConferenceRegistration, …): inherit
        // the report's category.
        _ => matches_foreign_category(report_category),
    }
}

fn matches_foreign_category(c: Option<&Category>) -> bool {
    matches!(c, Some(Category::ExpensesForeign))
}

// ─── Expense-type mappers (per-page SSOT) ──────────────────────────────────

/// Internal enum → domestic-portal Expense Type string. SSOT:
/// `reference/ers-expense-type-dropdown-domestic.png`.
fn map_expense_type_domestic(line: &ExpenseReportTransactionLinesItem) -> &'static str {
    let Some(kind) = line.common.expense_type.value.as_ref() else {
        return "Miscellaneous";
    };
    let alcohol = line.meal_details.as_ref()
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
        ExpenseType::MembershipDues => "Membership Dues",
        ExpenseType::OtherBusinessExpense => "Miscellaneous",
        // B2: best-guess label. Stanford's actual dropdown string for
        // mileage should be confirmed against the ERS portal once an
        // FA submits a personal-mileage line for the first time.
        ExpenseType::PersonalMileage => "Personal Mileage",
    }
}

/// Internal enum → foreign-portal Expense Type string. SSOT:
/// `reference/ers-template-foreign.xlsx` Sheet "Expense Type" (25 values).
///
/// Several internal variants don't have a clean 1:1 in the foreign
/// dropdown; we pick the closest match. The xlsx's typos and
/// unconventional dash spacing (`Ground Transportation-Foreign` —
/// no spaces around the dash) are preserved exactly.
fn map_expense_type_foreign(line: &ExpenseReportTransactionLinesItem) -> &'static str {
    let Some(kind) = line.common.expense_type.value.as_ref() else {
        return "Miscellaneous - Foreign";
    };
    let alcohol = line.meal_details.as_ref()
        .and_then(|m| m.has_alcohol_on_receipt.value)
        .unwrap_or(false);
    match kind {
        ExpenseType::AdjustedPerDiem => "Adjusted Per Diem",
        ExpenseType::AirfareDomestic
        | ExpenseType::AirfareForeign => "Airfare - Foreign and Domestic",
        ExpenseType::AncillaryAirlineFee => "Ancillary Airline Fee",
        ExpenseType::BusinessMeal => {
            if alcohol { "Business Meal with Alcohol" } else { "Business Meal" }
        }
        ExpenseType::CarRental => "Car Rental",
        ExpenseType::ConferenceRegistration => "Conference Registration",
        ExpenseType::GiftCardEmployeeForeign => "Gift Card - Employee (Foreign)",
        ExpenseType::GiftsForeignActivity => "Gifts - Foreign Activity",
        ExpenseType::GroundTransportationDomestic
        | ExpenseType::GroundTransportationForeign => "Ground Transportation-Foreign",
        // Foreign page spells it "w Alcohol" not "with Alcohol".
        ExpenseType::GroupTravelMeal => {
            if alcohol { "Group Travel Meal w Alcohol" } else { "Group Travel Meal" }
        }
        ExpenseType::HumanSubjectIncentive => "Human Subject Incentive",
        ExpenseType::LodgingDomestic
        | ExpenseType::LodgingForeign => "Lodging - Foreign and Domestic",
        ExpenseType::MembershipDues => "Membership Dues - Foreign",
        ExpenseType::OtherBusinessExpense => "Miscellaneous - Foreign",
        // B2: personal mileage on foreign trips is uncommon (IRS rate
        // is US-specific) but the mapper needs an arm. Best-guess
        // label; verify against Stanford's foreign-CSV template.
        ExpenseType::PersonalMileage => "Personal Mileage - Foreign",
    }
}

// ─── Foreign-page enum mappers ─────────────────────────────────────────────

fn map_affiliation_foreign(a: Affiliation) -> &'static str {
    // Foreign dropdown is just {DAPER Traveler, Visitor, Stanford Traveler}.
    // Our schema's stanford_* variants all collapse to "Stanford Traveler".
    // We have no DAPER-specific signal; FA fixes in Excel if needed.
    match a {
        Affiliation::StanfordStudent
        | Affiliation::StanfordPostdoc
        | Affiliation::StanfordFaculty
        | Affiliation::StanfordStaff => "Stanford Traveler",
        Affiliation::Other => "Visitor",
    }
}

fn map_airfare_booking_method_foreign(b: AirfareBookingMethod) -> &'static str {
    // The xlsx collapses all the per-airline UA/DL/AA/AS/HA Connect
    // variants into one dropdown value.
    match b {
        AirfareBookingMethod::StanfordTravelEgencia => "Stanford Travel - Egencia",
        AirfareBookingMethod::StanfordTravelKeyTravel => "Stanford Travel - Key Travel",
        AirfareBookingMethod::StanfordTravelConnectUa
        | AirfareBookingMethod::StanfordTravelConnectDl
        | AirfareBookingMethod::StanfordTravelConnectAa
        | AirfareBookingMethod::StanfordTravelConnectAs
        | AirfareBookingMethod::StanfordTravelConnectHa => {
            "Stanford Travel Connect (UA, DL, AA, AS and HA)"
        }
        AirfareBookingMethod::Other => "Other (Not a Stanford Travel Booking Method)",
    }
}

fn map_class_of_ticket_foreign(c: ClassOfTicket) -> &'static str {
    // The xlsx dropdown collapses Premium Economy and Business into
    // one option.
    match c {
        ClassOfTicket::Coach => "Coach",
        ClassOfTicket::First => "First",
        ClassOfTicket::PremiumEconomy | ClassOfTicket::Business => "Premium Economy/Business",
    }
}

fn map_foreign_activity_type(a: ForeignActivityType) -> &'static str {
    // Maps to entries in xlsx Sheet "Activity Type" (16 values). Our
    // schema enum is much smaller; other → "Other".
    match a {
        ForeignActivityType::Conference => "Conferences",
        ForeignActivityType::ResearchCollaboration => "Collaborations/Meetings",
        ForeignActivityType::Fieldwork => "Research: Field work",
        ForeignActivityType::Other => "Other",
    }
}

// ─── Date + currency formatters ────────────────────────────────────────────

/// Convert ISO 8601 date `YYYY-MM-DD` → portal format `DD-MMM-YYYY`
/// (e.g. `2024-09-02` → `02-Sep-2024`). Falls back to passthrough for
/// malformed input so a bad date doesn't lose the row.
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
/// 75-entry table from `reference/ers-template-foreign.xlsx` Sheet
/// "Expense_Currency". Capitalization is intentionally inconsistent
/// (`Argentine peso`, `South Korean won`, `New Taiwan dollar`,
/// `Angolanische Kwanza`) — preserved exactly as Stanford has them.
/// Unknown codes fall back to `<code> - <code>` so the row still emits.
fn currency_code_to_full(code: &str) -> String {
    let name = match code {
        "AED" => "United Arab Emirates Dirham",
        "ALL" => "Albanian Lek",
        "AOA" => "Angolanische Kwanza",
        "ARS" => "Argentine peso",
        "AUD" => "Australian Dollar",
        "BDT" => "Bangladeshi Taka",
        "BGN" => "Bulgarian Lev",
        "BRL" => "Brazilian Real",
        "BWP" => "Botswana Pula",
        "CAD" => "Canadian Dollar",
        "CHF" => "Swiss Franc",
        "CLP" => "Chilean Peso",
        "CNY" => "Chinese Yuan Renminbi",
        "COP" => "Colombian Peso",
        "CRC" => "Costa Rican Colon",
        "CZK" => "Czech Koruna",
        "DKK" => "Danish Krone",
        "DOP" => "Dominican Peso",
        "EGP" => "Egyptian Pound",
        "EUR" => "Euro",
        "GBP" => "British Pound",
        "GEL" => "Georgian Lari",
        "GTQ" => "Guatemalan Quetzal",
        "HKD" => "Hong Kong Dollar",
        "HUF" => "Hungarian Forint",
        "IDR" => "Indonesian Rupiah",
        "ILS" => "Israeli Shekel",
        "INR" => "Indian Rupee",
        "ISK" => "Icelandic Krona",
        "JMD" => "Jamaican Dollar",
        "JOD" => "Jordanian Dinars",
        "JPY" => "Japanese Yen",
        "KES" => "Kenyan Shilling",
        "KRW" => "South Korean won",
        "KWD" => "Kuwaiti Dinar",
        "KYD" => "Cayman Islands Dollar",
        "KZT" => "Kazakhstan Tenge",
        "LKR" => "Sri Lankan Rupee",
        "LTL" => "Lithuanian Litas",
        "LYD" => "Libyan Dinar",
        "MAD" => "Moroccan Dirham",
        "MGA" => "Malagasy Ariary",
        "MOP" => "Macanese Pataca",
        "MXN" => "Mexican Peso",
        "MYR" => "Malaysian Ringgit",
        "NAD" => "Namibian Dollar",
        "NGN" => "Nigerian Naira",
        "NIO" => "Nicaraguan Córdoba",
        "NOK" => "Norwegian Krone",
        "NPR" => "Nepalese Rupee",
        "NZD" => "New Zealand Dollar",
        "OMR" => "Omani Rial",
        "PEN" => "Sol",
        "PHP" => "Philippine Peso",
        "PLN" => "Polish Zloty",
        "PYG" => "Paraguay Guarani",
        "QAR" => "Qatari Rial",
        "RON" => "Romanian Leu",
        "RSD" => "Serbian Dinar",
        "SAR" => "Saudi Riyal",
        "SEK" => "Swedish Krona",
        "SGD" => "Singapore Dollar",
        "THB" => "Thai Baht",
        "TRY" => "Turkish Lira",
        "TTD" => "Trinidad and Tobago Dollar",
        "TWD" => "New Taiwan dollar",
        "TZS" => "Tanzanian Schilling",
        "UGX" => "Ugandan Shilling",
        "USD" => "US Dollar",
        "UYU" => "Uruguayan Peso",
        "VEF" => "Venezuelan Bolivar Fuerte",
        "VND" => "Vietnamese Dong",
        "XOF" => "West African CFA Franc",
        "ZAR" => "South African Rand",
        "ZWD" => "Zimbabwe Dollar",
        unknown => return format!("{unknown} - {unknown}"),
    };
    format!("{code} - {name}")
}

// ─── Business purpose text export ──────────────────────────────────────────

/// Concatenate the business_purpose sub-fields into one labeled text
/// blob the FA can paste into Stanford's report-level Business Purpose
/// box. Missing fields are skipped.
pub fn business_purpose_to_text(
    bp: &crate::expense_report_model::ExpenseReportGeneralInformationBusinessPurpose,
) -> String {
    let mut out = String::new();
    let pairs: [(&str, Option<&String>); 5] = [
        ("Who", bp.who.value.as_ref()),
        ("What", bp.what.value.as_ref()),
        ("When", bp.when.value.as_ref()),
        ("Where", bp.r#where.value.as_ref()),
        ("Why", bp.why.value.as_ref()),
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

// ─── Internals ─────────────────────────────────────────────────────────────

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
        ExpenseReportTransactionLinesItemAirfareDetails,
        ExpenseReportTransactionLinesItemLodgingDetails,
        ExpenseReportTransactionLinesItemMealDetails, IsoDate,
    };
    use crate::meta::{FieldMetadata, Wrapped};

    // ─── Test helpers ──────────────────────────────────────────────────────

    fn wrap<T>(v: T) -> Wrapped<T> {
        Wrapped { value: Some(v), meta: FieldMetadata::default() }
    }

    fn line_with(
        date: &str,
        amount: f64,
        kind: ExpenseType,
        remarks: &str,
    ) -> ExpenseReportTransactionLinesItem {
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.date = wrap(IsoDate(date.to_owned()));
        line.common.line_amount_usd = wrap(amount);
        line.common.expense_type = wrap(kind);
        line.common.remarks = wrap(remarks.to_owned());
        line
    }

    fn with_alcohol(
        mut line: ExpenseReportTransactionLinesItem,
        alcohol: bool,
    ) -> ExpenseReportTransactionLinesItem {
        let mut md = ExpenseReportTransactionLinesItemMealDetails::default();
        md.has_alcohol_on_receipt = wrap(alcohol);
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
            line.common.original_currency = wrap(code.to_owned());
        }
        if let Some(amt) = original_amount {
            line.common.original_amount = wrap(amt);
        }
        line
    }

    fn report_with_category(
        category: Option<Category>,
        lines: Vec<ExpenseReportTransactionLinesItem>,
    ) -> ExpenseReport {
        let mut r = ExpenseReport::default();
        if let Some(c) = category {
            r.general_information.category = wrap(c);
        }
        r.transaction_lines = Some(lines);
        r
    }

    // ─── Domestic CSV ──────────────────────────────────────────────────────

    #[test]
    fn domestic_csv_emits_seven_column_header_and_uses_plain_expense_types() {
        let report = report_with_category(
            Some(Category::ExpensesDomestic),
            vec![
                line_with("2024-09-02", 970.75, ExpenseType::AirfareDomestic, "BOM to SFO"),
                line_with("2024-09-04", 79.59, ExpenseType::BusinessMeal, "Dinner"),
                line_with("2024-09-03", 200.00, ExpenseType::LodgingDomestic, "Hotel"),
            ],
        );
        let csv = report_to_domestic_csv(&report);
        let expected = "\
Line,Expense Date,Expense Currency,Expense Amount,USD Amount,Expense Type,Remarks
1,02-Sep-2024,USD - US Dollar,970.75,970.75,Airfare,BOM to SFO
2,04-Sep-2024,USD - US Dollar,79.59,79.59,Business Meal,Dinner
3,03-Sep-2024,USD - US Dollar,200.00,200.00,Lodging,Hotel
";
        assert_eq!(csv, expected);
    }

    #[test]
    fn domestic_csv_excludes_foreign_typed_lines() {
        // Even if report category is somehow mis-set to ExpensesDomestic,
        // explicitly foreign lines must not appear in the domestic CSV.
        let report = report_with_category(
            Some(Category::ExpensesDomestic),
            vec![
                line_with("2024-09-02", 970.75, ExpenseType::AirfareForeign, "International"),
                line_with("2024-09-04", 79.59, ExpenseType::BusinessMeal, "Dinner"),
            ],
        );
        let csv = report_to_domestic_csv(&report);
        // Only the meal line emits, renumbered as line 1.
        assert!(!csv.contains("International"), "foreign line leaked: {csv}");
        assert!(csv.contains("1,04-Sep-2024,"), "meal line missing: {csv}");
    }

    #[test]
    fn domestic_csv_header_only_when_all_lines_are_foreign() {
        let report = report_with_category(
            Some(Category::ExpensesForeign),
            vec![
                line_with("2024-09-02", 970.75, ExpenseType::AirfareForeign, ""),
                line_with("2024-09-03", 200.00, ExpenseType::LodgingForeign, ""),
            ],
        );
        let csv = report_to_domestic_csv(&report);
        assert_eq!(
            csv,
            "Line,Expense Date,Expense Currency,Expense Amount,USD Amount,Expense Type,Remarks\n"
        );
    }

    #[test]
    fn domestic_csv_promotes_with_alcohol_meals() {
        let report = report_with_category(
            Some(Category::ExpensesDomestic),
            vec![
                with_alcohol(line_with("2024-09-02", 100.00, ExpenseType::BusinessMeal, ""), true),
                with_alcohol(line_with("2024-09-03", 200.00, ExpenseType::GroupTravelMeal, ""), true),
            ],
        );
        let csv = report_to_domestic_csv(&report);
        assert!(csv.contains("Business Meal with Alcohol"), "missing BM+alcohol: {csv}");
        assert!(csv.contains("Group Travel Meal with Alcohol"), "missing GTM+alcohol: {csv}");
    }

    #[test]
    fn domestic_csv_quotes_remarks_with_commas() {
        let report = report_with_category(
            Some(Category::ExpensesDomestic),
            vec![line_with("2024-09-02", 50.00, ExpenseType::BusinessMeal, "Lunch with A, B, C")],
        );
        let csv = report_to_domestic_csv(&report);
        assert!(csv.contains("\"Lunch with A, B, C\""), "missing comma-quoted: {csv}");
    }

    // ─── Foreign CSV ───────────────────────────────────────────────────────

    #[test]
    fn foreign_csv_emits_twenty_column_header() {
        let report = ExpenseReport::default();
        let csv = report_to_foreign_csv(&report);
        // Header is the first line; count commas + 1 to get column count.
        let header = csv.lines().next().unwrap();
        assert_eq!(header.matches(',').count() + 1, 20, "header: {header}");
        // Spot-check a few exact column names that show typos / odd formatting.
        assert!(header.contains("country of activity"), "missing G: {header}");
        assert!(header.contains("ticket_number  (not required)"),
                "ticket_number column should preserve double-space: {header}");
        assert!(header.contains("Conference Hotel"), "missing T: {header}");
    }

    #[test]
    fn foreign_csv_emits_full_row_for_airfare_line_with_suffixed_expense_type() {
        let mut line = line_with_currency(
            "2024-09-02", 970.75, Some("INR"), Some(80000.00),
            ExpenseType::AirfareForeign, "BOM to SFO",
        );
        line.common.country_of_activity = wrap("India".to_owned());
        line.common.foreign_activity_type = wrap(ForeignActivityType::Conference);
        let mut af = ExpenseReportTransactionLinesItemAirfareDetails::default();
        af.ticket_number = wrap("TKT12345".to_owned());
        af.booking_method = wrap(AirfareBookingMethod::StanfordTravelEgencia);
        af.airline = wrap("Air India".to_owned());
        af.class_of_ticket = wrap(ClassOfTicket::Coach);
        af.departure_airport = wrap("BOM".to_owned());
        af.destination_airport = wrap("SFO".to_owned());
        line.airfare_details = Some(af);

        let mut report = report_with_category(Some(Category::ExpensesForeign), vec![line]);
        report.general_information.payee.name = wrap("Jane Doe".to_owned());
        report.general_information.payee.sunet = Some("janedoe".to_owned());
        report.general_information.payee.affiliation = wrap(Affiliation::StanfordFaculty);

        let csv = report_to_foreign_csv(&report);
        let data_row = csv.lines().nth(1).expect("data row");
        // Spot-check by substring (full CSV equality is fragile vs the
        // 9208-entry airport table which Stanford may update).
        assert!(data_row.contains("Stanford Traveler,janedoe,Jane Doe,TKT12345"),
                "traveler block: {data_row}");
        // Airport codes expanded via airport_codes::airport_code_to_full;
        // SFO's display string contains a comma so it's CSV-quoted.
        assert!(data_row.contains("BOM - Chhatrapati Shivaji"),
                "BOM not expanded: {data_row}");
        assert!(data_row.contains("\"SFO - San Francisco International"),
                "SFO not expanded + quoted: {data_row}");
    }

    #[test]
    fn foreign_csv_clears_traveler_cols_on_non_airfare_rows() {
        // Stanford's filled example leaves affiliation/sunet/name BLANK
        // on lodging + meal rows. Confirm we mirror that.
        let mut lodging = line_with_currency(
            "2024-09-03", 200.0, Some("CHF"), Some(180.0),
            ExpenseType::LodgingForeign, "hotel",
        );
        let mut ld = ExpenseReportTransactionLinesItemLodgingDetails::default();
        ld.number_of_nights = wrap(2.0);
        ld.location = wrap("Zurich".to_owned());
        ld.booking_method = wrap(LodgingBookingMethod::Other);
        lodging.lodging_details = Some(ld);

        let meal = line_with("2024-09-04", 50.0, ExpenseType::BusinessMeal, "dinner");

        let mut report = report_with_category(
            Some(Category::ExpensesForeign),
            vec![lodging, meal],
        );
        report.general_information.payee.name = wrap("Jane Doe".to_owned());
        report.general_information.payee.sunet = Some("janedoe".to_owned());
        report.general_information.payee.affiliation = wrap(Affiliation::StanfordFaculty);

        let csv = report_to_foreign_csv(&report);
        for row in csv.lines().skip(1) {
            assert!(!row.contains("Stanford Traveler"),
                    "non-airfare row leaked affiliation: {row}");
            assert!(!row.contains("janedoe"),
                    "non-airfare row leaked sunet: {row}");
            assert!(!row.contains("Jane Doe"),
                    "non-airfare row leaked traveler_name: {row}");
        }
    }

    #[test]
    fn foreign_csv_falls_back_to_raw_iata_for_unknown_airport_code() {
        let mut line = line_with_currency(
            "2024-09-02", 100.0, Some("USD"), Some(100.0),
            ExpenseType::AirfareForeign, "",
        );
        let mut af = ExpenseReportTransactionLinesItemAirfareDetails::default();
        af.departure_airport = wrap("ZZZQ".to_owned());  // not in xlsx
        af.destination_airport = wrap("SFO".to_owned());
        line.airfare_details = Some(af);
        let report = report_with_category(Some(Category::ExpensesForeign), vec![line]);
        let csv = report_to_foreign_csv(&report);
        let data_row = csv.lines().nth(1).expect("data row");
        // Unknown code → raw IATA passthrough so the row still emits.
        assert!(data_row.contains(",ZZZQ,"), "unknown code raw passthrough: {data_row}");
        // SFO still expanded.
        assert!(data_row.contains("SFO - San Francisco"), "SFO not expanded: {data_row}");
    }

    #[test]
    fn foreign_csv_emits_full_row_for_lodging_line_with_conference_hotel_yes() {
        let mut line = line_with_currency(
            "2024-09-03", 200.00, Some("INR"), Some(16500.00),
            ExpenseType::LodgingForeign, "Hotel",
        );
        let mut ld = ExpenseReportTransactionLinesItemLodgingDetails::default();
        ld.number_of_nights = wrap(3.0);
        ld.location = wrap("Mumbai, India".to_owned());
        ld.booking_method = wrap(LodgingBookingMethod::ConferenceHotel);
        line.lodging_details = Some(ld);

        let report = report_with_category(Some(Category::ExpensesForeign), vec![line]);
        let csv = report_to_foreign_csv(&report);
        let data_row = csv.lines().nth(1).expect("data row");
        assert!(data_row.contains("Lodging - Foreign and Domestic"), "row: {data_row}");
        assert!(data_row.contains("3,\"Mumbai, India\",Yes"), "row: {data_row}");
    }

    #[test]
    fn foreign_csv_emits_no_for_non_conference_lodging() {
        let mut line = line_with("2024-09-03", 200.00, ExpenseType::LodgingForeign, "");
        let mut ld = ExpenseReportTransactionLinesItemLodgingDetails::default();
        ld.booking_method = wrap(LodgingBookingMethod::Other);
        line.lodging_details = Some(ld);
        let report = report_with_category(Some(Category::ExpensesForeign), vec![line]);
        let csv = report_to_foreign_csv(&report);
        let data_row = csv.lines().nth(1).expect("data row");
        // Trailing fields: nights,location,Conference Hotel — and Conference Hotel="No"
        assert!(data_row.ends_with(",,,No"), "expected ',,,No' at end, row: {data_row}");
    }

    #[test]
    fn foreign_csv_excludes_domestic_typed_lines() {
        let report = report_with_category(
            Some(Category::ExpensesForeign),
            vec![
                line_with("2024-09-02", 100.00, ExpenseType::AirfareDomestic, "domestic flight"),
                line_with("2024-09-03", 200.00, ExpenseType::LodgingForeign, "intl hotel"),
            ],
        );
        let csv = report_to_foreign_csv(&report);
        assert!(!csv.contains("domestic flight"), "domestic line leaked: {csv}");
        assert!(csv.contains("intl hotel"), "foreign line missing: {csv}");
    }

    #[test]
    fn foreign_csv_header_only_when_all_lines_are_domestic() {
        let report = report_with_category(
            Some(Category::ExpensesDomestic),
            vec![line_with("2024-09-02", 100.00, ExpenseType::BusinessMeal, "lunch")],
        );
        let csv = report_to_foreign_csv(&report);
        assert_eq!(csv.lines().count(), 1, "expected header-only: {csv}");
    }

    // ─── Routing ───────────────────────────────────────────────────────────

    #[test]
    fn routing_explicit_foreign_variants_always_go_foreign() {
        let domestic_category = Some(Category::ExpensesDomestic);
        for kind in [
            ExpenseType::AirfareForeign, ExpenseType::LodgingForeign,
            ExpenseType::GroundTransportationForeign,
            ExpenseType::GiftCardEmployeeForeign,
            ExpenseType::GiftsForeignActivity,
        ] {
            let line = line_with("2024-09-02", 100.0, kind, "");
            assert!(route_to_foreign(&line, domestic_category.as_ref()),
                    "{kind:?} should route foreign even with domestic category");
        }
    }

    #[test]
    fn routing_explicit_domestic_variants_always_go_domestic() {
        let foreign_category = Some(Category::ExpensesForeign);
        for kind in [
            ExpenseType::AirfareDomestic, ExpenseType::LodgingDomestic,
            ExpenseType::GroundTransportationDomestic,
        ] {
            let line = line_with("2024-09-02", 100.0, kind, "");
            assert!(!route_to_foreign(&line, foreign_category.as_ref()),
                    "{kind:?} should route domestic even with foreign category");
        }
    }

    #[test]
    fn routing_neutral_types_follow_report_category() {
        let neutral = ExpenseType::BusinessMeal;
        let line = line_with("2024-09-02", 100.0, neutral, "");
        assert!(route_to_foreign(&line, Some(&Category::ExpensesForeign)));
        assert!(!route_to_foreign(&line, Some(&Category::ExpensesDomestic)));
        assert!(!route_to_foreign(&line, None));
        // Other categories (Relocation, HumanSubjects) default domestic.
        assert!(!route_to_foreign(&line, Some(&Category::Relocation)));
    }

    #[test]
    fn line_counts_split_correctly() {
        let report = report_with_category(
            Some(Category::ExpensesForeign),
            vec![
                line_with("2024-09-02", 100.0, ExpenseType::AirfareDomestic, ""),
                line_with("2024-09-03", 200.0, ExpenseType::LodgingForeign, ""),
                line_with("2024-09-04", 50.0, ExpenseType::BusinessMeal, ""),  // → foreign
            ],
        );
        assert_eq!(line_counts(&report), (1, 2));
    }

    // ─── Mapper exhaustiveness ─────────────────────────────────────────────

    #[test]
    fn expense_type_domestic_mapping_covers_every_enum_variant() {
        use ExpenseType::*;
        for kind in [
            AdjustedPerDiem, AirfareDomestic, AirfareForeign, AncillaryAirlineFee,
            BusinessMeal, CarRental, ConferenceRegistration, GiftCardEmployeeForeign,
            GiftsForeignActivity, GroundTransportationDomestic, GroundTransportationForeign,
            GroupTravelMeal, HumanSubjectIncentive, LodgingDomestic, LodgingForeign,
            MembershipDues, OtherBusinessExpense,
        ] {
            let mut line = ExpenseReportTransactionLinesItem::default();
            line.common.expense_type = wrap(kind);
            let mapped = map_expense_type_domestic(&line);
            assert!(!mapped.is_empty(), "empty domestic mapping for {kind:?}");
        }
    }

    #[test]
    fn expense_type_foreign_mapping_covers_every_enum_variant() {
        use ExpenseType::*;
        for kind in [
            AdjustedPerDiem, AirfareDomestic, AirfareForeign, AncillaryAirlineFee,
            BusinessMeal, CarRental, ConferenceRegistration, GiftCardEmployeeForeign,
            GiftsForeignActivity, GroundTransportationDomestic, GroundTransportationForeign,
            GroupTravelMeal, HumanSubjectIncentive, LodgingDomestic, LodgingForeign,
            MembershipDues, OtherBusinessExpense,
        ] {
            let mut line = ExpenseReportTransactionLinesItem::default();
            line.common.expense_type = wrap(kind);
            let mapped = map_expense_type_foreign(&line);
            assert!(!mapped.is_empty(), "empty foreign mapping for {kind:?}");
        }
    }

    #[test]
    fn affiliation_foreign_mapping_covers_every_variant() {
        use Affiliation::*;
        assert_eq!(map_affiliation_foreign(StanfordStudent), "Stanford Traveler");
        assert_eq!(map_affiliation_foreign(StanfordPostdoc), "Stanford Traveler");
        assert_eq!(map_affiliation_foreign(StanfordFaculty), "Stanford Traveler");
        assert_eq!(map_affiliation_foreign(StanfordStaff), "Stanford Traveler");
        assert_eq!(map_affiliation_foreign(Other), "Visitor");
    }

    #[test]
    fn airfare_booking_method_foreign_collapses_connect_variants() {
        use AirfareBookingMethod::*;
        assert_eq!(map_airfare_booking_method_foreign(StanfordTravelEgencia),
                   "Stanford Travel - Egencia");
        assert_eq!(map_airfare_booking_method_foreign(StanfordTravelKeyTravel),
                   "Stanford Travel - Key Travel");
        for v in [StanfordTravelConnectUa, StanfordTravelConnectDl, StanfordTravelConnectAa,
                  StanfordTravelConnectAs, StanfordTravelConnectHa] {
            assert_eq!(map_airfare_booking_method_foreign(v),
                       "Stanford Travel Connect (UA, DL, AA, AS and HA)");
        }
        assert_eq!(map_airfare_booking_method_foreign(Other),
                   "Other (Not a Stanford Travel Booking Method)");
    }

    #[test]
    fn class_of_ticket_foreign_collapses_premium_into_business() {
        use ClassOfTicket::*;
        assert_eq!(map_class_of_ticket_foreign(Coach), "Coach");
        assert_eq!(map_class_of_ticket_foreign(First), "First");
        assert_eq!(map_class_of_ticket_foreign(PremiumEconomy), "Premium Economy/Business");
        assert_eq!(map_class_of_ticket_foreign(Business), "Premium Economy/Business");
    }

    #[test]
    fn foreign_activity_type_mapping_covers_every_variant() {
        use ForeignActivityType::*;
        assert_eq!(map_foreign_activity_type(Conference), "Conferences");
        assert_eq!(map_foreign_activity_type(ResearchCollaboration), "Collaborations/Meetings");
        assert_eq!(map_foreign_activity_type(Fieldwork), "Research: Field work");
        assert_eq!(map_foreign_activity_type(Other), "Other");
    }

    // ─── Date + currency ───────────────────────────────────────────────────

    #[test]
    fn portal_date_format_round_trip() {
        assert_eq!(format_portal_date("2024-09-02"), "02-Sep-2024");
        assert_eq!(format_portal_date("2026-04-23"), "23-Apr-2026");
        assert_eq!(format_portal_date("2026-01-01"), "01-Jan-2026");
        assert_eq!(format_portal_date("2026-12-31"), "31-Dec-2026");
    }

    #[test]
    fn portal_date_format_passes_through_malformed_input() {
        assert_eq!(format_portal_date(""), "");
        assert_eq!(format_portal_date("April 23 2026"), "April 23 2026");
        assert_eq!(format_portal_date("2026-13-01"), "2026-13-01");
        assert_eq!(format_portal_date("2026/04/23"), "2026/04/23");
    }

    #[test]
    fn currency_code_matches_xlsx_strings_exactly() {
        // Spot-check the typos / odd capitalization Stanford has.
        assert_eq!(currency_code_to_full("USD"), "USD - US Dollar");
        assert_eq!(currency_code_to_full("BRL"), "BRL - Brazilian Real");
        assert_eq!(currency_code_to_full("ARS"), "ARS - Argentine peso");  // lowercase!
        assert_eq!(currency_code_to_full("KRW"), "KRW - South Korean won");  // lowercase!
        assert_eq!(currency_code_to_full("AOA"), "AOA - Angolanische Kwanza");  // German!
        assert_eq!(currency_code_to_full("PEN"), "PEN - Sol");  // just "Sol"
        assert_eq!(currency_code_to_full("AED"), "AED - United Arab Emirates Dirham");
    }

    #[test]
    fn currency_code_unknown_falls_back_to_code_dash_code() {
        assert_eq!(currency_code_to_full("XYZ"), "XYZ - XYZ");
        assert_eq!(currency_code_to_full(""), " - ");
    }

    // ─── Business purpose ──────────────────────────────────────────────────

    #[test]
    fn business_purpose_text_concatenates_with_labels() {
        let mut bp = ExpenseReportGeneralInformationBusinessPurpose::default();
        bp.who = wrap("Jane Doe".to_owned());
        bp.what = wrap("Presented research".to_owned());
        bp.when = wrap("2024-09-01 to 2024-09-05".to_owned());
        bp.r#where = wrap("Buenos Aires".to_owned());
        bp.why = wrap("Disseminate".to_owned());

        let text = business_purpose_to_text(&bp);
        let expected = "\
Who: Jane Doe
What: Presented research
When: 2024-09-01 to 2024-09-05
Where: Buenos Aires
Why: Disseminate
";
        assert_eq!(text, expected);
    }

    #[test]
    fn business_purpose_text_skips_missing_fields() {
        let mut bp = ExpenseReportGeneralInformationBusinessPurpose::default();
        bp.who = wrap("Jane".to_owned());
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
