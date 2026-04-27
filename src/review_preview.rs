use crate::field_conventions::FieldControl;
use crate::review_packet::{CopyField, FilingStatus, ReviewPacket};

pub fn render_review_preview_html(packet: &ReviewPacket) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Expense Packet Preview</title>\n",
    );
    html.push_str("<style>\n");
    html.push_str(
        "body{margin:0;background:#f7f3eb;color:#1f1a17;font-family:Georgia,'Times New Roman',serif;}\
         .shell{max-width:1160px;margin:0 auto;padding:28px 24px 56px;}\
         .hero,.panel,.section-card,.instance-card,.document-card,.attachment-card,.issue-card{background:#fff;border:1px solid #e3dac9;border-radius:20px;box-shadow:0 10px 24px rgba(41,27,16,.05);}\
         .hero{padding:24px;margin-bottom:18px;}\
         .eyebrow{margin:0 0 8px;color:#9a5f33;font-size:12px;font-weight:700;letter-spacing:.18em;text-transform:uppercase;}\
         h1,h2,h3{margin:0;color:#1f1a17;}\
         h1{font-size:42px;line-height:1.05;}\
         h2{font-size:28px;line-height:1.15;}\
         h3{font-size:20px;line-height:1.2;}\
         .hero-subtitle,.hero-note,.summary-card p,.field-note,.field-source,.summary-list,.issue-card p,.attachment-card p,.document-card p,.field-value,.field-label,.field-source-link,.placeholder,.document-field-label,.document-field-value{margin:0;}\
         .hero-status{margin:10px 0 0;font-size:18px;font-weight:700;color:#2d5a46;text-transform:capitalize;}\
         .hero-subtitle{margin-top:8px;font-size:18px;color:#645a4f;}\
         .hero-note{margin-top:14px;color:#645a4f;max-width:780px;line-height:1.5;}\
         .action-bar{display:flex;flex-wrap:wrap;gap:12px;margin:0 0 18px;}\
         .action-link,.action-button{appearance:none;border:0;border-radius:999px;padding:11px 16px;font-size:15px;font-weight:700;cursor:pointer;text-decoration:none;}\
         .action-link{background:#2f5b53;color:#fff;}\
         .action-link.secondary{background:#efe7da;color:#40352d;}\
         .action-button{background:#1f1a17;color:#fff;}\
         .summary-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:14px;margin-bottom:18px;}\
         .summary-card{padding:18px;}\
         .summary-label{font-size:12px;font-weight:700;letter-spacing:.14em;text-transform:uppercase;color:#806c59;}\
         .summary-value{margin-top:10px;font-size:24px;line-height:1.2;}\
         .status-pill{display:inline-flex;align-items:center;border-radius:999px;padding:6px 11px;font-size:12px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;}\
         .status-pill.ready{background:#dcebd8;color:#21472f;}\
         .status-pill.attention{background:#f5e2c7;color:#7a4a1f;}\
         .status-pill.blocked{background:#f2d7d2;color:#7e2f26;}\
         .status-pill.review{background:#ece2d7;color:#584132;}\
         .section-stack,.instance-stack,.field-stack,.issue-list,.attachment-grid,.document-grid{display:grid;gap:16px;}\
         .section-card{padding:20px;margin-bottom:18px;}\
         .instance-card{padding:18px;background:#fbf8f2;}\
         .instance-heading{display:flex;justify-content:space-between;align-items:flex-start;gap:12px;margin-bottom:14px;}\
         .field-stack{grid-template-columns:repeat(2,minmax(0,1fr));}\
         .field-card{padding:15px 16px;border:1px solid #e6dece;border-radius:16px;background:#fff;}\
         .field-topline{display:flex;justify-content:space-between;align-items:flex-start;gap:12px;margin-bottom:8px;}\
         .field-label{font-size:18px;font-weight:700;}\
         .field-value{margin-top:10px;font-size:18px;line-height:1.4;white-space:pre-wrap;word-break:break-word;}\
         .placeholder{color:#8d5b35;font-style:italic;}\
         .field-note,.field-source{margin-top:10px;color:#645a4f;font-size:14px;line-height:1.45;}\
         .field-source-links{display:flex;flex-wrap:wrap;gap:10px;margin-top:10px;}\
         .field-source-link{color:#1b536e;text-decoration:none;font-weight:700;}\
         .structured-table{width:100%;border-collapse:collapse;margin-top:10px;font-size:14px;}\
         .structured-table th,.structured-table td{padding:9px 10px;border-bottom:1px solid #e6dece;text-align:left;vertical-align:top;}\
         .structured-table th{font-size:12px;letter-spacing:.1em;text-transform:uppercase;color:#7d6f62;}\
         .panel{padding:20px;margin-bottom:18px;}\
         .summary-list{margin-top:12px;padding-left:20px;color:#4d4035;line-height:1.5;}\
         .issue-list{margin-top:14px;}\
         .issue-card{padding:16px;}\
         .issue-label{font-size:18px;font-weight:700;}\
         .issue-meta{margin-top:6px;color:#6f6257;font-size:14px;}\
         .issue-message{margin-top:8px;color:#30271f;line-height:1.45;}\
         .attachment-grid,.document-grid{grid-template-columns:repeat(2,minmax(0,1fr));margin-top:14px;}\
         .attachment-card,.document-card{padding:16px;}\
         .attachment-files,.document-fields{margin:12px 0 0;padding-left:18px;}\
         .document-field{display:grid;gap:3px;margin-top:10px;}\
         .document-field-label{font-size:12px;letter-spacing:.08em;text-transform:uppercase;color:#7d6f62;font-weight:700;}\
         .document-field-value{line-height:1.4;}\
         .document-actions{display:flex;flex-wrap:wrap;gap:10px;margin-top:14px;}\
         .document-link{color:#1b536e;text-decoration:none;font-weight:700;}\
         @media (max-width: 900px){.summary-grid,.field-stack,.attachment-grid,.document-grid{grid-template-columns:1fr;}.shell{padding:20px 16px 40px;}h1{font-size:34px;}}\
         @media print{body{background:#fff;color:#000;}.shell{max-width:none;padding:0;}.action-bar{display:none !important;}.hero,.panel,.section-card,.instance-card,.field-card,.issue-card,.attachment-card,.document-card{box-shadow:none;border-color:#c8c0b3;break-inside:avoid;}a{color:#000;text-decoration:none;}}",
    );
    html.push_str("\n</style>\n</head>\n<body>\n<div class=\"shell\">\n");
    render_header(&mut html, packet);
    render_action_bar(&mut html);
    render_summary_panel(&mut html, packet);
    render_issues_panel(&mut html, packet);
    render_sections(&mut html, packet);
    render_attachments_panel(&mut html, packet);
    render_documents_panel(&mut html, packet);
    html.push_str("</div>\n</body>\n</html>\n");
    html
}

fn render_header(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<header class=\"hero\">");
    html.push_str("<p class=\"eyebrow\">Final Packet Preview</p>");
    html.push_str("<h1>Expense Report Preview</h1>");
    html.push_str("<p class=\"hero-status\">");
    html.push_str(filing_status_label(packet.summary.filing_status));
    html.push_str("</p>");
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
    html.push_str("</p>");
    html.push_str("<p class=\"hero-note\">This page is the filing-shaped packet preview. It is read-only, designed for final review, and printable as a PDF.</p>");
    html.push_str("</header>");
}

fn render_action_bar(html: &mut String) {
    html.push_str("<nav class=\"action-bar\">");
    html.push_str("<a class=\"action-link\" href=\"workbench\">Back to FA workbench</a>");
    html.push_str("<a class=\"action-link secondary\" href=\"overview\">Report overview</a>");
    html.push_str(
        "<button class=\"action-button\" type=\"button\" onclick=\"window.print()\">Print / Save PDF</button>",
    );
    html.push_str("</nav>");
}

fn render_summary_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"summary-grid\">");
    summary_card(
        html,
        "Payee",
        packet.summary.payee_name.as_deref().unwrap_or("[missing]"),
        packet.summary.payee_name.is_some(),
    );
    summary_card(
        html,
        "Event",
        packet.summary.event_name.as_deref().unwrap_or("[missing]"),
        packet.summary.event_name.is_some(),
    );
    summary_card(
        html,
        "Trip Window",
        packet.summary.trip_window.as_deref().unwrap_or("[missing]"),
        packet.summary.trip_window.is_some(),
    );
    summary_card(
        html,
        "Report Total USD",
        packet
            .summary
            .report_total_usd
            .as_deref()
            .unwrap_or("[missing]"),
        packet.summary.report_total_usd.is_some(),
    );
    summary_card(
        html,
        "Category",
        packet.summary.category.as_deref().unwrap_or("[missing]"),
        packet.summary.category.is_some(),
    );
    summary_card(
        html,
        "Transaction Type",
        packet
            .summary
            .transaction_type
            .as_deref()
            .unwrap_or("[missing]"),
        packet.summary.transaction_type.is_some(),
    );
    html.push_str("</section>");

    html.push_str("<section class=\"panel\">");
    html.push_str("<p class=\"eyebrow\">Packet Summary</p><h2>Readiness Snapshot</h2>");
    html.push_str("<ul class=\"summary-list\">");
    html.push_str(&format!(
        "<li>{} document(s) and {} transaction line(s)</li>",
        packet.summary.document_count, packet.summary.transaction_line_count
    ));
    html.push_str(&format!(
        "<li>{} system item(s), {} field(s) need your input, {} field(s) need review, {} warning(s)</li>",
        packet.summary.readiness.automation_gap_count,
        packet.summary.readiness.user_input_gap_count,
        packet.summary.readiness.manual_review_count,
        packet.summary.readiness.other_warning_count
    ));
    html.push_str(&format!(
        "<li>{} high-confidence, {} medium-confidence, {} low-confidence fields</li>",
        packet.summary.confidence.high,
        packet.summary.confidence.medium,
        packet.summary.confidence.low
    ));
    html.push_str("</ul></section>");
}

fn render_issues_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"panel\">");
    html.push_str("<p class=\"eyebrow\">Open Items</p><h2>Unresolved Fields</h2>");
    if packet.issues_queue.is_empty() {
        html.push_str(
            "<p class=\"hero-note\">No unresolved fields remain in the current packet.</p>",
        );
    } else {
        html.push_str("<div class=\"issue-list\">");
        for issue in &packet.issues_queue {
            html.push_str("<article class=\"issue-card\">");
            html.push_str("<div class=\"field-topline\"><div><p class=\"issue-label\">");
            html.push_str(&escape_html(&issue.label));
            html.push_str("</p></div><span class=\"status-pill ");
            html.push_str(issue_class_name(issue.class));
            html.push_str("\">");
            html.push_str(issue_label(issue.class));
            html.push_str("</span></div>");
            html.push_str("<p class=\"issue-message\">");
            html.push_str(&escape_html(&issue.message));
            html.push_str("</p>");
            if let Some(value) = issue.current_value.as_deref() {
                html.push_str("<p class=\"issue-meta\">Current value: ");
                html.push_str(&escape_html(value));
                html.push_str("</p>");
            }
            html.push_str("</article>");
        }
        html.push_str("</div>");
    }
    html.push_str("</section>");
}

fn render_sections(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"section-stack\">");
    for section in &packet.copy_sections {
        html.push_str("<article class=\"section-card\">");
        html.push_str("<p class=\"eyebrow\">Preview Section</p><h2>");
        html.push_str(&escape_html(&section.label));
        html.push_str("</h2>");
        html.push_str("<div class=\"instance-stack\">");
        for instance in &section.instances {
            html.push_str("<section class=\"instance-card\">");
            html.push_str("<div class=\"instance-heading\"><div><h3>");
            html.push_str(&escape_html(&instance.label));
            html.push_str("</h3></div>");
            html.push_str("<span class=\"status-pill ready\">Read-only preview</span></div>");
            html.push_str("<div class=\"field-stack\">");
            for field in &instance.fields {
                render_field(html, field);
            }
            html.push_str("</div></section>");
        }
        html.push_str("</div></article>");
    }
    html.push_str("</section>");
}

fn render_field(html: &mut String, field: &CopyField) {
    html.push_str("<article class=\"field-card\">");
    html.push_str("<div class=\"field-topline\"><div><p class=\"field-label\">");
    html.push_str(&escape_html(&field.label));
    html.push_str("</p></div><span class=\"status-pill ");
    html.push_str(field_status_class(field));
    html.push_str("\">");
    html.push_str(field_status_label(field));
    html.push_str("</span></div>");

    match field.control {
        FieldControl::StructuredList => render_structured_value(html, field),
        _ => render_scalar_value(html, field),
    }

    if field.needs_review {
        html.push_str("<p class=\"field-note\">This field is present, but the current packet still marks it for review.</p>");
    } else if !field.present && field.required {
        html.push_str("<p class=\"field-note\">This required field still needs to be completed before filing.</p>");
    }
    let links = evidence_document_links(&field.evidence);
    if !links.is_empty() {
        html.push_str("<div class=\"field-source-links\">");
        for (document_id, filename) in links {
            html.push_str("<a class=\"field-source-link\" href=\"document/");
            html.push_str(&escape_html_attribute(&document_id));
            html.push('/');
            html.push_str(&escape_html_attribute(&filename));
            html.push_str(
                "\" target=\"_blank\" rel=\"noreferrer noopener\">Open source document</a>",
            );
        }
        html.push_str("</div>");
    }
    html.push_str("</article>");
}

fn render_scalar_value(html: &mut String, field: &CopyField) {
    html.push_str("<p class=\"field-value");
    if !field.present {
        html.push_str(" placeholder");
    }
    html.push_str("\">");
    html.push_str(&escape_html(field.value.as_deref().unwrap_or("[missing]")));
    html.push_str("</p>");
}

fn render_structured_value(html: &mut String, field: &CopyField) {
    if field.collection_rows.is_empty() {
        html.push_str("<p class=\"field-value placeholder\">[missing]</p>");
        return;
    }
    html.push_str("<table class=\"structured-table\"><thead><tr>");
    for column in &field.collection_columns {
        html.push_str("<th>");
        html.push_str(&escape_html(&column.label));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for row in &field.collection_rows {
        html.push_str("<tr>");
        for column in &field.collection_columns {
            html.push_str("<td>");
            html.push_str(&escape_html(
                row.values
                    .get(&column.key)
                    .map(String::as_str)
                    .unwrap_or(""),
            ));
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
}

fn render_attachments_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"panel\">");
    html.push_str("<p class=\"eyebrow\">Attachments</p><h2>Attachment Checklist</h2>");
    if packet.attachment_checklist.is_empty() {
        html.push_str(
            "<p class=\"hero-note\">No projected attachment checklist items are available yet.</p>",
        );
    } else {
        html.push_str("<div class=\"attachment-grid\">");
        for item in &packet.attachment_checklist {
            html.push_str("<article class=\"attachment-card\">");
            html.push_str("<h3>Line ");
            html.push_str(&(item.line_index + 1).to_string());
            html.push_str("</h3><p>");
            html.push_str(&escape_html(
                item.expense_type
                    .as_deref()
                    .unwrap_or("Unknown expense type"),
            ));
            html.push_str("</p>");
            if let Some(remarks) = item.remarks.as_deref() {
                html.push_str("<p class=\"field-note\">");
                html.push_str(&escape_html(remarks));
                html.push_str("</p>");
            }
            if !item.filenames.is_empty() {
                html.push_str("<ul class=\"attachment-files\">");
                for filename in &item.filenames {
                    html.push_str("<li>");
                    html.push_str(&escape_html(filename));
                    html.push_str("</li>");
                }
                html.push_str("</ul>");
            }
            html.push_str("</article>");
        }
        html.push_str("</div>");
    }
    html.push_str("</section>");
}

fn render_documents_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"panel\">");
    html.push_str("<p class=\"eyebrow\">Source Documents</p><h2>Uploaded Evidence</h2>");
    if packet.document_snapshots.is_empty() {
        html.push_str(
            "<p class=\"hero-note\">No uploaded document snapshots are available yet.</p>",
        );
    } else {
        html.push_str("<div class=\"document-grid\">");
        for document in &packet.document_snapshots {
            html.push_str("<article class=\"document-card\">");
            html.push_str("<div class=\"field-topline\"><div><h3>");
            html.push_str(&escape_html(&document.filename));
            html.push_str("</h3><p class=\"field-note\">");
            html.push_str(&escape_html(&friendly_document_kind(&document.kind)));
            html.push_str(" · ");
            html.push_str(&escape_html(&friendly_document_status(
                &document.status_label,
            )));
            html.push_str("</p></div><span class=\"status-pill ");
            html.push_str(if document.projected_to_filing {
                "ready"
            } else if document.used_in_bundle {
                "attention"
            } else {
                "review"
            });
            html.push_str("\">");
            html.push_str(if document.projected_to_filing {
                "Used in report"
            } else if document.used_in_bundle {
                "Used to prepare report"
            } else {
                "Captured from upload"
            });
            html.push_str("</span></div>");
            for field in &document.summary_fields {
                html.push_str("<div class=\"document-field\"><p class=\"document-field-label\">");
                html.push_str(&escape_html(&field.label));
                html.push_str("</p><p class=\"document-field-value\">");
                html.push_str(&escape_html(&field.value));
                html.push_str("</p></div>");
            }
            html.push_str(
                "<div class=\"document-actions\"><a class=\"document-link\" href=\"document/",
            );
            html.push_str(&escape_html_attribute(&document.document_id));
            html.push('/');
            html.push_str(&escape_html_attribute(&document.filename));
            html.push_str(
                "\" target=\"_blank\" rel=\"noreferrer noopener\">Open uploaded document</a></div>",
            );
            html.push_str("</article>");
        }
        html.push_str("</div>");
    }
    html.push_str("</section>");
}

fn summary_card(html: &mut String, label: &str, value: &str, present: bool) {
    html.push_str("<article class=\"summary-card\"><p class=\"summary-label\">");
    html.push_str(&escape_html(label));
    html.push_str("</p><p class=\"summary-value");
    if !present {
        html.push_str(" placeholder");
    }
    html.push_str("\">");
    html.push_str(&escape_html(value));
    html.push_str("</p></article>");
}

fn field_status_label(field: &CopyField) -> &'static str {
    if !field.present && field.required {
        "Missing"
    } else if field.needs_review {
        "Check this field"
    } else if field.entry_mode.is_readonly() {
        "Computed"
    } else {
        "Ready"
    }
}

fn field_status_class(field: &CopyField) -> &'static str {
    if !field.present && field.required {
        "blocked"
    } else if field.needs_review {
        "attention"
    } else if field.entry_mode.is_readonly() {
        "review"
    } else {
        "ready"
    }
}

fn issue_label(class: crate::ReadinessIssueClass) -> &'static str {
    match class {
        crate::ReadinessIssueClass::AutomationGap => "Needs another source",
        crate::ReadinessIssueClass::UserInputRequired => "Needs your input",
        crate::ReadinessIssueClass::ManualReview => "Check this field",
        crate::ReadinessIssueClass::OtherWarning => "Warning",
    }
}

fn issue_class_name(class: crate::ReadinessIssueClass) -> &'static str {
    match class {
        crate::ReadinessIssueClass::AutomationGap => "blocked",
        crate::ReadinessIssueClass::UserInputRequired => "attention",
        crate::ReadinessIssueClass::ManualReview => "review",
        crate::ReadinessIssueClass::OtherWarning => "review",
    }
}

fn filing_status_label(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "action required",
        FilingStatus::UserInputRequired => "needs your input",
        FilingStatus::ManualReviewRequired => "ready for review",
        FilingStatus::ReadyToFile => "ready to file",
    }
}

fn friendly_document_status(raw: &str) -> String {
    match raw {
        "ocr captured, not yet supported" | "ocr captured, not projected" => {
            "Document scanned".to_owned()
        }
        "parsed for bundle context only" => "Used to prepare report".to_owned(),
        other => other.to_owned(),
    }
}

fn friendly_document_kind(kind: &str) -> String {
    kind.replace('_', " ")
}

fn evidence_document_links(evidence: &[crate::draft::EvidenceReference]) -> Vec<(String, String)> {
    let mut links = Vec::new();
    for item in evidence {
        let Some(document_id) = item.document_id.as_deref() else {
            continue;
        };
        let filename = item.filename.as_deref().unwrap_or(document_id).to_owned();
        let link = (document_id.to_owned(), filename);
        if !links.contains(&link) {
            links.push(link);
        }
    }
    links
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
    use super::render_review_preview_html;
    use crate::bundle_synthesis::synthesize_bundle_projection_with_fx;
    use crate::review_packet::build_review_packet;
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};
    use crate::StaticFxRateProvider;

    fn synthetic_packet() -> crate::ReviewPacket {
        let documents = generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect::<Vec<_>>();
        let projection =
            synthesize_bundle_projection_with_fx(&documents, &StaticFxRateProvider::demo());
        build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build")
    }

    #[test]
    fn review_preview_is_read_only_and_print_ready() {
        let rendered = render_review_preview_html(&synthetic_packet());
        assert!(rendered.contains("Print / Save PDF"));
        assert!(rendered.contains("@media print"));
        assert!(rendered.contains("window.print()"));
        assert!(!rendered.contains("<input"));
        assert!(!rendered.contains("<textarea"));
        assert!(!rendered.contains("<select"));
    }

    #[test]
    fn review_preview_contains_sections_attachments_and_documents() {
        let rendered = render_review_preview_html(&synthetic_packet());
        assert!(rendered.contains("Attachment Checklist"));
        assert!(rendered.contains("Uploaded Evidence"));
        assert!(rendered.contains("Open uploaded document"));
        assert!(rendered.contains("General Information"));
    }

    #[test]
    fn review_preview_omits_developer_and_schema_details() {
        let rendered = render_review_preview_html(&synthetic_packet());
        assert!(!rendered.contains("Developer tools"));
        assert!(!rendered.contains("Source tier:"));
        assert!(!rendered.contains("class=\"field-path\""));
        assert!(!rendered.contains("expense_report."));
    }
}
