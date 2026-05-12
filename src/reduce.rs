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

/// Build the `transaction_lines` array. For each receipt: take the
/// schema-shaped line, attach the single `source_document` naming the
/// FA-uploaded file. The line is otherwise passed through unchanged —
/// per-document fields are already populated by the extractor.
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
            line
        })
        .collect()
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
                nightly_rates: Wrapped::unknown(),
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
}
