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
    ExpenseReportTransactionLinesItemGroundTransportDetails,
    ExpenseReportTransactionLinesItemLodgingDetails,
    ExpenseReportTransactionLinesItemMealDetails, ExpenseReportTransactionSummary,
};
use crate::extracted_receipt::ExtractedReceipt;
use crate::meta::{ConfidenceLevel, FieldMetadata, Wrapped};
use crate::validator::{ValidationIssue, ValidationIssueKind, ValidationReport, ValidationSeverity};

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
    render_breakdown(&mut html, report);

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
    // Toast for click-to-copy feedback. Hidden by default; the JS
    // handler toggles `.visible` on copy and clears it after 1.5s.
    html.push_str("<div id=\"copy-toast\" class=\"copy-toast\" role=\"status\" aria-live=\"polite\"></div>\n");
    // Tiny in-view-aware jump handler + click-to-copy handler — see
    // JUMP_SCRIPT below for the in-script comments.
    html.push_str(JUMP_SCRIPT);
    html.push_str("</body>\n</html>\n");
    html
}

const JUMP_SCRIPT: &str = r#"<script>
// On page load: walk the issues panel and tag the corresponding field
// cards with `.has-issue`. Lets CSS pale-red those cards without the
// renderer needing to thread issue paths through every field-card
// helper. Recovers the same coupling at the display layer that's
// already encoded in the issues panel's <a href=#...> anchors.
document.addEventListener('DOMContentLoaded', function() {
  document.querySelectorAll('a.issue-jump').forEach(function(link) {
    const id = link.getAttribute('href').slice(1);
    const target = document.getElementById(id);
    if (target) target.classList.add('has-issue');
  });
});

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

// Click a card with [data-copy-value] → copy that value to clipboard
// and show a brief toast. Skips if the click was on the issue-jump
// arrow inside the card (so jump and copy don't conflict).
let copyToastTimer = null;
function showCopyToast(text) {
  const toast = document.getElementById('copy-toast');
  if (!toast) return;
  toast.textContent = text;
  toast.classList.add('visible');
  if (copyToastTimer) clearTimeout(copyToastTimer);
  copyToastTimer = setTimeout(function() {
    toast.classList.remove('visible');
  }, 1500);
}
document.addEventListener('click', function(e) {
  // If the click went to an issue-jump arrow, let that handler win.
  if (e.target.closest('a.issue-jump')) return;
  const target = e.target.closest('[data-copy-value]');
  if (!target) return;
  const value = target.getAttribute('data-copy-value');
  if (!value) return;
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(value).then(function() {
      showCopyToast('Copied: ' + value);
    }).catch(function() {
      showCopyToast('Copy failed');
    });
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
    html.push_str(&summary_card("Trip Date", &trip_date, Some(&trip_date)));
    html.push_str(&summary_card("Total USD", &total, Some(&total)));
    html.push_str(&summary_card("Category", &category, Some(&category)));
    html.push_str(&summary_card(
        "Confidence",
        &format!(
            "{} <span class=\"conf-dot conf-high\"></span> {} <span class=\"conf-dot conf-medium\"></span> {} <span class=\"conf-dot conf-low\"></span>",
            conf.high, conf.medium, conf.low
        ),
        // Confidence card has no plain-text value worth copying.
        None,
    ));
    html.push_str("</section>\n");
}

// ─── Breakdown chart ──────────────────────────────────────────────────────

/// One bucket of the per-kind expense breakdown.
struct BreakdownSegment {
    label: &'static str,
    icon: &'static str,
    color: &'static str,
    amount: f64,
}

/// Group transaction lines by expense kind (which detail block is non-null)
/// and sum line_amount_usd within each group. Lines with no amount are
/// excluded; zero-total kinds are filtered out so the chart stays clean.
fn kind_breakdown(report: &ExpenseReport) -> Vec<BreakdownSegment> {
    // Stable order: meal first, then transport, then the future kinds we
    // haven't shipped yet (in insertion order). Predictable layout when
    // the FA looks at multiple reports side by side.
    let mut totals: Vec<BreakdownSegment> = vec![
        BreakdownSegment { label: "Meal",     icon: "🍽️", color: "#f97316", amount: 0.0 },
        BreakdownSegment { label: "Transport", icon: "🚗", color: "#3b82f6", amount: 0.0 },
        BreakdownSegment { label: "Airfare",   icon: "✈️", color: "#8b5cf6", amount: 0.0 },
        BreakdownSegment { label: "Lodging",   icon: "🏨", color: "#10b981", amount: 0.0 },
        BreakdownSegment { label: "Conference",icon: "🎟️", color: "#ec4899", amount: 0.0 },
        BreakdownSegment { label: "Car Rental",icon: "🚙", color: "#14b8a6", amount: 0.0 },
        BreakdownSegment { label: "Gift",      icon: "🎁", color: "#eab308", amount: 0.0 },
        BreakdownSegment { label: "Human Subject", icon: "🧪", color: "#6b7280", amount: 0.0 },
        BreakdownSegment { label: "Other",     icon: "📄", color: "#9ca3af", amount: 0.0 },
    ];

    let lines = match report.transaction_lines.as_ref() {
        Some(v) => v,
        None => return Vec::new(),
    };

    for line in lines {
        let Some(amt) = line.common.line_amount_usd.value else { continue; };
        let bucket = if line.meal_details.is_some() { 0 }
            else if line.ground_transport_details.is_some() { 1 }
            else if line.airfare_details.is_some() { 2 }
            else if line.lodging_details.is_some() { 3 }
            else if line.conference_registration_details.is_some() { 4 }
            else if line.car_rental_details.is_some() { 5 }
            else if line.gift_details.is_some() { 6 }
            else if line.human_subject_details.is_some() { 7 }
            else { 8 };
        totals[bucket].amount += amt;
    }

    totals.into_iter().filter(|s| s.amount > 0.0).collect()
}

fn render_breakdown(html: &mut String, report: &ExpenseReport) {
    let segments = kind_breakdown(report);
    if segments.is_empty() {
        return;
    }
    let total: f64 = segments.iter().map(|s| s.amount).sum();
    if total <= 0.0 {
        return;
    }

    html.push_str("<section class=\"panel breakdown\">\n");
    html.push_str("<p class=\"eyebrow\">Spend by kind</p>\n");
    html.push_str(&format!("<h2 class=\"breakdown-total\">${:.2}</h2>\n", total));

    // Stacked horizontal bar. Each segment is a flex child sized by its
    // share of the total.
    html.push_str("<div class=\"breakdown-bar\">\n");
    for seg in &segments {
        let pct = seg.amount / total * 100.0;
        html.push_str(&format!(
            "<div class=\"breakdown-segment\" style=\"width:{:.4}%; background:{};\" title=\"{} {} (${:.2}, {:.0}%)\"></div>\n",
            pct, seg.color, escape(seg.icon), escape(seg.label), seg.amount, pct,
        ));
    }
    html.push_str("</div>\n");

    // Legend below the bar: emoji + label + amount + percent. Each row
    // aligns with a colored swatch on the left so the legend reads as
    // "what color = what kind."
    html.push_str("<ul class=\"breakdown-legend\">\n");
    for seg in &segments {
        let pct = seg.amount / total * 100.0;
        html.push_str(&format!(
            "<li class=\"breakdown-legend-item\">\
              <span class=\"breakdown-swatch\" style=\"background:{};\" aria-hidden=\"true\"></span>\
              <span class=\"breakdown-icon\" aria-hidden=\"true\">{}</span>\
              <span class=\"breakdown-label\">{}</span>\
              <span class=\"breakdown-amount\">${:.2}</span>\
              <span class=\"breakdown-pct\">{:.0}%</span>\
             </li>\n",
            seg.color, escape(seg.icon), escape(seg.label), seg.amount, pct,
        ));
    }
    html.push_str("</ul>\n");
    html.push_str("</section>\n");
}

fn summary_card(label: &str, value: &str, copy_value: Option<&str>) -> String {
    // copy_value is None when `value` is HTML (e.g. the Confidence card
    // uses inline spans for the dot legend) — those aren't worth copying
    // anyway. Plain-text cards get the copy attribute + the cursor hint.
    let (extras, class) = match copy_value {
        Some(v) if v != "—" => (
            format!(" data-copy-value=\"{}\" title=\"Click to copy\"", escape(v)),
            " summary-card--copyable",
        ),
        _ => (String::new(), ""),
    };
    format!(
        "<article class=\"summary-card{class}\"{extras}><p class=\"summary-label\">{}</p><p class=\"summary-value\">{}</p></article>\n",
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

/// FA-facing buckets for the issues panel. The category header itself
/// tells the FA what action is needed, so each issue card stays terse —
/// no per-card "you must fill this in" verbosity required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IssueCategory {
    MissingFields,
    NeedsReview,
    Other,
}

impl IssueCategory {
    fn label(self) -> &'static str {
        match self {
            IssueCategory::MissingFields => "Missing fields",
            IssueCategory::NeedsReview => "Needs review",
            IssueCategory::Other => "Other issues",
        }
    }
}

fn issue_category(kind: ValidationIssueKind) -> IssueCategory {
    use ValidationIssueKind::*;
    match kind {
        MissingRequiredField | MissingDependency => IssueCategory::MissingFields,
        ManualReviewRequired | LowConfidenceWithoutReview => IssueCategory::NeedsReview,
        // Data-integrity / schema-internal kinds. Rare in practice;
        // shown only when present so the rail isn't cluttered with an
        // empty section.
        TypeMismatch
        | InvalidEnumValue
        | MissingFieldMetadata
        | MissingEvidenceReference
        | OrphanFieldMetadata
        | InvalidEvidenceReference
        | UnsupportedExpression
        | UnresolvedExpressionReference
        | InternalSchemaError => IssueCategory::Other,
    }
}

fn render_issues_panel(html: &mut String, validation: &ValidationReport) {
    html.push_str("<section class=\"panel issues-panel\">\n");
    html.push_str("<p class=\"eyebrow\">Queue</p>\n<h2>Issues</h2>\n");

    // Group by FA-facing category, preserving original order within each
    // bucket. Empty categories are hidden — a clean report shows nothing.
    let mut by_category: std::collections::BTreeMap<IssueCategory, Vec<&ValidationIssue>> =
        std::collections::BTreeMap::new();
    for issue in &validation.issues {
        by_category
            .entry(issue_category(issue.kind))
            .or_default()
            .push(issue);
    }

    for (category, issues) in by_category.iter() {
        html.push_str(&format!(
            "<div class=\"issue-section\">\n\
             <h3 class=\"issue-section-title\">{} <span class=\"issue-count\">({})</span></h3>\n\
             <ul class=\"issue-list\">\n",
            escape((*category).label()),
            issues.len(),
        ));
        for issue in issues {
            let severity_class = match issue.severity {
                ValidationSeverity::Error => "issue-error",
                ValidationSeverity::Warning => "issue-warning",
            };
            let anchor = field_anchor(&issue.path);
            html.push_str(&format!(
                "<li class=\"issue-card {severity_class}\">\
                 <p class=\"issue-label\">{}</p>\
                 <a class=\"issue-jump\" href=\"#{}\" aria-label=\"Jump to field\">→</a>\
                 </li>\n",
                escape(&friendly_field_label(&issue.path)),
                anchor
            ));
        }
        html.push_str("</ul>\n</div>\n");
    }

    html.push_str("</section>\n");
}

/// Map a dotted schema path to an FA-readable label for the issues
/// panel. Examples:
///   expense_report.general_information.payee.name -> "Payee Name"
///   expense_report.general_information.business_purpose.who -> "Business Purpose: Who"
///   expense_report.transaction_lines[0].common.country_of_activity -> "Line 1: Country of Activity"
///   expense_report.transaction_summary.transaction_date -> "Trip Date"
///
/// A small override map handles cases where the workbench display name
/// differs from the schema path (e.g. transaction_date appears as "Trip
/// Date" in the summary card, so the issue label matches).
fn friendly_field_label(path: &str) -> String {
    // Overrides for fields where the schema name and the workbench
    // display name differ. Match against the suffix so the lookup is
    // stable across Line N / non-Line contexts.
    let overrides: &[(&str, &str)] = &[
        ("transaction_summary.transaction_date", "Trip Date"),
        ("transaction_summary.total_usd", "Total USD"),
        ("transaction_summary.transaction_number", "Transaction Number"),
        ("general_information.category", "Category"),
        ("general_information.payment_method", "Payment Method"),
        ("general_information.event_name", "Event Name"),
        ("general_information.authorized_by", "Authorized By"),
        ("general_information.rush_processing", "Rush Processing"),
    ];
    for (suffix, label) in overrides {
        if path.ends_with(suffix) {
            return (*label).to_owned();
        }
    }

    // Strip the expense_report. prefix.
    let stripped = path.strip_prefix("expense_report.").unwrap_or(path);

    // Split on '.', skipping group prefixes like "general_information"
    // (the bucket headers in the issues panel already convey context).
    // For transaction_lines[N], extract the index and prepend "Line N+1:".
    let parts: Vec<&str> = stripped.split('.').collect();
    let mut prefix = String::new();
    let mut tail_start = 0;

    if let Some(first) = parts.first() {
        if let Some(idx_str) = first
            .strip_prefix("transaction_lines[")
            .and_then(|s| s.strip_suffix(']'))
        {
            if let Ok(idx) = idx_str.parse::<usize>() {
                prefix = format!("Line {}: ", idx + 1);
                tail_start = 1;
            }
        }
        if first == &"general_information"
            || first == &"transaction_summary"
            || first == &"per_diem_expenses"
            || first == &"mileage_expenses"
            || first == &"allocation_and_approvers"
        {
            tail_start = 1;
        }
    }

    // Drop block names that just structure the schema and add no
    // FA-facing context — the field name itself is informative. Keep
    // `payee`, `business_purpose`, etc. because they DO give context
    // ("Payee: Name" vs just "Name"; "Business Purpose: Who" vs "Who").
    let intermediate_blocks = [
        "common",
        "meal_details",
        "ground_transport_details",
        "airfare_details",
        "lodging_details",
        "conference_registration_details",
        "car_rental_details",
        "gift_details",
        "human_subject_details",
    ];
    let tail: Vec<String> = parts[tail_start..]
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            // Keep the LAST component always (it's the field name).
            // Strip intermediate group names like "common".
            let is_last = i == parts[tail_start..].len() - 1;
            if !is_last && intermediate_blocks.contains(p) {
                None
            } else {
                Some(title_case(p))
            }
        })
        .collect();

    format!("{}{}", prefix, tail.join(": "))
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
    let headline = line_summary_headline(line);
    let icon = line_kind_icon(line);

    // Note: previously this row also rendered a "kind text" span (e.g.
    // "Business Meal with Alcohol"). FA feedback: that text duplicates
    // what the left-side icon already conveys. Dropped — the icon is
    // the visual indicator. The full string still appears on the
    // expense_type field card inside the body for anyone who needs the
    // exact words.
    html.push_str(&format!(
        "<details class=\"line-card\" open>\n\
         <summary class=\"line-summary\">\
           <span class=\"line-chev\" aria-hidden=\"true\"></span>\
           <span class=\"line-icon\" aria-hidden=\"true\">{}</span>\
           <span class=\"line-index\">#{}</span>\
           <span class=\"line-venue\">{}</span>\
           <span class=\"line-date\">{}</span>\
           <span class=\"line-amount\">{}</span>\
         </summary>\n\
         <div class=\"line-body\">\n",
        icon,
        idx + 1,
        escape(&headline),
        escape(&summary_date),
        escape(&summary_amount),
    ));

    html.push_str("<div class=\"field-grid\">\n");
    let path_prefix = format!("expense_report.transaction_lines[{idx}].common");
    field_card_text(html, "Date", &line.common.date, &format!("{path_prefix}.date"), |d| d.0.clone());
    field_card_optional_money(html, "Amount (USD)", line.common.line_amount_usd.value, &format!("{path_prefix}.line_amount_usd"));
    field_card_text_opt(html, "Original Currency", &line.common.original_currency, &format!("{path_prefix}.original_currency"), |s: &String| s.clone());
    field_card_optional_money(html, "Original Amount", line.common.original_amount.value, &format!("{path_prefix}.original_amount"));
    field_card_text(html, "Expense Type", &line.common.expense_type, &format!("{path_prefix}.expense_type"), |e| display_expense_type(e, line.meal_details.as_ref()));
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

    if let Some(gt) = &line.ground_transport_details {
        html.push_str("<h4 class=\"subsection-title\">Ground Transport Details</h4>\n");
        html.push_str("<div class=\"field-grid\">\n");
        let gp = format!("expense_report.transaction_lines[{idx}].ground_transport_details");
        render_ground_transport_details(html, gt, &gp);
        html.push_str("</div>\n");
    }

    if let Some(lodging) = &line.lodging_details {
        html.push_str("<h4 class=\"subsection-title\">Lodging Details</h4>\n");
        html.push_str("<div class=\"field-grid\">\n");
        let lp = format!("expense_report.transaction_lines[{idx}].lodging_details");
        render_lodging_details(html, lodging, &lp);
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

fn render_ground_transport_details(
    html: &mut String,
    gt: &ExpenseReportTransactionLinesItemGroundTransportDetails,
    path: &str,
) {
    field_card_text(html, "Service Provider", &gt.service_provider, &format!("{path}.service_provider"), |s: &String| s.clone());
    field_card_text(html, "Origin", &gt.origin, &format!("{path}.origin"), |s: &String| s.clone());
    field_card_text(html, "Destination", &gt.destination, &format!("{path}.destination"), |s: &String| s.clone());
}

fn render_lodging_details(
    html: &mut String,
    lodging: &ExpenseReportTransactionLinesItemLodgingDetails,
    path: &str,
) {
    field_card_text(html, "Hotel", &lodging.hotel_name, &format!("{path}.hotel_name"), |s: &String| s.clone());
    field_card_text(html, "Location", &lodging.location, &format!("{path}.location"), |s: &String| s.clone());
    field_card_text(html, "Check In", &lodging.check_in_date, &format!("{path}.check_in_date"), |d| d.0.clone());
    field_card_text(html, "Check Out", &lodging.check_out_date, &format!("{path}.check_out_date"), |d| d.0.clone());
    // number_of_nights is T2 (derived by reduction). Render as integer
    // even though the underlying type is f64 — nights are whole numbers.
    field_card_text(html, "Nights", &lodging.number_of_nights, &format!("{path}.number_of_nights"), |n| format!("{}", *n as i64));
    field_card_text(html, "Daily Rate", &lodging.daily_rate, &format!("{path}.daily_rate"), |r| format!("${:.2}", r));
    field_card_text(html, "Booking Method", &lodging.booking_method, &format!("{path}.booking_method"), |b| title_case(b.as_str()));
    field_card_text(html, "Shared Lodging", &lodging.is_shared_lodging, &format!("{path}.is_shared_lodging"), |b| if *b { "yes".into() } else { "no".into() });
    // shared_with_transaction_number is T1 (FA fills only if shared);
    // render only the card when is_shared_lodging is true to keep the
    // grid uncluttered on the common case.
    if let Some(true) = lodging.is_shared_lodging.value {
        field_card_optional_string(html, "Shared With", lodging.shared_with_transaction_number.as_deref(), &format!("{path}.shared_with_transaction_number"));
    }
}

// ─── Source documents (bottom) ─────────────────────────────────────────────

fn render_source_documents(html: &mut String, receipts: &[ExtractedReceipt]) {
    html.push_str("<section class=\"panel source-docs\">\n");
    html.push_str("<p class=\"eyebrow\">Provenance</p>\n<h2>Source Documents</h2>\n");
    html.push_str("<div class=\"doc-grid\">\n");
    for r in receipts {
        let kind = r.line.common.expense_type.value.as_ref().map(|e| display_expense_type(e, r.line.meal_details.as_ref())).unwrap_or("—".into());
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

// ─── Display assembly: expense_type + alcohol → FA-facing string ──────────
//
// The schema collapsed the alcohol-suffix enum variants (Phase 1 Pair A);
// alcohol presence lives on `meal_details.has_alcohol_on_receipt`. The
// workbench reassembles the FA-facing string here so what the FA sees in
// the workbench matches what they'd pick from the Stanford portal's
// 4-value dropdown for meals. This is display-only — the underlying
// schema, reduction, and validation never see the suffix.
//
// A future submission layer (when it exists) will reassemble the suffixed
// enum value separately for the portal API; that is not this code's job.
fn display_expense_type(
    enum_val: &crate::expense_report_model::ExpenseReportTransactionLinesItemCommonExpenseTypeEnum,
    meal: Option<&ExpenseReportTransactionLinesItemMealDetails>,
) -> String {
    use crate::expense_report_model::ExpenseReportTransactionLinesItemCommonExpenseTypeEnum::*;
    let base = title_case(enum_val.as_str());
    // Append " with Alcohol" only for the meal variants when the meal
    // details report alcohol on the receipt. Other expense kinds aren't
    // affected.
    let with_alcohol = matches!(enum_val, BusinessMeal | GroupTravelMeal)
        && meal
            .and_then(|m| m.has_alcohol_on_receipt.value)
            .unwrap_or(false);
    if with_alcohol {
        format!("{base} with Alcohol")
    } else {
        base
    }
}

/// Per-kind icon for the line summary header. Emoji-based so the
/// workbench stays self-contained — no asset shipping, no SVG markup.
/// Falls back to a generic receipt for unknown kinds.
fn line_kind_icon(line: &ExpenseReportTransactionLinesItem) -> &'static str {
    if line.meal_details.is_some() {
        return "🍽️";
    }
    if line.ground_transport_details.is_some() {
        return "🚗";
    }
    if line.airfare_details.is_some() {
        return "✈️";
    }
    if line.lodging_details.is_some() {
        return "🏨";
    }
    if line.conference_registration_details.is_some() {
        return "🎟️";
    }
    if line.car_rental_details.is_some() {
        return "🚙";
    }
    if line.gift_details.is_some() {
        return "🎁";
    }
    if line.human_subject_details.is_some() {
        return "🧪";
    }
    "📄"
}

/// Per-kind headline for the collapsed transaction-line summary header.
/// For meal lines, it's the venue name (the most distinctive identifier
/// for "which restaurant was this?"); for transport lines, the service
/// provider plays the same role ("Lyft" / "Uber"). Future per-kind
/// blocks should extend this with their natural headline.
fn line_summary_headline(line: &ExpenseReportTransactionLinesItem) -> String {
    if let Some(meal) = &line.meal_details {
        if let Some(venue) = meal.venue_name.value.as_deref() {
            if !venue.is_empty() {
                return venue.to_owned();
            }
        }
        return "(no venue)".to_owned();
    }
    if let Some(gt) = &line.ground_transport_details {
        if let Some(provider) = gt.service_provider.value.as_deref() {
            if !provider.is_empty() {
                return provider.to_owned();
            }
        }
        return "(no service provider)".to_owned();
    }
    "—".to_owned()
}

/// Title-case an underscore-separated identifier:
/// "business_meal" -> "Business Meal", "ground_transportation_foreign" ->
/// "Ground Transportation Foreign".
fn title_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
        "reduce.lodging.daily_rate" => Some("average of per-night rates"),
        "reduce.lodging.number_of_nights" => Some("check-out minus check-in"),
        "reduce.fx.mock" => Some("converted via mock FX rate (placeholder; real-time tool TBD)"),
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

    // Confidence reason: shown for ALL confidence levels when present.
    // This is both FA-facing context AND a debugging/audit signal — the
    // FA (or anyone reviewing extractions) can read the model's
    // justification and decide whether to trust the value. High-conf
    // reasons are rendered in unobtrusive gray; medium/low get the
    // prominent amber styling that flags "look at this." Older cached
    // extractions without the field render the same as before (no extra
    // line).
    let reason_block = match meta.confidence_reason.as_deref() {
        Some(r) if !r.is_empty() => {
            let (label, class) = match meta.confidence {
                ConfidenceLevel::High => ("High", "field-reason field-reason--high"),
                ConfidenceLevel::Medium => ("Medium", "field-reason"),
                ConfidenceLevel::Low => ("Low", "field-reason"),
            };
            format!("<p class=\"{class}\">{label}: {}</p>", escape(r))
        }
        _ => String::new(),
    };

    let copy_attrs = copy_attrs_for(value);
    html.push_str(&format!(
        "<div class=\"field-card{copy_class}\" id=\"{}\"{copy_attrs}>\
           <p class=\"field-label\">{}</p>\
           <p class=\"field-value\">{} <span class=\"conf-dot {}\"></span></p>\
           {}\
           {}\
           {}\
         </div>\n",
        field_anchor(path),
        escape(label),
        escape(value),
        conf_class,
        evidence_block,
        reason_block,
        needs_review_tag,
        copy_class = if copy_attrs.is_empty() { "" } else { " field-card--copyable" },
        copy_attrs = copy_attrs,
    ));
}

fn field_card_bare(html: &mut String, label: &str, path: &str, value: &str) {
    // For T1/T2 fields without a Wrapped/_meta — no confidence dot.
    let copy_attrs = copy_attrs_for(value);
    html.push_str(&format!(
        "<div class=\"field-card{copy_class}\" id=\"{}\"{copy_attrs}>\
           <p class=\"field-label\">{}</p>\
           <p class=\"field-value\">{}</p>\
         </div>\n",
        field_anchor(path),
        escape(label),
        escape(value),
        copy_class = if copy_attrs.is_empty() { "" } else { " field-card--copyable" },
        copy_attrs = copy_attrs,
    ));
}

/// Build the `data-copy-value` + `title` attributes for a card value.
/// Empty string when the value is missing ("—") or blank — those aren't
/// worth offering to copy.
fn copy_attrs_for(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "—" {
        String::new()
    } else {
        format!(
            " data-copy-value=\"{}\" title=\"Click to copy\"",
            escape(trimmed)
        )
    }
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
                confidence_reason: None,
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

    #[test]
    fn display_expense_type_assembles_alcohol_suffix_for_meals() {
        use crate::expense_report_model::ExpenseReportTransactionLinesItemCommonExpenseTypeEnum::*;

        // Helper: build a meal_details with the given alcohol bool.
        let meal_with = |alcohol: bool| ExpenseReportTransactionLinesItemMealDetails {
            has_alcohol_on_receipt: Wrapped::known(alcohol),
            ..Default::default()
        };

        // business_meal × alcohol toggle
        assert_eq!(display_expense_type(&BusinessMeal, Some(&meal_with(true))), "Business Meal with Alcohol");
        assert_eq!(display_expense_type(&BusinessMeal, Some(&meal_with(false))), "Business Meal");
        // group_travel_meal × alcohol toggle
        assert_eq!(display_expense_type(&GroupTravelMeal, Some(&meal_with(true))), "Group Travel Meal with Alcohol");
        assert_eq!(display_expense_type(&GroupTravelMeal, Some(&meal_with(false))), "Group Travel Meal");
        // No meal_details (e.g. extractor didn't fill it) — no suffix.
        assert_eq!(display_expense_type(&BusinessMeal, None), "Business Meal");
        // Non-meal expense kinds — alcohol bool is irrelevant, no suffix even if true.
        assert_eq!(display_expense_type(&AirfareDomestic, None), "Airfare Domestic");
        assert_eq!(display_expense_type(&GroundTransportationForeign, Some(&meal_with(true))), "Ground Transportation Foreign");
    }

    #[test]
    fn friendly_field_label_examples() {
        // Override map wins over generic title-casing.
        assert_eq!(
            friendly_field_label("expense_report.transaction_summary.transaction_date"),
            "Trip Date"
        );
        assert_eq!(
            friendly_field_label("expense_report.general_information.category"),
            "Category"
        );

        // Plain general_information field: drop the bucket prefix, title-case.
        assert_eq!(
            friendly_field_label("expense_report.general_information.payee.name"),
            "Payee: Name"
        );
        assert_eq!(
            friendly_field_label("expense_report.general_information.business_purpose.who"),
            "Business Purpose: Who"
        );

        // Transaction-line field: extract index, drop "common"/detail-block names.
        assert_eq!(
            friendly_field_label("expense_report.transaction_lines[0].common.country_of_activity"),
            "Line 1: Country Of Activity"
        );
        assert_eq!(
            friendly_field_label("expense_report.transaction_lines[2].meal_details.venue_name"),
            "Line 3: Venue Name"
        );
        assert_eq!(
            friendly_field_label("expense_report.transaction_lines[1].ground_transport_details.origin"),
            "Line 2: Origin"
        );
    }

    #[test]
    fn kind_breakdown_groups_and_sums() {
        // Build a report with 2 meal lines + 1 transport line, each
        // with a known amount. Verify segments contain only the kinds
        // present, in the canonical order (meal first, then transport).
        let mut report = ExpenseReport::default();
        let mut lines = Vec::new();
        for amount in [100.0, 50.0] {
            let mut line = ExpenseReportTransactionLinesItem::default();
            line.common.line_amount_usd = Wrapped::known(amount);
            line.meal_details = Some(ExpenseReportTransactionLinesItemMealDetails::default());
            lines.push(line);
        }
        let mut transport_line = ExpenseReportTransactionLinesItem::default();
        transport_line.common.line_amount_usd = Wrapped::known(25.0);
        transport_line.ground_transport_details =
            Some(ExpenseReportTransactionLinesItemGroundTransportDetails::default());
        lines.push(transport_line);
        report.transaction_lines = Some(lines);

        let segments = kind_breakdown(&report);
        assert_eq!(segments.len(), 2, "only kinds with non-zero totals should appear");
        assert_eq!(segments[0].label, "Meal");
        assert!((segments[0].amount - 150.0).abs() < 1e-9);
        assert_eq!(segments[1].label, "Transport");
        assert!((segments[1].amount - 25.0).abs() < 1e-9);
    }

    #[test]
    fn kind_breakdown_excludes_amountless_lines() {
        // Lines without line_amount_usd shouldn't contribute to any
        // bucket — they're skipped, not counted as $0.
        let mut report = ExpenseReport::default();
        let mut line = ExpenseReportTransactionLinesItem::default();
        line.meal_details = Some(ExpenseReportTransactionLinesItemMealDetails::default());
        // line.common.line_amount_usd left as Wrapped::unknown() — value=None
        report.transaction_lines = Some(vec![line]);

        let segments = kind_breakdown(&report);
        assert!(segments.is_empty(), "amountless line should produce no segments");
    }

    #[test]
    fn line_kind_icon_per_detail_block() {
        // Default line (no detail block) gets the generic fallback.
        let bare = ExpenseReportTransactionLinesItem::default();
        assert_eq!(line_kind_icon(&bare), "📄");

        // Meal line.
        let mut meal_line = ExpenseReportTransactionLinesItem::default();
        meal_line.meal_details = Some(ExpenseReportTransactionLinesItemMealDetails::default());
        assert_eq!(line_kind_icon(&meal_line), "🍽️");

        // Transport line.
        let mut transport_line = ExpenseReportTransactionLinesItem::default();
        transport_line.ground_transport_details =
            Some(ExpenseReportTransactionLinesItemGroundTransportDetails::default());
        assert_eq!(line_kind_icon(&transport_line), "🚗");
    }

    #[test]
    fn issue_category_buckets() {
        use crate::validator::ValidationIssueKind::*;
        assert_eq!(issue_category(MissingRequiredField), IssueCategory::MissingFields);
        assert_eq!(issue_category(MissingDependency), IssueCategory::MissingFields);
        assert_eq!(issue_category(ManualReviewRequired), IssueCategory::NeedsReview);
        assert_eq!(issue_category(LowConfidenceWithoutReview), IssueCategory::NeedsReview);
        assert_eq!(issue_category(TypeMismatch), IssueCategory::Other);
        assert_eq!(issue_category(InternalSchemaError), IssueCategory::Other);
    }
}
