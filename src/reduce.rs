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
    ExpenseReportTransactionLinesItemCommonSourceDocumentsItem,
    ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum, IsoDate,
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

/// Confidence in an "any non-USD → foreign" derivation: the floor of the
/// contributing receipts' `printed_currency` confidences. If every receipt
/// reported `printed_currency` with `high` confidence, the derived
/// category is `high` too. If some are missing or `low`, the result is
/// `low`. The derivation rule itself is unambiguous, so the only loss
/// of confidence comes from the inputs.
fn category_confidence(receipts: &[ExtractedReceipt]) -> ConfidenceLevel {
    confidence_floor(receipts.iter().map(|r| r.extras.printed_currency.meta.confidence))
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
    }
}

/// Build the `transaction_lines` array. For each receipt: take the
/// schema-shaped line, attach a single `source_documents` entry naming the
/// FA-uploaded file. The line is otherwise passed through unchanged —
/// per-document fields are already populated by the extractor.
pub fn reduce_transaction_lines(
    receipts: &[ExtractedReceipt],
) -> Vec<ExpenseReportTransactionLinesItem> {
    receipts
        .iter()
        .map(|r| {
            let mut line = r.line.clone();
            line.common.source_documents = vec![
                ExpenseReportTransactionLinesItemCommonSourceDocumentsItem {
                    filename: Some(r.source_filename.clone()),
                    document_type: Some(
                        ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum::Receipt,
                    ),
                },
            ];
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
        meta: derived_meta(category_confidence(receipts), "reduce.inferred_category"),
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
            expense_kind: "meal".to_owned(),
            line,
            extras: Extras {
                merchant_address: Wrapped::unknown(),
                printed_currency,
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

    #[test]
    fn transaction_lines_attaches_source_document_with_filename() {
        let receipts = vec![make_receipt("mjsushi.jpeg", "2026-05-02", 79.59, Some("USD"))];
        let lines = reduce_transaction_lines(&receipts);
        assert_eq!(lines.len(), 1);
        let sources = &lines[0].common.source_documents;
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].filename.as_deref(), Some("mjsushi.jpeg"));
        assert_eq!(
            sources[0].document_type,
            Some(
                ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum::Receipt
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

        // 4 lines, each with its own source_documents entry.
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
