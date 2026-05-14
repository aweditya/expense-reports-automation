//! Cross-document reduction: list of per-receipt extractions → one
//! schema-typed `ExpenseReport`.
//!
//! Each `reduce_*` function is named, single-purpose, and unit-tested in
//! isolation. The top-level `reduce_to_expense_report` calls them in order.
//! Pure functions throughout — I/O lives in the M6.2.f binary.
//!
//! Architecture B: per-receipt JSON has both schema-shaped fields (which
//! flow into the report unchanged) and `extras` (which feed derivations).
//! `category` and friends are derived here; trip-window inference and
//! per-diem expansion are deferred until we have a corpus that exercises
//! them.

use crate::expense_report_model::{
    ExpenseReport, ExpenseReportGeneralInformationCategoryEnum, ExpenseReportTransactionLinesItem,
    ExpenseReportTransactionLinesItemCommonSourceDocument,
    ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum, IsoDate,
};
use crate::extracted_receipt::ExtractedReceipt;
use crate::meta::{ConfidenceLevel, EvidenceKind, EvidenceReference, FieldMetadata, Wrapped};

/// Sum of `common.line_amount_usd.value` across receipts. None values are
/// treated as 0 (validator catches "missing required line amount" later).
pub fn reduce_total_usd(receipts: &[ExtractedReceipt]) -> f64 {
    receipts
        .iter()
        .filter_map(|r| r.line.common.line_amount_usd.value)
        .sum()
}

/// Earliest `common.date.value` across receipts. None if no receipt has a
/// date set.
pub fn reduce_earliest_date(receipts: &[ExtractedReceipt]) -> Option<IsoDate> {
    receipts
        .iter()
        .filter_map(|r| r.line.common.date.value.clone())
        .min()
}

/// Returns `ExpensesForeign` if any receipt has a printed_currency that
/// isn't USD. Null printed_currency is treated as USD-by-default. This
/// matches the locked decision in docs/redesign-plan.md ("null → USD").
pub fn reduce_inferred_category(
    receipts: &[ExtractedReceipt],
) -> ExpenseReportGeneralInformationCategoryEnum {
    let any_foreign = receipts.iter().any(|r| {
        r.extras
            .printed_currency
            .value
            .as_deref()
            .is_some_and(|cur| !cur.eq_ignore_ascii_case("USD"))
    });
    if any_foreign {
        ExpenseReportGeneralInformationCategoryEnum::ExpensesForeign
    } else {
        ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic
    }
}

/// "Floor" of two confidence levels — the more cautious of the two.
/// Low < Medium < High.
fn min_confidence(a: ConfidenceLevel, b: ConfidenceLevel) -> ConfidenceLevel {
    use ConfidenceLevel::*;
    match (a, b) {
        (Low, _) | (_, Low) => Low,
        (Medium, _) | (_, Medium) => Medium,
        _ => High,
    }
}

fn confidence_floor<I: Iterator<Item = ConfidenceLevel>>(mut levels: I) -> ConfidenceLevel {
    let mut acc = match levels.next() {
        Some(c) => c,
        None => return ConfidenceLevel::Low,
    };
    for c in levels {
        acc = min_confidence(acc, c);
    }
    acc
}

/// Confidence in an "any non-USD → foreign" derivation.
///
/// Currency-floor by default: floor of every receipt's `printed_currency`
/// confidence. The prompt asks Gemini to use `medium` when it has to
/// infer USD from a bare `$` symbol (which is also CAD/AUD/NZD/HKD/MXN/…)
/// — so a US receipt that just prints `$` truthfully comes back medium,
/// and the floor is medium even when the report is obviously domestic.
///
/// Country lift: if every receipt has `country_of_activity` filled with
/// `high` confidence and the values are consistent with the inferred
/// category (all `United States` for domestic; all non-US for foreign),
/// the address signal overrides the currency-symbol ambiguity and we
/// lift to `high`. A `$` on a CAD receipt with country=Canada still
/// flags via the consistency check.
fn category_confidence(
    receipts: &[ExtractedReceipt],
    inferred: ExpenseReportGeneralInformationCategoryEnum,
) -> ConfidenceLevel {
    let currency_floor = confidence_floor(
        receipts.iter().map(|r| r.extras.printed_currency.meta.confidence),
    );

    let country_signal_ok = !receipts.is_empty()
        && receipts.iter().all(|r| {
            let c = &r.line.common.country_of_activity;
            if c.meta.confidence != ConfidenceLevel::High {
                return false;
            }
            let Some(country) = c.value.as_deref() else { return false; };
            let is_us = country.eq_ignore_ascii_case("United States");
            match inferred {
                ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic => is_us,
                ExpenseReportGeneralInformationCategoryEnum::ExpensesForeign => !is_us,
                _ => false,
            }
        });

    if country_signal_ok {
        ConfidenceLevel::High
    } else {
        currency_floor
    }
}

/// Construct a `FieldMetadata` for a value derived in the reduction step.
/// The evidence is a single `system_generated` entry naming the reduction
/// origin so the workbench can show where the value came from.
fn derived_meta(confidence: ConfidenceLevel, origin: &str) -> FieldMetadata {
    FieldMetadata {
        confidence,
        evidence: vec![EvidenceReference {
            kind: EvidenceKind::SystemGenerated,
            document_id: None,
            filename: None,
            page: None,
            quote: None,
            origin: Some(origin.to_owned()),
        }],
        needs_review: false,
        flags: Vec::new(),
        // Reduction-derived fields don't need a confidence_reason —
        // the existing system_generated origin already explains the
        // derivation; the workbench shows it via human_readable_origin.
        confidence_reason: None,
    }
}

/// Mock USD-per-unit rates for foreign currencies, accurate to ~2024-2025
/// averages. Used by `apply_mock_fx` when the extractor leaves
/// `common.line_amount_usd` null on a foreign-currency receipt
/// (origin: `needs_fx_conversion`). Returns `None` for currencies we
/// don't have a rate for; reduction leaves `line_amount_usd` unset in
/// that case so the validator surfaces it.
///
/// MOCK / TODO: this is a placeholder. Real-time integration is a
/// follow-up — either a free FX API (exchangerate-api.com, frankfurter.app)
/// or a Gemini grounded-search call. The workbench surfaces the
/// `reduce.fx.mock` origin so any FA reviewing a converted amount
/// knows it came from this mock, not a real-time rate.
fn mock_usd_rate(currency: &str) -> Option<f64> {
    match currency {
        "INR" => Some(0.012),  // ~83 INR/USD
        "EUR" => Some(1.07),   // ~0.93 EUR/USD
        "GBP" => Some(1.27),   // ~0.79 GBP/USD
        "JPY" => Some(0.0067), // ~150 JPY/USD
        _ => None,
    }
}

/// Fill `common.line_amount_usd` from `original_amount` × mock FX rate
/// when the extractor left it null on a foreign-currency receipt. No-op
/// when `line_amount_usd` is already set, when either of the source
/// fields is missing, or when the currency isn't in `mock_usd_rate`'s
/// table. Marks the derived value `needs_review: true` so the FA knows
/// to verify it before submission (the rate is a mock, not real-time).
fn apply_mock_fx(line: &mut ExpenseReportTransactionLinesItem) {
    if line.common.line_amount_usd.value.is_some() {
        return;
    }
    let amount = match line.common.original_amount.value {
        Some(a) => a,
        None => return,
    };
    let currency = match line.common.original_currency.value.as_deref() {
        Some(c) => c,
        None => return,
    };
    let rate = match mock_usd_rate(currency) {
        Some(r) => r,
        None => return,
    };
    let mut meta = derived_meta(ConfidenceLevel::Medium, "reduce.fx.mock");
    // Mock rate; the FA should confirm before submission. (Other
    // reduction-derived fields are deterministic from extractor input —
    // this one depends on a placeholder rate, which is the difference.)
    meta.needs_review = true;
    line.common.line_amount_usd = Wrapped {
        value: Some(amount * rate),
        meta,
    };
    // Also surface the rate itself on the line so the FA can audit
    // exactly what conversion factor was used. Same medium confidence +
    // needs-review treatment — both fields are downstream of the same
    // mock and share its caveats.
    let mut rate_meta = derived_meta(ConfidenceLevel::Medium, "reduce.fx.mock");
    rate_meta.needs_review = true;
    line.common.exchange_rate = Wrapped {
        value: Some(rate),
        meta: rate_meta,
    };
}

/// Build the `transaction_lines` array. For each receipt: take the
/// schema-shaped line, attach the single `source_document` naming the
/// FA-uploaded file, fill `line_amount_usd` from a mock FX rate when
/// the extractor deferred (foreign-currency receipts; airfare today),
/// and run per-kind derivations (currently only lodging — average
/// nightly_rates into daily_rate and compute number_of_nights from
/// check-in/check-out).
pub fn reduce_transaction_lines(
    receipts: &[ExtractedReceipt],
) -> Vec<ExpenseReportTransactionLinesItem> {
    receipts
        .iter()
        .map(|r| {
            let mut line = r.line.clone();
            line.common.source_document = ExpenseReportTransactionLinesItemCommonSourceDocument {
                filename: Some(r.source_filename.clone()),
                document_type: Some(
                    ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum::Receipt,
                ),
            };
            // Generic across kinds: if extractor deferred FX (foreign
            // ticket etc.), fill line_amount_usd from a mock rate. No-op
            // when line_amount_usd is already set.
            apply_mock_fx(&mut line);
            // Lodging-specific: average per-night rates → daily_rate,
            // compute nights from check-in/check-out. Both are T2 fields
            // the extractor doesn't fill.
            if line.lodging_details.is_some() {
                derive_lodging_fields(&mut line, &r.extras);
            }
            line
        })
        .collect()
}

/// Populate the T2 fields of `lodging_details` from the per-document
/// `extras.nightly_rates` array and `check_in_date` / `check_out_date`.
/// Confidence inheritance: daily_rate inherits from the nightly_rates
/// array's confidence; number_of_nights inherits the floor of the two
/// date fields' confidences.
fn derive_lodging_fields(
    line: &mut ExpenseReportTransactionLinesItem,
    extras: &crate::extracted_receipt::Extras,
) {
    let lodging = line
        .lodging_details
        .as_mut()
        .expect("called on a line with lodging_details");

    // daily_rate = mean of nightly_rates[].rate. Confidence is `high`
    // when the breakdown is present (the leaf-wrapper that previously
    // carried per-array confidence is gone — Vertex rejects leaf-of-
    // array). When the array is empty (model didn't recover the
    // breakdown), daily_rate stays Wrapped::unknown.
    if !extras.nightly_rates.is_empty() {
        let sum: f64 = extras.nightly_rates.iter().map(|n| n.rate).sum();
        let avg = sum / extras.nightly_rates.len() as f64;
        lodging.daily_rate = Wrapped {
            value: Some(avg),
            meta: derived_meta(ConfidenceLevel::High, "reduce.lodging.daily_rate"),
        };
    }

    // number_of_nights = check_out - check_in (in days)
    if let (Some(checkin), Some(checkout)) = (
        lodging.check_in_date.value.as_ref(),
        lodging.check_out_date.value.as_ref(),
    ) {
        if let Some(nights) = nights_between(&checkin.0, &checkout.0) {
            let conf = min_confidence(
                lodging.check_in_date.meta.confidence,
                lodging.check_out_date.meta.confidence,
            );
            lodging.number_of_nights = Wrapped {
                value: Some(nights as f64),
                meta: derived_meta(conf, "reduce.lodging.number_of_nights"),
            };
        }
    }
}

/// Compute the number of nights between two ISO 8601 date strings
/// (YYYY-MM-DD). Returns None if parsing fails or check_out is before
/// check_in.
fn nights_between(check_in: &str, check_out: &str) -> Option<u32> {
    let parse = |s: &str| -> Option<(i32, u32, u32)> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    };
    let to_julian = |(y, m, d): (i32, u32, u32)| -> i64 {
        // Plain proleptic Gregorian day-number (Fairfield's algorithm).
        // Good enough for date subtraction on realistic check-in dates.
        let a = (14 - m as i64) / 12;
        let y = y as i64 + 4800 - a;
        let m = m as i64 + 12 * a - 3;
        d as i64 + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045
    };
    let in_d = to_julian(parse(check_in)?);
    let out_d = to_julian(parse(check_out)?);
    if out_d < in_d {
        return None;
    }
    Some((out_d - in_d) as u32)
}

/// One-shot entry point: assemble a complete `ExpenseReport` from a
/// list of per-receipt extractions. Fields the reduction can derive are
/// populated; everything else stays at its `Default::default()` (T1
/// fields the FA fills later, or T2 fields a future stage computes).
pub fn reduce_to_expense_report(receipts: &[ExtractedReceipt]) -> ExpenseReport {
    let mut report = ExpenseReport::default();

    let lines = reduce_transaction_lines(receipts);
    let total = reduce_total_usd(receipts);
    let earliest = reduce_earliest_date(receipts);
    let category = reduce_inferred_category(receipts);

    // Derive confidence floors from the contributing receipts' dates
    // and amounts — same principle as category_confidence.
    let date_confidence = confidence_floor(
        receipts.iter().map(|r| r.line.common.date.meta.confidence)
    );
    let amount_confidence = confidence_floor(
        receipts.iter().map(|r| r.line.common.line_amount_usd.meta.confidence)
    );

    report.transaction_lines = Some(lines);
    report.transaction_summary.total_usd = Wrapped {
        value: Some(total),
        meta: derived_meta(amount_confidence, "reduce.total_usd"),
    };
    report.transaction_summary.transaction_date = match earliest {
        Some(date) => Wrapped {
            value: Some(date),
            meta: derived_meta(date_confidence, "reduce.earliest_date"),
        },
        None => Wrapped::unknown(),
    };
    report.general_information.category = Wrapped {
        value: Some(category),
        meta: derived_meta(category_confidence(receipts, category), "reduce.inferred_category"),
    };

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expense_report_model::ExpenseReportTransactionLinesItem;
    use crate::extracted_receipt::Extras;
    use crate::meta::FieldMetadata;

    fn make_receipt(filename: &str, date: &str, amount: f64, currency: Option<&str>) -> ExtractedReceipt {
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.date = Wrapped {
            value: Some(IsoDate(date.to_owned())),
            meta: FieldMetadata::default(),
        };
        line.common.line_amount_usd = Wrapped {
            value: Some(amount),
            meta: FieldMetadata::default(),
        };

        let printed_currency = match currency {
            Some(c) => Wrapped {
                value: Some(c.to_owned()),
                meta: FieldMetadata::default(),
            },
            None => Wrapped::unknown(),
        };

        ExtractedReceipt {
            source_filename: filename.to_owned(),
            line,
            extras: Extras {
                merchant_address: Wrapped::unknown(),
                printed_currency,
                nightly_rates: Vec::new(),
                segments: Vec::new(),
            },
        }
    }

    #[test]
    fn total_usd_sums_line_amounts() {
        let receipts = vec![
            make_receipt("a.jpeg", "2026-04-19", 163.54, Some("USD")),
            make_receipt("b.jpeg", "2026-04-04", 123.19, Some("USD")),
            make_receipt("c.png", "2026-03-05", 387.12, Some("USD")),
        ];
        assert!((reduce_total_usd(&receipts) - 673.85).abs() < 1e-9);
    }

    #[test]
    fn earliest_date_picks_minimum() {
        let receipts = vec![
            make_receipt("a.jpeg", "2026-04-19", 1.0, None),
            make_receipt("b.jpeg", "2026-04-04", 1.0, None),
            make_receipt("c.png", "2026-03-05", 1.0, None),
        ];
        assert_eq!(
            reduce_earliest_date(&receipts).map(|d| d.0),
            Some("2026-03-05".to_owned())
        );
    }

    #[test]
    fn earliest_date_none_when_no_dates() {
        let mut receipts = vec![make_receipt("a.jpeg", "2026-04-19", 1.0, None)];
        receipts[0].line.common.date = Wrapped::unknown();
        assert!(reduce_earliest_date(&receipts).is_none());
    }

    #[test]
    fn category_domestic_when_all_usd() {
        let receipts = vec![
            make_receipt("a.jpeg", "2026-04-19", 1.0, Some("USD")),
            make_receipt("b.jpeg", "2026-04-04", 1.0, Some("USD")),
        ];
        assert_eq!(
            reduce_inferred_category(&receipts),
            ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic
        );
    }

    #[test]
    fn category_foreign_when_any_non_usd() {
        let receipts = vec![
            make_receipt("a.jpeg", "2026-04-19", 1.0, Some("USD")),
            make_receipt("b.jpeg", "2026-04-04", 1.0, Some("SGD")),
        ];
        assert_eq!(
            reduce_inferred_category(&receipts),
            ExpenseReportGeneralInformationCategoryEnum::ExpensesForeign
        );
    }

    #[test]
    fn category_treats_null_currency_as_usd() {
        // The locked rule: null printed_currency → USD-by-default.
        let receipts = vec![
            make_receipt("a.jpeg", "2026-04-19", 1.0, None),
            make_receipt("b.jpeg", "2026-04-04", 1.0, Some("USD")),
        ];
        assert_eq!(
            reduce_inferred_category(&receipts),
            ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic
        );
    }

    fn with_currency_confidence(
        mut r: ExtractedReceipt,
        currency_confidence: ConfidenceLevel,
    ) -> ExtractedReceipt {
        r.extras.printed_currency.meta.confidence = currency_confidence;
        r
    }

    fn with_country(
        mut r: ExtractedReceipt,
        country: &str,
        country_confidence: ConfidenceLevel,
    ) -> ExtractedReceipt {
        r.line.common.country_of_activity = Wrapped {
            value: Some(country.to_owned()),
            meta: FieldMetadata {
                confidence: country_confidence,
                ..FieldMetadata::default()
            },
        };
        r
    }

    #[test]
    fn category_confidence_lifts_to_high_when_all_us_addresses_high() {
        // Live-prod scenario: every receipt prints just "$" (Gemini → medium
        // on printed_currency), but every merchant address is in the US
        // (country_of_activity = "United States" with high). The address
        // signal should override the symbol-vs-string ambiguity.
        let receipts = vec![
            with_country(
                with_currency_confidence(
                    make_receipt("a.jpeg", "2026-04-19", 1.0, Some("USD")),
                    ConfidenceLevel::Medium,
                ),
                "United States",
                ConfidenceLevel::High,
            ),
            with_country(
                with_currency_confidence(
                    make_receipt("b.jpeg", "2026-04-04", 1.0, Some("USD")),
                    ConfidenceLevel::Medium,
                ),
                "United States",
                ConfidenceLevel::High,
            ),
        ];
        let inferred = reduce_inferred_category(&receipts);
        assert_eq!(category_confidence(&receipts, inferred), ConfidenceLevel::High);
    }

    #[test]
    fn category_confidence_falls_back_to_currency_floor_when_country_inconsistent() {
        // Mismatched signal — derived category=domestic (USD currency) but
        // one country is non-US. The address signal can't lift; we fall
        // back to the currency floor (medium).
        let receipts = vec![
            with_country(
                with_currency_confidence(
                    make_receipt("a.jpeg", "2026-04-19", 1.0, Some("USD")),
                    ConfidenceLevel::Medium,
                ),
                "United States",
                ConfidenceLevel::High,
            ),
            with_country(
                with_currency_confidence(
                    make_receipt("b.jpeg", "2026-04-04", 1.0, Some("USD")),
                    ConfidenceLevel::Medium,
                ),
                "Canada",
                ConfidenceLevel::High,
            ),
        ];
        let inferred = reduce_inferred_category(&receipts);
        assert_eq!(category_confidence(&receipts, inferred), ConfidenceLevel::Medium);
    }

    #[test]
    fn category_confidence_uses_currency_floor_when_country_missing() {
        // No country_of_activity filled → no lift, currency floor wins.
        let receipts = vec![
            with_currency_confidence(
                make_receipt("a.jpeg", "2026-04-19", 1.0, Some("USD")),
                ConfidenceLevel::High,
            ),
            with_currency_confidence(
                make_receipt("b.jpeg", "2026-04-04", 1.0, Some("USD")),
                ConfidenceLevel::Medium,
            ),
        ];
        let inferred = reduce_inferred_category(&receipts);
        assert_eq!(category_confidence(&receipts, inferred), ConfidenceLevel::Medium);
    }

    #[test]
    fn transaction_lines_attaches_source_document_with_filename() {
        let receipts = vec![make_receipt("mjsushi.jpeg", "2026-05-02", 79.59, Some("USD"))];
        let lines = reduce_transaction_lines(&receipts);
        assert_eq!(lines.len(), 1);
        let source = &lines[0].common.source_document;
        assert_eq!(source.filename.as_deref(), Some("mjsushi.jpeg"));
        assert_eq!(
            source.document_type,
            Some(
                ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum::Receipt
            )
        );
    }

    #[test]
    fn reduce_to_expense_report_assembles_all_fields() {
        let receipts = vec![
            make_receipt("a.jpeg", "2026-04-19", 163.54, Some("USD")),
            make_receipt("b.jpeg", "2026-04-04", 123.19, Some("USD")),
            make_receipt("c.png", "2026-03-05", 387.12, Some("USD")),
            make_receipt("d.jpeg", "2026-05-02", 79.59, Some("USD")),
        ];

        let report = reduce_to_expense_report(&receipts);

        // 4 lines, each with its own source_document.
        let lines = report.transaction_lines.expect("transaction_lines populated");
        assert_eq!(lines.len(), 4);

        // Total = sum of all four amounts.
        assert!(
            (report.transaction_summary.total_usd.value.unwrap() - (163.54 + 123.19 + 387.12 + 79.59))
                .abs()
                < 1e-9
        );

        // Earliest date is "2026-03-05".
        assert_eq!(
            report
                .transaction_summary
                .transaction_date
                .value
                .as_ref()
                .map(|d| d.0.as_str()),
            Some("2026-03-05")
        );

        // All USD → domestic.
        assert_eq!(
            report.general_information.category.value,
            Some(ExpenseReportGeneralInformationCategoryEnum::ExpensesDomestic)
        );
    }

    /// Helper: build a lodging receipt with given nightly_rates +
    /// check-in/check-out. Common date/amount left at defaults; only
    /// the lodging-specific bits matter for these tests.
    fn make_lodging_receipt(
        filename: &str,
        check_in: &str,
        check_out: &str,
        rates: &[f64],
    ) -> ExtractedReceipt {
        use crate::expense_report_model::ExpenseReportTransactionLinesItemLodgingDetails;
        use crate::extracted_receipt::NightlyRate;

        let mut line = ExpenseReportTransactionLinesItem::default();
        let mut lodging = ExpenseReportTransactionLinesItemLodgingDetails::default();
        lodging.check_in_date = Wrapped {
            value: Some(IsoDate(check_in.to_owned())),
            meta: FieldMetadata::default(),
        };
        lodging.check_out_date = Wrapped {
            value: Some(IsoDate(check_out.to_owned())),
            meta: FieldMetadata::default(),
        };
        line.lodging_details = Some(lodging);

        let nights: Vec<NightlyRate> = rates
            .iter()
            .enumerate()
            .map(|(i, rate)| NightlyRate {
                date: format!("2026-04-{:02}", 19 + i),
                rate: *rate,
                taxes_and_fees: 0.0,
            })
            .collect();

        ExtractedReceipt {
            source_filename: filename.to_owned(),
            line,
            extras: Extras {
                merchant_address: Wrapped::unknown(),
                printed_currency: Wrapped::unknown(),
                nightly_rates: nights,
                segments: Vec::new(),
            },
        }
    }

    #[test]
    fn lodging_daily_rate_is_average_of_nightly_rates() {
        // 3 nights at varying rates: $189, $189, $250 → mean ≈ 209.33.
        let receipts = vec![make_lodging_receipt(
            "hotel.pdf",
            "2026-04-19",
            "2026-04-22",
            &[189.0, 189.0, 250.0],
        )];
        let lines = reduce_transaction_lines(&receipts);
        let lodging = lines[0].lodging_details.as_ref().expect("lodging present");
        let daily_rate = lodging.daily_rate.value.expect("daily_rate populated");
        assert!((daily_rate - 209.3333).abs() < 0.001, "got {daily_rate}");
        // Non-empty nightly_rates → high-confidence daily_rate.
        assert_eq!(lodging.daily_rate.meta.confidence, ConfidenceLevel::High);
    }

    #[test]
    fn lodging_daily_rate_handles_flat_rate_trivially() {
        // 6 flat-rate nights (the Sheraton case): mean == the rate.
        let receipts = vec![make_lodging_receipt(
            "sheraton.pdf",
            "2024-01-14",
            "2024-01-20",
            &[134.0; 6],
        )];
        let lines = reduce_transaction_lines(&receipts);
        let lodging = lines[0].lodging_details.as_ref().unwrap();
        assert_eq!(lodging.daily_rate.value, Some(134.0));
    }

    #[test]
    fn lodging_number_of_nights_from_check_in_check_out() {
        let receipts = vec![make_lodging_receipt(
            "hotel.pdf",
            "2024-01-13",
            "2024-01-14",
            &[177.0],
        )];
        let lines = reduce_transaction_lines(&receipts);
        let lodging = lines[0].lodging_details.as_ref().unwrap();
        assert_eq!(lodging.number_of_nights.value, Some(1.0));

        // 6-night stay (Jan 14 → Jan 20).
        let receipts = vec![make_lodging_receipt(
            "sheraton.pdf",
            "2024-01-14",
            "2024-01-20",
            &[134.0; 6],
        )];
        let lines = reduce_transaction_lines(&receipts);
        let lodging = lines[0].lodging_details.as_ref().unwrap();
        assert_eq!(lodging.number_of_nights.value, Some(6.0));
    }

    #[test]
    fn lodging_skips_derivation_on_non_lodging_lines() {
        // Meal receipt: derive_lodging_fields should NOT run. The
        // make_receipt helper produces meal lines (or rather, lines
        // without lodging_details).
        let receipts = vec![make_receipt("a.jpeg", "2026-04-19", 50.0, Some("USD"))];
        let lines = reduce_transaction_lines(&receipts);
        assert!(lines[0].lodging_details.is_none());
    }

    #[test]
    fn lodging_daily_rate_unknown_when_breakdown_empty() {
        // No nightly_rates emitted (model couldn't recover the breakdown):
        // daily_rate stays unknown, no derivation runs.
        let receipts = vec![make_lodging_receipt(
            "hotel.pdf",
            "2026-04-19",
            "2026-04-22",
            &[],
        )];
        let lines = reduce_transaction_lines(&receipts);
        let lodging = lines[0].lodging_details.as_ref().unwrap();
        assert_eq!(lodging.daily_rate.value, None);
    }

    #[test]
    fn nights_between_handles_invalid_input() {
        assert_eq!(nights_between("2024-01-13", "2024-01-14"), Some(1));
        assert_eq!(nights_between("2024-01-14", "2024-01-20"), Some(6));
        // Cross-year.
        assert_eq!(nights_between("2023-12-30", "2024-01-02"), Some(3));
        // Check-out before check-in.
        assert_eq!(nights_between("2024-01-20", "2024-01-14"), None);
        // Malformed.
        assert_eq!(nights_between("not-a-date", "2024-01-14"), None);
    }

    #[test]
    fn mock_usd_rate_returns_known_currencies() {
        assert_eq!(mock_usd_rate("INR"), Some(0.012));
        assert_eq!(mock_usd_rate("EUR"), Some(1.07));
        assert_eq!(mock_usd_rate("GBP"), Some(1.27));
        assert_eq!(mock_usd_rate("JPY"), Some(0.0067));
        // USD doesn't need conversion; not in the table.
        assert_eq!(mock_usd_rate("USD"), None);
        // Unknown currency: caller leaves line_amount_usd unset.
        assert_eq!(mock_usd_rate("XYZ"), None);
    }

    #[test]
    fn apply_mock_fx_fills_line_amount_usd_for_foreign_currency() {
        // Air India BOM→SFO ticket: extractor leaves line_amount_usd
        // null with origin "needs_fx_conversion"; reduction fills it.
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.original_amount = Wrapped {
            value: Some(80896.0),
            meta: FieldMetadata::default(),
        };
        line.common.original_currency = Wrapped {
            value: Some("INR".to_owned()),
            meta: FieldMetadata::default(),
        };
        // line_amount_usd starts as Wrapped::unknown() (default).
        assert!(line.common.line_amount_usd.value.is_none());

        apply_mock_fx(&mut line);

        let usd = line.common.line_amount_usd.value.expect("filled");
        // 80896 INR × 0.012 = 970.752 USD.
        assert!((usd - 970.752).abs() < 0.001, "got {}", usd);
        assert_eq!(line.common.line_amount_usd.meta.confidence, ConfidenceLevel::Medium);
        assert!(line.common.line_amount_usd.meta.needs_review);
        assert_eq!(
            line.common.line_amount_usd.meta.evidence[0].origin.as_deref(),
            Some("reduce.fx.mock"),
        );
        // exchange_rate filled with the same rate, so the FA can audit
        // the conversion. Same medium confidence + needs-review.
        let rate = line.common.exchange_rate.value.expect("rate filled");
        assert!((rate - 0.012).abs() < 1e-9, "got rate {}", rate);
        assert_eq!(line.common.exchange_rate.meta.confidence, ConfidenceLevel::Medium);
        assert!(line.common.exchange_rate.meta.needs_review);
        assert_eq!(
            line.common.exchange_rate.meta.evidence[0].origin.as_deref(),
            Some("reduce.fx.mock"),
        );
    }

    #[test]
    fn apply_mock_fx_leaves_line_amount_usd_alone_when_already_set() {
        // Domestic USD ticket: extractor already filled line_amount_usd.
        // FX mock is a no-op; the existing high-confidence value stays.
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.line_amount_usd = Wrapped {
            value: Some(519.97),
            meta: FieldMetadata {
                confidence: ConfidenceLevel::High,
                ..Default::default()
            },
        };

        apply_mock_fx(&mut line);

        assert_eq!(line.common.line_amount_usd.value, Some(519.97));
        assert_eq!(line.common.line_amount_usd.meta.confidence, ConfidenceLevel::High);
    }

    #[test]
    fn apply_mock_fx_leaves_unknown_when_currency_unrecognized() {
        // Foreign ticket in a currency we don't have a mock rate for:
        // line_amount_usd stays unset; the validator surfaces it
        // downstream as "missing required field" and the FA fills it.
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.original_amount = Wrapped {
            value: Some(1000.0),
            meta: FieldMetadata::default(),
        };
        line.common.original_currency = Wrapped {
            value: Some("XYZ".to_owned()),
            meta: FieldMetadata::default(),
        };

        apply_mock_fx(&mut line);

        assert!(line.common.line_amount_usd.value.is_none());
    }
}
