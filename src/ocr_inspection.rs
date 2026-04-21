use crate::document_extract::extract_document_facts;
use crate::document_facts::{DocumentFactsPayload, ExtractedDocumentFacts};
use crate::draft::ConfidenceLevel;
use crate::ocr_compare::{OcrComparisonResult, OcrComparisonStatus};
use crate::ocr_grounding::DocumentOcrGroundingSummary;
use crate::transcribe::TranscribedDocument;

pub fn render_ocr_inspection_html(
    passes: &[&TranscribedDocument],
    comparison: Option<&OcrComparisonResult>,
    grounding: Option<&DocumentOcrGroundingSummary>,
    source_href: Option<&str>,
    comparison_href: Option<&str>,
    grounding_href: Option<&str>,
) -> String {
    let primary = passes
        .first()
        .expect("ocr inspection requires at least one transcribed pass");
    let primary_facts = extract_document_facts(primary);

    let mut html = String::new();
    html.push_str(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>OCR Inspection</title><style>\
         :root{color-scheme:light;font-family:ui-sans-serif,system-ui,sans-serif;}\
         body{margin:0;background:#f7f3eb;color:#231f1a;}\
         main{max-width:1240px;margin:0 auto;padding:32px 24px 48px;}\
         h1,h2,h3{margin:0 0 12px;}\
         p{margin:0 0 12px;}\
         .hero,.section,.pass-card,.field-card{background:#fffdf9;border:1px solid #ddcfbb;border-radius:18px;box-shadow:0 8px 24px rgba(86,61,35,.08);}\
         .hero,.section{padding:22px 24px;margin-bottom:18px;}\
         .hero-actions{display:flex;flex-wrap:wrap;gap:10px;margin-top:16px;}\
         .nav-link,.field-link{display:inline-flex;align-items:center;gap:6px;padding:8px 12px;border-radius:999px;border:1px solid #d6c5ac;background:#f7efe3;color:#5f3b1f;text-decoration:none;font-weight:700;}\
         .muted{color:#6f6258;}\
         .grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;}\
         .metric{padding:12px 14px;border-radius:14px;background:#f3ece0;border:1px solid #e1d3bf;}\
         .metric-label{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#8a5a2b;font-weight:700;}\
         .metric-value{margin-top:6px;font-size:20px;font-weight:700;}\
         .pass-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:16px;}\
         .pass-card,.field-card{padding:18px 20px;}\
         .pass-header{display:flex;justify-content:space-between;gap:16px;align-items:flex-start;}\
         .badge{display:inline-flex;align-items:center;gap:6px;padding:4px 10px;border-radius:999px;font-size:12px;font-weight:700;text-transform:uppercase;letter-spacing:.05em;border:1px solid transparent;}\
         .badge.high,.badge.consensus{background:#d8f0df;color:#195c31;}\
         .badge.medium,.badge.partialconsensus{background:#fff0c9;color:#7d5700;}\
         .badge.low,.badge.divergent,.badge.missing{background:#ffd7d2;color:#8f2414;}\
         .facts{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:10px;margin:14px 0;}\
         .fact{padding:10px 12px;border-radius:12px;background:#f8f3eb;border:1px solid #eadfce;}\
         .fact-label{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#8a5a2b;font-weight:700;}\
         .fact-value{margin-top:6px;font-weight:700;word-break:break-word;}\
         .preview{margin-top:14px;padding:14px;border-radius:14px;background:#221c16;color:#f4eee6;overflow:auto;white-space:pre-wrap;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px;line-height:1.5;max-height:320px;}\
         .field-grid{display:grid;gap:14px;}\
         .candidate-list,.grounding-list{margin:10px 0 0;padding:0;list-style:none;}\
         .candidate-list li,.grounding-list li{padding:8px 0;border-top:1px solid #eadfce;}\
         .candidate-list li:first-child,.grounding-list li:first-child{border-top:0;}\
         .reason{margin-top:10px;padding:10px 12px;border-radius:12px;background:#f8ede7;color:#7b3426;}\
         .section-heading{display:flex;justify-content:space-between;gap:16px;align-items:flex-start;margin-bottom:14px;}\
         .empty{color:#6f6258;line-height:1.6;}\
         @media (max-width: 900px){main{padding:24px 16px 36px;}}\
         </style></head><body><main>",
    );
    html.push_str("<section class=\"hero\"><p class=\"muted\">Developer OCR inspection</p><h1>");
    html.push_str(&escape_html(&primary.filename));
    html.push_str("</h1><p class=\"muted\">");
    html.push_str(&escape_html(&format!(
        "document {} · {} pass{}",
        primary.document_id,
        passes.len(),
        if passes.len() == 1 { "" } else { "es" }
    )));
    html.push_str("</p><div class=\"hero-actions\">");
    if let Some(href) = source_href {
        html.push_str("<a class=\"nav-link\" href=\"");
        html.push_str(&escape_html_attribute(href));
        html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open source document</a>");
    }
    if let Some(href) = comparison_href {
        html.push_str("<a class=\"nav-link\" href=\"");
        html.push_str(&escape_html_attribute(href));
        html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open OCR diff</a>");
    }
    if let Some(href) = grounding_href {
        html.push_str("<a class=\"nav-link\" href=\"");
        html.push_str(&escape_html_attribute(href));
        html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open grounded source</a>");
    }
    html.push_str("</div></section>");

    html.push_str("<section class=\"section\"><div class=\"section-heading\"><div><h2>Primary extraction</h2><p class=\"muted\">The current filing pipeline still follows the primary OCR pass. This card shows what it extracted before any FA intervention.</p></div></div><div class=\"grid\">");
    html.push_str(&metric_card(
        "kind",
        primary_facts.classification.kind.as_str(),
    ));
    html.push_str(&metric_card(
        "classification confidence",
        confidence_label(primary_facts.classification.confidence),
    ));
    html.push_str(&metric_card(
        "extraction status",
        &format!("{:?}", primary_facts.extraction_status).to_ascii_lowercase(),
    ));
    html.push_str(&metric_card(
        "issues",
        &primary_facts.issues.len().to_string(),
    ));
    html.push_str("</div>");
    render_primary_summary(&mut html, &primary_facts);
    html.push_str("</section>");

    html.push_str("<section class=\"section\"><div class=\"section-heading\"><div><h2>OCR passes</h2><p class=\"muted\">Each pass keeps its own OCR text, extractor outcome, and preprocess metadata so disagreements can be debugged without rerunning the receipt.</p></div></div><div class=\"pass-grid\">");
    for document in passes {
        render_pass_card(&mut html, document);
    }
    html.push_str("</div></section>");

    if let Some(comparison) = comparison {
        html.push_str("<section class=\"section\"><div class=\"section-heading\"><div><h2>Pass comparison</h2><p class=\"muted\">Field-level consensus drives the next OCR confidence layer. Divergent fields are the ones most likely to need manual review or better preprocessing.</p></div></div><div class=\"grid\">");
        html.push_str(&metric_card(
            "overall confidence",
            confidence_label(comparison.overall_confidence),
        ));
        html.push_str(&metric_card(
            "disagreement count",
            &comparison.disagreement_count.to_string(),
        ));
        html.push_str(&metric_card(
            "field count",
            &comparison.fields.len().to_string(),
        ));
        html.push_str("</div><div class=\"field-grid\">");
        for field in &comparison.fields {
            render_comparison_field(&mut html, field);
        }
        html.push_str("</div></section>");
    }

    if let Some(grounding) = grounding {
        html.push_str("<section class=\"section\"><div class=\"section-heading\"><div><h2>Grounded OCR</h2><p class=\"muted\">These localized regions are the current bridge from OCR text to visual proof. They are the foundation for field highlighting in the FA workbench.</p></div></div><div class=\"grid\">");
        html.push_str(&metric_card(
            "geometry source",
            grounding.geometry_source.as_str(),
        ));
        html.push_str(&metric_card(
            "geometry available",
            if grounding.geometry_available {
                "yes"
            } else {
                "no"
            },
        ));
        html.push_str(&metric_card(
            "region count",
            &grounding.regions.len().to_string(),
        ));
        html.push_str("</div>");
        if grounding.regions.is_empty() {
            html.push_str(
                "<p class=\"empty\">No grounded regions were captured for this document yet.</p>",
            );
        } else {
            html.push_str("<ul class=\"grounding-list\">");
            for region in &grounding.regions {
                html.push_str("<li><strong>");
                html.push_str(&escape_html(&region.region_id));
                html.push_str("</strong> <span class=\"muted\">(");
                html.push_str(&escape_html(region.kind.as_str()));
                html.push_str(" · page ");
                html.push_str(&region.page_number.to_string());
                html.push_str(")</span><br>");
                html.push_str(&escape_html(&region.text));
                html.push_str("</li>");
            }
            html.push_str("</ul>");
        }
        html.push_str("</section>");
    }

    html.push_str("</main></body></html>");
    html
}

fn render_primary_summary(html: &mut String, facts: &ExtractedDocumentFacts) {
    html.push_str("<div class=\"facts\">");
    match &facts.facts {
        DocumentFactsPayload::Receipt(receipt) => {
            fact_tile(
                html,
                "Merchant",
                receipt
                    .merchant_name
                    .as_ref()
                    .map(|value| value.value.as_str())
                    .unwrap_or("[missing]"),
            );
            fact_tile(
                html,
                "Date",
                receipt
                    .transaction_date
                    .as_ref()
                    .map(|value| value.value.as_str())
                    .unwrap_or("[missing]"),
            );
            let total = receipt
                .total_paid
                .as_ref()
                .map(|value| {
                    let currency = value.value.currency.as_deref().unwrap_or("?");
                    format!("{currency} {}", value.value.amount)
                })
                .unwrap_or_else(|| "[missing]".to_owned());
            fact_tile(html, "Total", &total);
            fact_tile(html, "Line items", &receipt.line_items.len().to_string());
        }
        _ => {
            fact_tile(html, "Document kind", facts.classification.kind.as_str());
            fact_tile(
                html,
                "Extraction status",
                &format!("{:?}", facts.extraction_status).to_ascii_lowercase(),
            );
        }
    }
    html.push_str("</div>");
    if facts.issues.is_empty() {
        html.push_str(
            "<p class=\"muted\">No extractor issues were recorded for the primary pass.</p>",
        );
    } else {
        html.push_str("<ul class=\"candidate-list\">");
        for issue in &facts.issues {
            html.push_str("<li>");
            html.push_str(&escape_html(&issue.message));
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
}

fn render_pass_card(html: &mut String, document: &TranscribedDocument) {
    let facts = extract_document_facts(document);
    html.push_str("<article class=\"pass-card\"><div class=\"pass-header\"><div><h3><code>");
    html.push_str(&escape_html(&document.metadata.pass_id));
    html.push_str("</code></h3><p class=\"muted\">");
    html.push_str(&escape_html(&format!(
        "{} · {} · geometry {}",
        document.metadata.pass_kind.as_str(),
        document.metadata.preprocess_variant.as_str(),
        if document.metadata.geometry_available {
            document.metadata.geometry_source.as_str()
        } else {
            "none"
        }
    )));
    html.push_str("</p></div><span class=\"badge ");
    html.push_str(confidence_label(facts.classification.confidence));
    html.push_str("\">");
    html.push_str(confidence_label(facts.classification.confidence));
    html.push_str("</span></div><div class=\"facts\">");
    fact_tile(html, "Kind", facts.classification.kind.as_str());
    fact_tile(
        html,
        "Status",
        &format!("{:?}", facts.extraction_status).to_ascii_lowercase(),
    );
    match &facts.facts {
        DocumentFactsPayload::Receipt(receipt) => {
            fact_tile(
                html,
                "Merchant",
                receipt
                    .merchant_name
                    .as_ref()
                    .map(|value| value.value.as_str())
                    .unwrap_or("[missing]"),
            );
            fact_tile(
                html,
                "Date",
                receipt
                    .transaction_date
                    .as_ref()
                    .map(|value| value.value.as_str())
                    .unwrap_or("[missing]"),
            );
            let total = receipt
                .total_paid
                .as_ref()
                .map(|value| {
                    let currency = value.value.currency.as_deref().unwrap_or("?");
                    format!("{currency} {}", value.value.amount)
                })
                .unwrap_or_else(|| "[missing]".to_owned());
            fact_tile(html, "Total", &total);
        }
        _ => {
            fact_tile(html, "Pages", &document.pages.len().to_string());
            fact_tile(html, "Issues", &facts.issues.len().to_string());
        }
    }
    html.push_str("</div><div class=\"preview\">");
    html.push_str(&escape_html(&render_page_preview(document)));
    html.push_str("</div></article>");
}

fn render_comparison_field(html: &mut String, field: &crate::ocr_compare::OcrFieldComparison) {
    html.push_str("<article class=\"field-card\"><div class=\"pass-header\"><div><h3><code>");
    html.push_str(&escape_html(&field.field));
    html.push_str("</code></h3><p class=\"muted\">Consensus value: ");
    html.push_str(&escape_html(
        field.consensus_value.as_deref().unwrap_or("[missing]"),
    ));
    html.push_str("</p></div><div><span class=\"badge ");
    html.push_str(status_class(field.status));
    html.push_str("\">");
    html.push_str(status_label(field.status));
    html.push_str("</span> <span class=\"badge ");
    html.push_str(confidence_label(field.confidence));
    html.push_str("\">");
    html.push_str(confidence_label(field.confidence));
    html.push_str("</span></div></div>");
    if let Some(reason) = field.disagreement_reason.as_deref() {
        html.push_str("<div class=\"reason\">");
        html.push_str(&escape_html(reason));
        html.push_str("</div>");
    }
    html.push_str("<ul class=\"candidate-list\">");
    for candidate in &field.candidates {
        html.push_str("<li><strong><code>");
        html.push_str(&escape_html(&candidate.pass_id));
        html.push_str("</code></strong> → ");
        html.push_str(&escape_html(
            candidate.value.as_deref().unwrap_or("[missing]"),
        ));
        html.push_str(" <span class=\"muted\">(");
        html.push_str(
            candidate
                .extractor_confidence
                .map(confidence_label)
                .unwrap_or("unknown"),
        );
        html.push_str(")</span></li>");
    }
    html.push_str("</ul></article>");
}

fn render_page_preview(document: &TranscribedDocument) -> String {
    document
        .pages
        .iter()
        .map(|page| format!("--- Page {} ---\n{}", page.page_number, page.text.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn fact_tile(html: &mut String, label: &str, value: &str) {
    html.push_str("<div class=\"fact\"><div class=\"fact-label\">");
    html.push_str(&escape_html(label));
    html.push_str("</div><div class=\"fact-value\">");
    html.push_str(&escape_html(value));
    html.push_str("</div></div>");
}

fn metric_card(label: &str, value: &str) -> String {
    format!(
        "<div class=\"metric\"><div class=\"metric-label\">{}</div><div class=\"metric-value\">{}</div></div>",
        escape_html(label),
        escape_html(value)
    )
}

fn confidence_label(value: ConfidenceLevel) -> &'static str {
    match value {
        ConfidenceLevel::High => "high",
        ConfidenceLevel::Medium => "medium",
        ConfidenceLevel::Low => "low",
    }
}

fn status_label(value: OcrComparisonStatus) -> &'static str {
    match value {
        OcrComparisonStatus::Consensus => "consensus",
        OcrComparisonStatus::PartialConsensus => "partial consensus",
        OcrComparisonStatus::Divergent => "divergent",
        OcrComparisonStatus::Missing => "missing",
    }
}

fn status_class(value: OcrComparisonStatus) -> &'static str {
    match value {
        OcrComparisonStatus::Consensus => "consensus",
        OcrComparisonStatus::PartialConsensus => "partialconsensus",
        OcrComparisonStatus::Divergent => "divergent",
        OcrComparisonStatus::Missing => "missing",
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn escape_html_attribute(value: &str) -> String {
    escape_html(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr_compare::compare_ocr_passes;
    use crate::ocr_grounding::{DocumentOcrGroundingSummary, GroundedRegionSummary};
    use crate::transcribe::{
        OcrBoundingBox, OcrGeometrySource, OcrPassKind, OcrPreprocessVariant, OcrRegionKind,
        PageDimensions, TranscribedPage, TranscribedRegion, TranscriptionEngine,
        TranscriptionMetadata,
    };
    use std::path::PathBuf;

    fn receipt_document(
        pass_id: &str,
        pass_kind: OcrPassKind,
        preprocess_variant: OcrPreprocessVariant,
        markdown: &str,
        geometry_available: bool,
    ) -> TranscribedDocument {
        TranscribedDocument {
            document_id: "receipt_demo".to_owned(),
            filename: "receipt.png".to_owned(),
            source_path: PathBuf::from("receipt.png"),
            engine: TranscriptionEngine::VertexGeminiSdk,
            metadata: TranscriptionMetadata {
                pass_id: pass_id.to_owned(),
                pass_kind,
                preprocess_variant,
                producer: "google_genai_sdk".to_owned(),
                model: Some("gemini-3-flash-preview".to_owned()),
                geometry_source: if geometry_available {
                    OcrGeometrySource::Gemini
                } else {
                    OcrGeometrySource::None
                },
                geometry_available,
            },
            pages: vec![TranscribedPage {
                page_number: 1,
                text: markdown.to_owned(),
                dimensions: Some(PageDimensions {
                    width: 1200,
                    height: 1800,
                }),
                regions: if geometry_available {
                    vec![TranscribedRegion {
                        region_id: "total_paid".to_owned(),
                        kind: OcrRegionKind::ValueCandidate,
                        text: "MYR 9.00".to_owned(),
                        bbox: Some(OcrBoundingBox {
                            left: 0.1,
                            top: 0.2,
                            width: 0.3,
                            height: 0.05,
                        }),
                    }]
                } else {
                    Vec::new()
                },
            }],
        }
    }

    #[test]
    fn inspection_html_renders_passes_comparison_and_grounding() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
            true,
        );
        let secondary = receipt_document(
            "receipt_table_focused_binarized",
            OcrPassKind::TableFocused,
            OcrPreprocessVariant::Binarized,
            "# Merchant Receipt\n- Merchant Name: BOOK TALK\n- Date: 25/12/2018\n- Total: MYR 90.00\n",
            false,
        );
        let comparison = compare_ocr_passes(&[primary.clone(), secondary.clone()]).unwrap();
        let grounding = DocumentOcrGroundingSummary {
            document_id: "receipt_demo".to_owned(),
            geometry_source: OcrGeometrySource::Gemini,
            geometry_available: true,
            preview_href: Some(
                "artifact/ocr_grounding/receipt_demo/grounded_preview.html".to_owned(),
            ),
            regions: vec![GroundedRegionSummary {
                region_id: "total_paid".to_owned(),
                page_number: 1,
                kind: OcrRegionKind::ValueCandidate,
                text: "MYR 9.00".to_owned(),
            }],
        };

        let rendered = render_ocr_inspection_html(
            &[&primary, &secondary],
            Some(&comparison),
            Some(&grounding),
            Some("../../../document/receipt_demo/receipt.png"),
            Some("../../ocr_pass_comparisons/receipt_demo/comparison.html"),
            Some("../../ocr_grounding/receipt_demo/grounded_preview.html"),
        );

        assert!(rendered.contains("Developer OCR inspection"));
        assert!(rendered.contains("receipt_primary_original"));
        assert!(rendered.contains("receipt_table_focused_binarized"));
        assert!(rendered.contains("Open OCR diff"));
        assert!(rendered.contains("Open grounded source"));
        assert!(rendered.contains("Open source document"));
        assert!(rendered.contains("OCR passes disagreed"));
        assert!(rendered.contains("MYR 90.00"));
        assert!(rendered.contains("total_paid"));
    }

    #[test]
    fn inspection_html_without_optional_artifacts_still_renders_primary_summary() {
        let primary = receipt_document(
            "receipt_primary_original",
            OcrPassKind::Primary,
            OcrPreprocessVariant::Original,
            "# Merchant Receipt\n- Merchant Name: Book Talk\n- Date: 25/12/2018\n- Total: MYR 9.00\n",
            false,
        );

        let rendered = render_ocr_inspection_html(&[&primary], None, None, None, None, None);

        assert!(rendered.contains("Primary extraction"));
        assert!(rendered.contains("Book Talk"));
        assert!(rendered.contains("--- Page 1 ---"));
        assert!(!rendered.contains("Open OCR diff"));
        assert!(!rendered.contains("Grounded OCR"));
    }
}
