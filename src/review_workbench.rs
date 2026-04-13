use std::collections::BTreeMap;

use crate::draft::EvidenceReference;
use crate::review_packet::{CopyField, FilingStatus, ReviewPacket};

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvidenceRecord {
    id: String,
    bucket: String,
    title: String,
    meta: String,
    quote: Option<String>,
    used_by: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkbenchIndex {
    field_targets: BTreeMap<String, String>,
    instance_targets: BTreeMap<String, String>,
    section_targets: BTreeMap<String, String>,
    field_to_evidence: BTreeMap<String, String>,
    evidence_records: Vec<EvidenceRecord>,
}

pub fn render_review_workbench_html(packet: &ReviewPacket) -> String {
    let index = build_workbench_index(packet);
    let mut html = String::new();

    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Expense Report Review Workbench</title>\n",
    );
    html.push_str("<style>\n");
    html.push_str(include_str!("review_workbench.css"));
    html.push_str("\n</style>\n");
    html.push_str(
        "<script>\nfunction copyFieldValue(button){const value=button.getAttribute('data-copy');if(!value){return;}navigator.clipboard.writeText(value);button.textContent='Copied';setTimeout(()=>{button.textContent='Copy';},900);}\n</script>\n",
    );
    html.push_str("</head>\n<body>\n<div class=\"shell\">\n");

    render_header(&mut html, packet);
    html.push_str("<main class=\"workbench-grid\">\n");
    render_issues_panel(&mut html, packet, &index);
    render_copy_panel(&mut html, packet, &index);
    render_evidence_panel(&mut html, &index);
    html.push_str("</main>\n");
    render_attachments_panel(&mut html, packet);
    html.push_str("</div>\n</body>\n</html>\n");
    html
}

fn render_header(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<header class=\"hero\">\n");
    html.push_str("<div class=\"hero-copy\">\n");
    html.push_str("<p class=\"eyebrow\">Expense Report Review</p>\n");
    html.push_str("<h1>FA Workbench</h1>\n");
    html.push_str("<p class=\"hero-status\">");
    html.push_str(&escape_html(filing_status_label(packet.summary.filing_status)));
    html.push_str("</p>\n");
    html.push_str("<p class=\"hero-subtitle\">");
    html.push_str(&escape_html(
        packet
            .summary
            .payee_name
            .as_deref()
            .unwrap_or("Unknown payee"),
    ));
    html.push_str(" · ");
    html.push_str(&escape_html(
        packet
            .summary
            .event_name
            .as_deref()
            .unwrap_or("Missing event name"),
    ));
    html.push_str("</p>\n");
    html.push_str("</div>\n");
    html.push_str("<div class=\"hero-cards\">\n");
    summary_card(
        html,
        "Trip Window",
        packet.summary.trip_window.as_deref().unwrap_or("[missing]"),
    );
    summary_card(
        html,
        "Report Total USD",
        packet.summary.report_total_usd.as_deref().unwrap_or("[missing]"),
    );
    summary_card(
        html,
        "Readiness",
        &format!(
            "{} automation · {} user input · {} review",
            packet.summary.readiness.automation_gap_count,
            packet.summary.readiness.user_input_gap_count,
            packet.summary.readiness.manual_review_count
        ),
    );
    summary_card(
        html,
        "Confidence",
        &format!(
            "{} high · {} medium · {} low",
            packet.summary.confidence.high,
            packet.summary.confidence.medium,
            packet.summary.confidence.low
        ),
    );
    html.push_str("</div>\n</header>\n");
}

fn render_issues_panel(html: &mut String, packet: &ReviewPacket, index: &WorkbenchIndex) {
    html.push_str("<section class=\"panel issues-panel\">\n");
    html.push_str("<div class=\"panel-heading\"><p class=\"eyebrow\">Queue</p><h2>Issues</h2></div>\n");
    if packet.issues_queue.is_empty() {
        html.push_str("<p class=\"empty-state\">No blocking or review issues.</p>\n");
    } else {
        html.push_str("<ul class=\"issue-list\">\n");
        for issue in &packet.issues_queue {
            html.push_str("<li class=\"issue-card ");
            html.push_str(issue_class_name(issue.class));
            html.push_str("\">\n");
            html.push_str("<div class=\"issue-topline\">");
            html.push_str("<span class=\"issue-class\">");
            html.push_str(&escape_html(issue_class_name(issue.class)));
            html.push_str("</span>");
            if let Some(source) = issue.source.as_deref() {
                html.push_str("<span class=\"issue-source\">");
                html.push_str(&escape_html(source));
                html.push_str("</span>");
            }
            html.push_str("</div>\n");
            html.push_str("<h3>");
            html.push_str(&escape_html(&issue.label));
            html.push_str("</h3>\n");
            html.push_str("<p class=\"issue-path\">");
            html.push_str(&escape_html(&issue.path));
            html.push_str("</p>\n");
            if let Some(value) = issue.current_value.as_deref() {
                html.push_str("<p class=\"issue-value\">Current value: ");
                html.push_str(&escape_html(value));
                html.push_str("</p>\n");
            }
            html.push_str("<p class=\"issue-message\">");
            html.push_str(&escape_html(&issue.message));
            html.push_str("</p>\n");
            if let Some(target) = issue_target(issue.path.as_str(), index) {
                html.push_str("<a class=\"issue-link\" href=\"#");
                html.push_str(&escape_html(&target));
                html.push_str("\">Jump to field</a>\n");
            }
            html.push_str("</li>\n");
        }
        html.push_str("</ul>\n");
    }
    html.push_str("</section>\n");
}

fn render_copy_panel(html: &mut String, packet: &ReviewPacket, index: &WorkbenchIndex) {
    html.push_str("<section class=\"panel copy-panel\">\n");
    html.push_str("<div class=\"panel-heading\"><p class=\"eyebrow\">Oracle Copy View</p><h2>Ready-To-Copy Fields</h2></div>\n");
    for section in &packet.copy_sections {
        let section_id = anchor_id("section", &section.key);
        html.push_str("<section class=\"copy-section\" id=\"");
        html.push_str(&escape_html(&section_id));
        html.push_str("\">\n");
        html.push_str("<h3>");
        html.push_str(&escape_html(&section.label));
        html.push_str("</h3>\n");
        for instance in &section.instances {
            let instance_id = anchor_id("instance", &instance.path);
            html.push_str("<article class=\"copy-instance\" id=\"");
            html.push_str(&escape_html(&instance_id));
            html.push_str("\">\n");
            if section.repeated {
                html.push_str("<h4>");
                html.push_str(&escape_html(&instance.label));
                html.push_str("</h4>\n");
            }
            html.push_str("<div class=\"field-list\">\n");
            for field in &instance.fields {
                render_copy_field(html, field, index);
            }
            html.push_str("</div>\n</article>\n");
        }
        html.push_str("</section>\n");
    }
    html.push_str("</section>\n");
}

fn render_copy_field(html: &mut String, field: &CopyField, index: &WorkbenchIndex) {
    let field_id = index
        .field_targets
        .get(&field.path)
        .cloned()
        .unwrap_or_else(|| anchor_id("field", &field.path));
    html.push_str("<article class=\"field-card");
    if field.needs_review {
        html.push_str(" needs-review");
    }
    if !field.present {
        html.push_str(" missing");
    }
    html.push_str("\" id=\"");
    html.push_str(&escape_html(&field_id));
    html.push_str("\">\n");
    html.push_str("<div class=\"field-head\">\n");
    html.push_str("<div><p class=\"field-label\">");
    html.push_str(&escape_html(&field.label));
    html.push_str("</p><p class=\"field-path\">");
    html.push_str(&escape_html(&field.path));
    html.push_str("</p></div>\n");
    html.push_str("<div class=\"field-badges\">");
    if let Some(source) = field.source.as_deref() {
        badge(html, source);
    }
    badge(html, &field.entry_mode);
    if field.required {
        badge(html, "required");
    }
    if field.needs_review {
        badge(html, "review");
    }
    html.push_str("</div>\n</div>\n");
    html.push_str("<div class=\"field-body\">\n");
    html.push_str("<div class=\"field-value\">");
    html.push_str(&escape_html(
        field.value.as_deref().unwrap_or("[missing]"),
    ));
    html.push_str("</div>\n");
    html.push_str("<div class=\"field-actions\">");
    if let Some(value) = field.value.as_deref() {
        html.push_str("<button class=\"copy-button\" type=\"button\" onclick=\"copyFieldValue(this)\" data-copy=\"");
        html.push_str(&escape_html_attribute(value));
        html.push_str("\">Copy</button>");
    }
    if let Some(evidence_id) = index.field_to_evidence.get(&field.path) {
        html.push_str("<a class=\"evidence-link\" href=\"#");
        html.push_str(&escape_html(evidence_id));
        html.push_str("\">Evidence</a>");
    }
    html.push_str("</div>\n</div>\n");
    html.push_str("</article>\n");
}

fn render_evidence_panel(html: &mut String, index: &WorkbenchIndex) {
    html.push_str("<aside class=\"panel evidence-panel\">\n");
    html.push_str("<div class=\"panel-heading\"><p class=\"eyebrow\">Evidence</p><h2>Reference Index</h2></div>\n");
    if index.evidence_records.is_empty() {
        html.push_str("<p class=\"empty-state\">No evidence references available.</p>\n");
    } else {
        let mut current_bucket = String::new();
        for record in &index.evidence_records {
            if record.bucket != current_bucket {
                if !current_bucket.is_empty() {
                    html.push_str("</div>\n");
                }
                current_bucket = record.bucket.clone();
                html.push_str("<div class=\"evidence-group\">\n<h3>");
                html.push_str(&escape_html(&current_bucket));
                html.push_str("</h3>\n");
            }
            html.push_str("<article class=\"evidence-card\" id=\"");
            html.push_str(&escape_html(&record.id));
            html.push_str("\">\n<h4>");
            html.push_str(&escape_html(&record.title));
            html.push_str("</h4>\n<p class=\"evidence-meta\">");
            html.push_str(&escape_html(&record.meta));
            html.push_str("</p>\n");
            if let Some(quote) = record.quote.as_deref() {
                html.push_str("<blockquote>");
                html.push_str(&escape_html(quote));
                html.push_str("</blockquote>\n");
            }
            if !record.used_by.is_empty() {
                html.push_str("<p class=\"evidence-usage\">Used by: ");
                html.push_str(&escape_html(&record.used_by.join(", ")));
                html.push_str("</p>\n");
            }
            html.push_str("</article>\n");
        }
        if !current_bucket.is_empty() {
            html.push_str("</div>\n");
        }
    }
    html.push_str("</aside>\n");
}

fn render_attachments_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"panel attachments-panel\">\n");
    html.push_str("<div class=\"panel-heading\"><p class=\"eyebrow\">Attachments</p><h2>Checklist</h2></div>\n");
    if packet.attachment_checklist.is_empty() {
        html.push_str("<p class=\"empty-state\">No projected attachments.</p>\n");
    } else {
        html.push_str("<div class=\"attachment-list\">\n");
        for item in &packet.attachment_checklist {
            html.push_str("<article class=\"attachment-card\">\n<h3>Line ");
            html.push_str(&(item.line_index + 1).to_string());
            html.push_str("</h3>\n<p>");
            html.push_str(&escape_html(
                item.expense_type.as_deref().unwrap_or("unknown expense type"),
            ));
            html.push_str("</p>\n");
            if let Some(remarks) = item.remarks.as_deref() {
                html.push_str("<p class=\"attachment-remarks\">");
                html.push_str(&escape_html(remarks));
                html.push_str("</p>\n");
            }
            html.push_str("<ul>");
            for filename in &item.filenames {
                html.push_str("<li>");
                html.push_str(&escape_html(filename));
                html.push_str("</li>");
            }
            html.push_str("</ul>\n</article>\n");
        }
        html.push_str("</div>\n");
    }
    html.push_str("</section>\n");
}

fn build_workbench_index(packet: &ReviewPacket) -> WorkbenchIndex {
    let mut index = WorkbenchIndex::default();
    let mut evidence_map = BTreeMap::<String, EvidenceRecord>::new();

    for section in &packet.copy_sections {
        index
            .section_targets
            .insert(section.key.clone(), anchor_id("section", &section.key));
        for instance in &section.instances {
            index
                .instance_targets
                .insert(instance.path.clone(), anchor_id("instance", &instance.path));
            for field in &instance.fields {
                let field_id = anchor_id("field", &field.path);
                index
                    .field_targets
                    .insert(field.path.clone(), field_id.clone());
                for evidence in &field.evidence {
                    let key = evidence_key(evidence);
                    let record = evidence_map.entry(key.clone()).or_insert_with(|| EvidenceRecord {
                        id: anchor_id("evidence", &key),
                        bucket: evidence_bucket(evidence),
                        title: evidence_title(evidence),
                        meta: evidence_meta(evidence),
                        quote: evidence.quote.clone(),
                        used_by: Vec::new(),
                    });
                    let usage = format!("{} · {}", instance.label, field.label);
                    if !record.used_by.contains(&usage) {
                        record.used_by.push(usage);
                    }
                    index
                        .field_to_evidence
                        .entry(field.path.clone())
                        .or_insert_with(|| record.id.clone());
                }
            }
        }
    }

    index.evidence_records = evidence_map.into_values().collect();
    index
}

fn issue_target(path: &str, index: &WorkbenchIndex) -> Option<String> {
    if let Some(target) = index.field_targets.get(path) {
        return Some(target.clone());
    }

    if let Some(instance_path) = transaction_line_instance_path(path) {
        if let Some(target) = index.instance_targets.get(instance_path) {
            return Some(target.clone());
        }
    }

    if let Some(section_key) = section_key_for_path(path) {
        if let Some(target) = index.section_targets.get(section_key) {
            return Some(target.clone());
        }
    }

    None
}

fn section_key_for_path(path: &str) -> Option<&str> {
    let remainder = path.strip_prefix("expense_report.")?;
    if remainder.starts_with("transaction_lines[") {
        return Some("transaction_lines");
    }
    match remainder.find('.') {
        Some(dot_index) => Some(&remainder[..dot_index]),
        None => Some(remainder),
    }
}

fn transaction_line_instance_path(path: &str) -> Option<&str> {
    let prefix = "expense_report.transaction_lines[";
    let remainder = path.strip_prefix(prefix)?;
    let closing_index = remainder.find(']')?;
    Some(&path[..prefix.len() + closing_index + 1])
}

fn summary_card(html: &mut String, label: &str, value: &str) {
    html.push_str("<article class=\"summary-card\"><p class=\"summary-label\">");
    html.push_str(&escape_html(label));
    html.push_str("</p><p class=\"summary-value\">");
    html.push_str(&escape_html(value));
    html.push_str("</p></article>\n");
}

fn badge(html: &mut String, value: &str) {
    html.push_str("<span class=\"badge\">");
    html.push_str(&escape_html(value));
    html.push_str("</span>");
}

fn evidence_key(evidence: &EvidenceReference) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        evidence_kind_name(evidence),
        evidence.document_id.as_deref().unwrap_or(""),
        evidence.filename.as_deref().unwrap_or(""),
        evidence.page.map(|value| value.to_string()).as_deref().unwrap_or(""),
        evidence.quote.as_deref().unwrap_or(""),
        evidence.origin.as_deref().unwrap_or("")
    )
}

fn evidence_bucket(evidence: &EvidenceReference) -> String {
    evidence
        .filename
        .clone()
        .or_else(|| evidence.document_id.clone())
        .or_else(|| evidence.origin.clone())
        .unwrap_or_else(|| "other".to_owned())
}

fn evidence_title(evidence: &EvidenceReference) -> String {
    evidence
        .filename
        .clone()
        .or_else(|| evidence.document_id.clone())
        .or_else(|| evidence.origin.clone())
        .unwrap_or_else(|| evidence_kind_name(evidence).to_owned())
}

fn evidence_meta(evidence: &EvidenceReference) -> String {
    let mut parts = vec![evidence_kind_name(evidence).to_owned()];
    if let Some(page) = evidence.page {
        parts.push(format!("page {page}"));
    }
    if let Some(origin) = evidence.origin.as_deref() {
        parts.push(origin.to_owned());
    }
    parts.join(" · ")
}

fn evidence_kind_name(evidence: &EvidenceReference) -> &'static str {
    match evidence.kind {
        crate::draft::EvidenceKind::Document => "document",
        crate::draft::EvidenceKind::DocumentSpan => "document_span",
        crate::draft::EvidenceKind::SystemGenerated => "system_generated",
        crate::draft::EvidenceKind::UserInput => "user_input",
    }
}

fn anchor_id(prefix: &str, value: &str) -> String {
    let mut id = String::from(prefix);
    id.push('-');
    for ch in value.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => id.push(ch.to_ascii_lowercase()),
            _ => id.push('-'),
        }
    }
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    id.trim_matches('-').to_owned()
}

fn filing_status_label(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "automation blocked",
        FilingStatus::UserInputRequired => "user input required",
        FilingStatus::ManualReviewRequired => "manual review required",
        FilingStatus::ReadyToFile => "ready to file",
    }
}

fn issue_class_name(class: crate::ReadinessIssueClass) -> &'static str {
    match class {
        crate::ReadinessIssueClass::AutomationGap => "automation-gap",
        crate::ReadinessIssueClass::UserInputRequired => "user-input-required",
        crate::ReadinessIssueClass::ManualReview => "manual-review",
        crate::ReadinessIssueClass::OtherWarning => "other-warning",
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_html_attribute(value: &str) -> String {
    escape_html(value).replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::render_review_workbench_html;
    use crate::bundle_synthesis::{synthesize_bundle_projection, synthesize_bundle_projection_with_fx, StaticFxRateProvider};
    use crate::review_packet::build_review_packet;
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};

    fn synthetic_documents() -> Vec<crate::ExtractedDocumentFacts> {
        generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect()
    }

    #[test]
    fn renders_fa_workbench_with_issue_links_and_copy_buttons() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet =
            build_review_packet(&projection.bundle, &projection.draft, &projection.validation)
                .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("<title>Expense Report Review Workbench</title>"));
        assert!(rendered.contains("FA Workbench"));
        assert!(rendered.contains("Jump to field"));
        assert!(rendered.contains("copyFieldValue(this)"));
        assert!(rendered.contains("synthetic_flight_itinerary_baseline.md"));
        assert!(rendered.contains("Business meal during travel in Singapore"));
    }

    #[test]
    fn workbench_filters_irrelevant_optional_fields_from_line_cards() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet =
            build_review_packet(&projection.bundle, &projection.draft, &projection.validation)
                .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(!rendered.contains("Shared With Transaction Number"));
        assert!(!rendered.contains("Per Subject Amount"));
        assert!(rendered.contains("Hotel Name"));
        assert!(rendered.contains("Venue Name"));
    }

    #[test]
    fn no_fx_workbench_still_shows_automation_blocked_status() {
        let projection = synthesize_bundle_projection(&synthetic_documents());
        let packet =
            build_review_packet(&projection.bundle, &projection.draft, &projection.validation)
                .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("automation blocked"));
        assert!(rendered.contains("expense_report.transaction_summary.total_usd"));
    }

    #[test]
    fn issue_fallback_links_cover_section_level_anchors() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet =
            build_review_packet(&projection.bundle, &projection.draft, &projection.validation)
                .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains(
            "<a class=\"issue-link\" href=\"#section-transaction-lines\">Jump to field</a>"
        ));
        assert!(rendered.contains(
            "<a class=\"issue-link\" href=\"#section-general-information\">Jump to field</a>"
        ));
    }
}
