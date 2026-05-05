//! M7.b — minimal workbench HTML renderer for the new pipeline.
//!
//! Walks a typed `ExpenseReport` (post-reduction) plus a list of validation
//! issues, emits a single self-contained HTML page. Inline CSS, no
//! JavaScript. Read-only for now; editable inputs come post-M7.
//!
//! Visual vocabulary inherited from the old workbench: shell / hero /
//! eyebrow / panel / card. The structure is much smaller because the
//! data flow is much smaller — no async jobs, no OCR artifacts, no
//! ledger versions, no editable filing surface.

use crate::expense_report_model::{
    ExpenseReport, ExpenseReportGeneralInformation, ExpenseReportTransactionLinesItem,
    ExpenseReportTransactionLinesItemMealDetails, ExpenseReportTransactionSummary,
};
use crate::extracted_receipt::ExtractedReceipt;
use crate::meta::{ConfidenceLevel, FieldMetadata, Wrapped};
use crate::validator::{ValidationReport, ValidationSeverity};
#[cfg(test)]
use crate::validator::{ValidationIssue, ValidationIssueKind};

const CSS: &str = include_str!("workbench_simple.css");

/// Render the full HTML page. `receipts` is the per-receipt extraction
/// list (used for the source-documents panel); `report` is the reduced
/// typed report; `validation` is the Pass-2 result.
///
/// "Download JSON" links are always emitted as RELATIVE URLs
/// (`extractions/<stem>.json`). The page is self-contained: whoever
/// serves the directory containing this HTML (Flask, file://, GCS,
/// nginx) makes the links work as long as the file is at the relative
/// path. No backend-specific URL knowledge in the renderer.
pub fn render_workbench_html(
    report: &ExpenseReport,
    receipts: &[ExtractedReceipt],
    validation: &ValidationReport,
) -> String {
    let mut html = String::with_capacity(8192);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>Stanford Expense Report</title>\n<style>\n");
    html.push_str(CSS);
    html.push_str("\n</style>\n</head>\n<body>\n<div class=\"shell\">\n");

    render_hero(&mut html, report, validation);
    render_summary_cards(&mut html, report);

    // Layout split: left rail (issues) + center column (form sections).
    // Rail is hidden via CSS when there are no issues so the center column
    // takes the full width.
    let has_issues = !validation.issues.is_empty();
    let layout_class = if has_issues { "layout split" } else { "layout solo" };
    html.push_str(&format!("<div class=\"{layout_class}\">\n"));

    if has_issues {
        html.push_str("<aside class=\"rail\">\n");
        render_issues_panel(&mut html, validation);
        html.push_str("</aside>\n");
    }

    html.push_str("<main class=\"main-col\">\n");
    render_general_information(&mut html, &report.general_information);
    render_transaction_lines(&mut html, report);
    render_transaction_summary(&mut html, &report.transaction_summary);
    render_source_documents(&mut html, receipts);
    html.push_str("</main>\n");

    html.push_str("</div>\n");
    html.push_str("</div>\n");
    // Tiny in-view-aware jump handler: if the target field is already
    // visible in the viewport, just flash the amber highlight without
    // scrolling. Otherwise let the browser do its default anchor scroll.
    html.push_str(JUMP_SCRIPT);
    html.push_str("</body>\n</html>\n");
    html
}

const JUMP_SCRIPT: &str = r#"<script>
// Click an issue → highlight the target field card. If the target is
// already visible, skip the browser's scroll-to-top behavior (still
// highlight). If it's out of view, let the browser scroll normally.
// Only one field card stays highlighted at a time.
document.addEventListener('click', function(e) {
  const link = e.target.closest('a.issue-jump');
  if (!link) return;
  const id = link.getAttribute('href').slice(1);
  const target = document.getElementById(id);
  if (!target) return;
  document.querySelectorAll('.field-card.active').forEach(function(el) {
    el.classList.remove('active');
  });
  target.classList.add('active');
  const rect = target.getBoundingClientRect();
  const inView = rect.top >= 0 && rect.bottom <= window.innerHeight;
  if (inView) {
    e.preventDefault();
    history.replaceState(null, '', '#' + id);
  }
});
</script>
"#;

// ─── Hero ──────────────────────────────────────────────────────────────────

fn render_hero(html: &mut String, report: &ExpenseReport, validation: &ValidationReport) {
    let issue_count = validation.issues.len();
    let line_count = report
        .transaction_lines
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0);
    let status_label = if issue_count == 0 {
        format!("{line_count} lines · ready to file")
    } else {
        format!("{line_count} lines · {issue_count} to review")
    };
    let status_class = if issue_count == 0 { "pill-good" } else { "pill-warn" };

    let payee = leaf_text(&report.general_information.payee.name).unwrap_or("(payee not yet set)");
    let event = leaf_text(&report.general_information.event_name).unwrap_or("(event not yet set)");

    html.push_str("<header class=\"hero\">\n");
    html.push_str("<p class=\"eyebrow\">Stanford Expense Report</p>\n");
    html.push_str("<h1>Review &amp; File</h1>\n");
    html.push_str(&format!(
        "<p class=\"hero-status\"><span class=\"pill {status_class}\">{}</span></p>\n",
        escape(&status_label)
    ));
    html.push_str(&format!(
        "<p class=\"hero-subtitle\">{} · {}</p>\n",
        escape(payee),
        escape(event)
    ));
    html.push_str("</header>\n");
}

// ─── Summary cards ─────────────────────────────────────────────────────────

fn render_summary_cards(html: &mut String, report: &ExpenseReport) {
    let trip_date =
        leaf_iso_date(&report.transaction_summary.transaction_date).unwrap_or("—".into());
    let total = report
        .transaction_summary
        .total_usd
        .value
        .map(|t| format!("${:.2}", t))
        .unwrap_or("—".into());
    let category = report
        .general_information
        .category
        .value
        .as_ref()
        .map(|c| c.as_str().replace('_', " "))
        .unwrap_or("—".into());

    let conf = confidence_breakdown(report);

    html.push_str("<section class=\"summary-cards\">\n");
    html.push_str(&summary_card("Trip Date", &trip_date));
    html.push_str(&summary_card("Total USD", &total));
    html.push_str(&summary_card("Category", &category));
    html.push_str(&summary_card(
        "Confidence",
        &format!(
            "{} <span class=\"conf-dot conf-high\"></span> {} <span class=\"conf-dot conf-medium\"></span> {} <span class=\"conf-dot conf-low\"></span>",
            conf.high, conf.medium, conf.low
        ),
    ));
    html.push_str("</section>\n");
}

fn summary_card(label: &str, value: &str) -> String {
    format!(
        "<article class=\"summary-card\"><p class=\"summary-label\">{}</p><p class=\"summary-value\">{}</p></article>\n",
        escape(label),
        value // value is constructed safely above (all our own strings)
    )
}

struct ConfBreakdown {
    high: usize,
    medium: usize,
    low: usize,
}

fn confidence_breakdown(report: &ExpenseReport) -> ConfBreakdown {
    let mut acc = ConfBreakdown { high: 0, medium: 0, low: 0 };
    let json = serde_json::to_value(report).unwrap_or(serde_json::Value::Null);
    walk_confidence(&json, &mut acc);
    acc
}

fn walk_confidence(value: &serde_json::Value, acc: &mut ConfBreakdown) {
    if let Some(obj) = value.as_object() {
        // Detect a Wrapped<T> shape: has both "value" and "_meta" keys.
        if let (Some(_), Some(meta)) = (obj.get("value"), obj.get("_meta")) {
            if let Some(level) = meta.get("confidence").and_then(|c| c.as_str()) {
                match level {
                    "high" => acc.high += 1,
                    "medium" => acc.medium += 1,
                    "low" => acc.low += 1,
                    _ => {}
                }
            }
            return;
        }
        for (_, v) in obj {
            walk_confidence(v, acc);
        }
    } else if let Some(arr) = value.as_array() {
        for v in arr {
            walk_confidence(v, acc);
        }
    }
}

// ─── Issues queue ──────────────────────────────────────────────────────────

fn render_issues_panel(html: &mut String, validation: &ValidationReport) {
    html.push_str("<section class=\"panel issues-panel\">\n");
    html.push_str("<p class=\"eyebrow\">Queue</p>\n<h2>Issues</h2>\n<ul class=\"issue-list\">\n");
    for issue in &validation.issues {
        let severity_class = match issue.severity {
            ValidationSeverity::Error => "issue-error",
            ValidationSeverity::Warning => "issue-warning",
        };
        let anchor = field_anchor(&issue.path);
        html.push_str(&format!(
            "<li class=\"issue-card {severity_class}\">\
             <p class=\"issue-path\">{}</p>\
             <p class=\"issue-message\">{}</p>\
             <a class=\"issue-jump\" href=\"#{}\">Jump to field →</a>\
             </li>\n",
            escape(&issue.path),
            escape(&issue.message),
            anchor
        ));
    }
    html.push_str("</ul>\n</section>\n");
}

fn field_anchor(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => c,
            _ => '-',
        })
        .collect::<String>()
        .to_ascii_lowercase()
}

// ─── General Information ───────────────────────────────────────────────────

fn render_general_information(html: &mut String, gi: &ExpenseReportGeneralInformation) {
    html.push_str("<section class=\"panel\">\n");
    html.push_str("<p class=\"eyebrow\">Section 1</p>\n<h2>General Information</h2>\n");
    html.push_str("<div class=\"field-grid\">\n");

    field_card_text(html, "Category", &gi.category, "expense_report.general_information.category", |c: &crate::expense_report_model::ExpenseReportGeneralInformationCategoryEnum| c.as_str().replace('_', " "));
    field_card_text(html, "Payee Name", &gi.payee.name, "expense_report.general_information.payee.name", |s: &String| s.clone());
    field_card_text(html, "Affiliation", &gi.payee.affiliation, "expense_report.general_information.payee.affiliation", |a: &crate::expense_report_model::ExpenseReportGeneralInformationPayeeAffiliationEnum| a.as_str().replace('_', " "));
    field_card_text(html, "Event Name", &gi.event_name, "expense_report.general_information.event_name", |s: &String| s.clone());
    field_card_optional_string(html, "Authorized By", gi.authorized_by.as_deref(), "expense_report.general_information.authorized_by");
    field_card_optional_enum(html, "Rush Processing", &gi.rush_processing, "expense_report.general_information.rush_processing", |r: &crate::expense_report_model::ExpenseReportGeneralInformationRushProcessingEnum| r.as_str().to_owned());
    field_card_text(html, "Payment Method", &gi.payment_method, "expense_report.general_information.payment_method", |s: &String| s.clone());

    // business_purpose is its own nested struct of T1 fields — render as a sub-block.
    html.push_str("</div>\n");
    html.push_str("<h3 class=\"subsection-title\">Business Purpose</h3>\n");
    html.push_str("<div class=\"field-grid\">\n");
    field_card_optional_string(html, "Who", gi.business_purpose.who.as_deref(), "expense_report.general_information.business_purpose.who");
    field_card_optional_string(html, "What", gi.business_purpose.what.as_deref(), "expense_report.general_information.business_purpose.what");
    field_card_optional_string(html, "When", gi.business_purpose.when.as_deref(), "expense_report.general_information.business_purpose.when");
    field_card_optional_string(html, "Where", gi.business_purpose.r#where.as_deref(), "expense_report.general_information.business_purpose.where");
    field_card_optional_string(html, "Why", gi.business_purpose.why.as_deref(), "expense_report.general_information.business_purpose.why");
    field_card_optional_string(html, "Key (30 chars)", gi.business_purpose.key_30char.as_deref(), "expense_report.general_information.business_purpose.key_30char");
    html.push_str("</div>\n");

    html.push_str("</section>\n");
}

// ─── Transaction Summary ───────────────────────────────────────────────────

fn render_transaction_summary(html: &mut String, ts: &ExpenseReportTransactionSummary) {
    html.push_str("<section class=\"panel\">\n");
    html.push_str("<p class=\"eyebrow\">Section 2</p>\n<h2>Transaction Summary</h2>\n");
    html.push_str("<div class=\"field-grid\">\n");

    field_card_text(html, "Transaction Date", &ts.transaction_date, "expense_report.transaction_summary.transaction_date", |d: &crate::expense_report_model::IsoDate| d.0.clone());
    field_card_text(html, "Transaction Number", &ts.transaction_number, "expense_report.transaction_summary.transaction_number", |s: &String| s.clone());
    field_card_optional_enum(html, "Status", &ts.status, "expense_report.transaction_summary.status", |s: &crate::expense_report_model::ExpenseReportTransactionSummaryStatusEnum| s.as_str().to_owned());
    field_card_text(html, "Total USD", &ts.total_usd, "expense_report.transaction_summary.total_usd", |t: &f64| format!("${:.2}", t));

    html.push_str("</div>\n</section>\n");
}

// ─── Transaction Lines ─────────────────────────────────────────────────────

fn render_transaction_lines(html: &mut String, report: &ExpenseReport) {
    html.push_str("<section class=\"panel\">\n");
    html.push_str("<p class=\"eyebrow\">Section 3</p>\n<h2>Transaction Lines</h2>\n");

    let lines = report.transaction_lines.as_ref().map(|v| v.as_slice()).unwrap_or(&[]);
    if lines.is_empty() {
        html.push_str("<p class=\"empty-note\">No transaction lines yet.</p>\n");
    } else {
        for (idx, line) in lines.iter().enumerate() {
            render_transaction_line(html, idx, line);
        }
    }
    html.push_str("</section>\n");
}

fn render_transaction_line(html: &mut String, idx: usize, line: &ExpenseReportTransactionLinesItem) {
    let summary_amount = line
        .common
        .line_amount_usd
        .value
        .map(|t| format!("${:.2}", t))
        .unwrap_or("—".into());
    let summary_date = line
        .common
        .date
        .value
        .as_ref()
        .map(|d| d.0.clone())
        .unwrap_or("—".into());
    let summary_kind = line
        .common
        .expense_type
        .value
        .as_ref()
        .map(|e| e.as_str().replace('_', " "))
        .unwrap_or("—".into());
    let venue = line
        .meal_details
        .as_ref()
        .and_then(|m| m.venue_name.value.as_deref())
        .unwrap_or("");

    html.push_str(&format!(
        "<details class=\"line-card\" open>\n\
         <summary class=\"line-summary\">\
           <span class=\"line-chev\" aria-hidden=\"true\"></span>\
           <span class=\"line-index\">#{}</span>\
           <span class=\"line-venue\">{}</span>\
           <span class=\"line-kind\">{}</span>\
           <span class=\"line-date\">{}</span>\
           <span class=\"line-amount\">{}</span>\
         </summary>\n\
         <div class=\"line-body\">\n",
        idx + 1,
        escape(if venue.is_empty() { "(no venue)" } else { venue }),
        escape(&summary_kind),
        escape(&summary_date),
        escape(&summary_amount),
    ));

    html.push_str("<div class=\"field-grid\">\n");
    let path_prefix = format!("expense_report.transaction_lines[{idx}].common");
    field_card_text(html, "Date", &line.common.date, &format!("{path_prefix}.date"), |d| d.0.clone());
    field_card_optional_money(html, "Amount (USD)", line.common.line_amount_usd.value, &format!("{path_prefix}.line_amount_usd"));
    field_card_text_opt(html, "Original Currency", &line.common.original_currency, &format!("{path_prefix}.original_currency"), |s: &String| s.clone());
    field_card_optional_money(html, "Original Amount", line.common.original_amount.value, &format!("{path_prefix}.original_amount"));
    field_card_text(html, "Expense Type", &line.common.expense_type, &format!("{path_prefix}.expense_type"), |e| e.as_str().replace('_', " "));
    field_card_text(html, "Remarks", &line.common.remarks, &format!("{path_prefix}.remarks"), |s: &String| s.clone());
    field_card_text_opt(html, "Country", &line.common.country_of_activity, &format!("{path_prefix}.country_of_activity"), |s: &String| s.clone());
    html.push_str("</div>\n");

    if let Some(meal) = &line.meal_details {
        html.push_str("<h4 class=\"subsection-title\">Meal Details</h4>\n");
        html.push_str("<div class=\"field-grid\">\n");
        let mp = format!("expense_report.transaction_lines[{idx}].meal_details");
        render_meal_details(html, meal, &mp);
        html.push_str("</div>\n");
    }

    html.push_str("</div>\n</details>\n");
}

fn render_meal_details(html: &mut String, meal: &ExpenseReportTransactionLinesItemMealDetails, path: &str) {
    field_card_text(html, "Venue", &meal.venue_name, &format!("{path}.venue_name"), |s: &String| s.clone());
    field_card_optional_money(html, "Tip", meal.tip_amount.value, &format!("{path}.tip_amount"));
    field_card_optional_money(html, "Alcohol", meal.alcohol_amount.value, &format!("{path}.alcohol_amount"));
    field_card_text(html, "Has Alcohol", &meal.has_alcohol_on_receipt, &format!("{path}.has_alcohol_on_receipt"), |b| if *b { "yes".into() } else { "no".into() });
}

// ─── Source documents (bottom) ─────────────────────────────────────────────

fn render_source_documents(html: &mut String, receipts: &[ExtractedReceipt]) {
    html.push_str("<section class=\"panel source-docs\">\n");
    html.push_str("<p class=\"eyebrow\">Provenance</p>\n<h2>Source Documents</h2>\n");
    html.push_str("<div class=\"doc-grid\">\n");
    for r in receipts {
        let kind = r.line.common.expense_type.value.as_ref().map(|e| e.as_str().replace('_', " ")).unwrap_or("—".into());
        let amount = r.line.common.line_amount_usd.value.map(|t| format!("${:.2}", t)).unwrap_or("—".into());
        // Relative link: works under any backend that serves the directory
        // containing this HTML. For the Flask app the workbench is served
        // from .../uploads/<id>/workbench.html and extractions live at
        // .../uploads/<id>/extractions/, so the link resolves naturally.
        let stem = std::path::Path::new(&r.source_filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&r.source_filename);
        let download_link = format!(
            "<a class=\"doc-download\" href=\"extractions/{}.json\" download>Download JSON</a>",
            escape(stem),
        );
        html.push_str(&format!(
            "<article class=\"doc-card\">\
              <div class=\"doc-topline\"><span class=\"badge\">{}</span><span class=\"doc-amount\">{}</span></div>\
              <h3>{}</h3>\
              {}\
             </article>\n",
            escape(&kind),
            escape(&amount),
            escape(&r.source_filename),
            download_link,
        ));
    }
    html.push_str("</div>\n</section>\n");
}

// ─── Field cards (Pattern A: compact, value + dot + tooltip) ──────────────

fn field_card_text<T>(
    html: &mut String,
    label: &str,
    wrapped: &Wrapped<T>,
    path: &str,
    fmt_value: impl Fn(&T) -> String,
) {
    let value = wrapped.value.as_ref().map(fmt_value);
    field_card_inner(html, label, path, value.as_deref().unwrap_or("—"), &wrapped.meta);
}

fn field_card_text_opt<T>(
    html: &mut String,
    label: &str,
    wrapped: &Wrapped<T>,
    path: &str,
    fmt_value: impl Fn(&T) -> String,
) {
    // Same as field_card_text but treats None as "—" (mirror for clarity).
    let value = wrapped.value.as_ref().map(fmt_value);
    field_card_inner(html, label, path, value.as_deref().unwrap_or("—"), &wrapped.meta);
}

fn field_card_optional_enum<T>(
    html: &mut String,
    label: &str,
    opt: &Option<T>,
    path: &str,
    fmt_value: impl Fn(&T) -> String,
) {
    let v = opt.as_ref().map(fmt_value);
    field_card_bare(html, label, path, v.as_deref().unwrap_or("—"));
}

fn field_card_optional_string(html: &mut String, label: &str, value: Option<&str>, path: &str) {
    field_card_bare(html, label, path, value.unwrap_or("—"));
}

fn field_card_optional_money(html: &mut String, label: &str, value: Option<f64>, path: &str) {
    let v = value.map(|t| format!("${:.2}", t));
    field_card_bare(html, label, path, v.as_deref().unwrap_or("—"));
}

/// Map a derivation origin code (set by reduce.rs when it builds derived
/// values) to a short human-readable phrase the FA can read directly.
/// Returns None for codes that describe absence (`not_applicable_for_*`,
/// `not_present_in_receipt`) — those shouldn't surface as provenance.
fn human_readable_origin(origin: &str) -> Option<&'static str> {
    match origin {
        "reduce.total_usd" => Some("sum of all line amounts"),
        "reduce.earliest_date" => Some("earliest date across all receipts"),
        "reduce.inferred_category" => Some("based on receipt currencies"),
        _ => None,
    }
}

fn field_card_inner(html: &mut String, label: &str, path: &str, value: &str, meta: &FieldMetadata) {
    let conf_class = match meta.confidence {
        ConfidenceLevel::High => "conf-high",
        ConfidenceLevel::Medium => "conf-medium",
        ConfidenceLevel::Low => "conf-low",
    };
    let needs_review_tag = if meta.needs_review {
        "<span class=\"needs-review\">needs review</span>"
    } else {
        ""
    };
    // Inline provenance: prefer a real receipt quote (T3 fields), then
    // fall back to a human-readable label for derived T2 fields (so the
    // FA can see WHY the value is what it is). Internal origin codes
    // like "not_applicable_for_domestic" are filtered out — those are
    // about absence, not the source of a present value.
    let evidence_quote = meta.evidence.iter().find_map(|e| e.quote.as_deref());
    let evidence_origin = meta
        .evidence
        .iter()
        .find_map(|e| e.origin.as_deref())
        .and_then(human_readable_origin);
    let evidence_block = match (evidence_quote, evidence_origin) {
        (Some(q), _) if !q.is_empty() => format!(
            "<p class=\"field-evidence\">“{}”</p>",
            escape(q)
        ),
        (_, Some(label)) => format!(
            "<p class=\"field-evidence\">{}</p>",
            escape(label)
        ),
        _ => String::new(),
    };

    html.push_str(&format!(
        "<div class=\"field-card\" id=\"{}\">\
           <p class=\"field-label\">{}</p>\
           <p class=\"field-value\">{} <span class=\"conf-dot {}\"></span></p>\
           {}\
           {}\
         </div>\n",
        field_anchor(path),
        escape(label),
        escape(value),
        conf_class,
        evidence_block,
        needs_review_tag,
    ));
}

fn field_card_bare(html: &mut String, label: &str, path: &str, value: &str) {
    // For T1/T2 fields without a Wrapped/_meta — no confidence dot.
    html.push_str(&format!(
        "<div class=\"field-card\" id=\"{}\">\
           <p class=\"field-label\">{}</p>\
           <p class=\"field-value\">{}</p>\
         </div>\n",
        field_anchor(path),
        escape(label),
        escape(value),
    ));
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn leaf_text(wrapped: &Wrapped<String>) -> Option<&str> {
    wrapped.value.as_deref()
}

fn leaf_iso_date(wrapped: &Wrapped<crate::expense_report_model::IsoDate>) -> Option<String> {
    wrapped.value.as_ref().map(|d| d.0.clone())
}

fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expense_report_model::IsoDate;
    use crate::extracted_receipt::Extras;
    use crate::reduce::reduce_to_expense_report;
    use crate::validator::ValidationReport;

    fn sample_receipt(filename: &str, date: &str, amount: f64, venue: &str) -> ExtractedReceipt {
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.common.date = Wrapped {
            value: Some(IsoDate(date.to_owned())),
            meta: FieldMetadata {
                confidence: ConfidenceLevel::High,
                evidence: vec![],
                needs_review: false,
                flags: vec![],
            },
        };
        line.common.line_amount_usd = Wrapped {
            value: Some(amount),
            meta: FieldMetadata::default(),
        };
        line.meal_details = Some(ExpenseReportTransactionLinesItemMealDetails {
            venue_name: Wrapped {
                value: Some(venue.to_owned()),
                meta: FieldMetadata::default(),
            },
            ..Default::default()
        });
        ExtractedReceipt {
            source_filename: filename.to_owned(),
            line,
            extras: Extras::default(),
        }
    }

    #[test]
    fn renders_html_without_crashing_on_real_shape() {
        let receipts = vec![
            sample_receipt("mels1.jpeg", "2026-04-19", 163.54, "MJ Sushi"),
            sample_receipt("mjsushi.jpeg", "2026-05-02", 79.59, "MJ Sushi"),
        ];
        let report = reduce_to_expense_report(&receipts);
        let html = render_workbench_html(&report, &receipts, &ValidationReport { issues: vec![] });

        // Smoke checks.
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Stanford Expense Report"));
        assert!(html.contains("MJ Sushi"));
        assert!(html.contains("$163.54"));
        assert!(html.contains("$79.59"));
        assert!(html.contains("Source Documents"));
        assert!(html.contains("Transaction Lines"));
    }

    #[test]
    fn issues_panel_only_renders_when_validation_has_issues() {
        let receipts = vec![sample_receipt("a.jpeg", "2026-04-19", 1.0, "X")];
        let report = reduce_to_expense_report(&receipts);

        let no_issues = render_workbench_html(&report, &receipts, &ValidationReport { issues: vec![] });
        // No-issues path: layout is "solo" (no rail), and no <aside> is rendered.
        // Check for the actual rendered link element (class="issue-jump") rather
        // than a substring like "Jump to field" which can spuriously match in
        // the inlined CSS/JS comments.
        assert!(no_issues.contains("layout solo"));
        assert!(!no_issues.contains("<aside class=\"rail\""));
        assert!(!no_issues.contains("class=\"issue-jump\""));

        let with_issues = render_workbench_html(
            &report,
            &receipts,
            &ValidationReport {
                issues: vec![ValidationIssue {
                    severity: ValidationSeverity::Error,
                    kind: crate::validator::ValidationIssueKind::MissingRequiredField,
                    path: "expense_report.general_information.payee.name".to_owned(),
                    schema_path: "expense_report.general_information.payee.name".to_owned(),
                    message: "Required field is missing".to_owned(),
                }],
            },
        );
        // Has-issues path: layout is "split", rail is rendered, jump link present.
        assert!(with_issues.contains("layout split"));
        assert!(with_issues.contains("<aside class=\"rail\""));
        assert!(with_issues.contains("class=\"issue-jump\""));
    }
}
