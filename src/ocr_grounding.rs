use crate::transcribe::{
    OcrGeometrySource, OcrPreprocessVariant, OcrRegionKind, TranscribedDocument, TranscribedPage,
    TranscribedRegion,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedRegionSummary {
    pub region_id: String,
    pub page_number: u32,
    pub kind: OcrRegionKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentOcrGroundingSummary {
    pub document_id: String,
    pub geometry_source: OcrGeometrySource,
    pub geometry_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_preprocess_variant: Option<OcrPreprocessVariant>,
    pub preview_href: Option<String>,
    pub regions: Vec<GroundedRegionSummary>,
}

pub fn summarize_ocr_grounding(
    document: &TranscribedDocument,
    preview_href: Option<String>,
) -> DocumentOcrGroundingSummary {
    DocumentOcrGroundingSummary {
        document_id: document.document_id.clone(),
        geometry_source: document.metadata.geometry_source,
        geometry_available: document.metadata.geometry_available,
        grounding_preprocess_variant: document.metadata.grounding_preprocess_variant,
        preview_href,
        regions: document
            .pages
            .iter()
            .flat_map(|page| {
                page.regions
                    .iter()
                    .map(move |region| GroundedRegionSummary {
                        region_id: region.region_id.clone(),
                        page_number: page.page_number,
                        kind: region.kind,
                        text: region.text.clone(),
                    })
            })
            .collect(),
    }
}

pub fn render_ocr_grounding_html(
    document: &TranscribedDocument,
    image_href: Option<&str>,
) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str("<title>OCR Grounded Source Preview</title><style>");
    html.push_str(
        "body{font-family:ui-sans-serif,system-ui,sans-serif;margin:0;background:#f6f1e8;color:#221b16;}
        .shell{max-width:1200px;margin:0 auto;padding:24px;}
        .hero{display:flex;justify-content:space-between;gap:24px;align-items:flex-start;margin-bottom:24px;}
        .hero h1{margin:0;font-size:36px;}
        .meta{color:#6f6258;line-height:1.6;}
        .panel{background:rgba(255,255,255,.82);border:1px solid rgba(120,97,73,.18);border-radius:20px;padding:20px;box-shadow:0 12px 35px rgba(63,39,26,.08);}
        .layout{display:grid;grid-template-columns:minmax(0,2fr) minmax(300px,1fr);gap:20px;}
        .image-frame{position:relative;border-radius:16px;overflow:hidden;background:#fff;border:1px solid rgba(120,97,73,.16);}
        .image-frame img{display:block;width:100%;height:auto;}
        .region-box{position:absolute;border:2px solid #b86c3f;background:rgba(184,108,63,.12);border-radius:8px;box-sizing:border-box;}
        .region-box span{position:absolute;top:-24px;left:0;background:#241d18;color:#fff;padding:3px 8px;border-radius:999px;font-size:11px;letter-spacing:.04em;}
        .region-list{display:grid;gap:12px;}
        .region-card{border:1px solid rgba(120,97,73,.16);border-radius:14px;padding:14px;background:#fff;}
        .region-card:target{outline:3px solid rgba(184,108,63,.5);scroll-margin-top:24px;}
        .region-card h3{margin:0 0 8px;font-size:15px;}
        .badge{display:inline-block;padding:4px 10px;border-radius:999px;border:1px solid rgba(120,97,73,.16);font-size:12px;color:#6f6258;margin-right:6px;}
        .empty{color:#6f6258;line-height:1.6;}
        @media (max-width: 980px){.layout{grid-template-columns:1fr;}}
        ",
    );
    html.push_str("</style></head><body><div class=\"shell\">");
    html.push_str("<header class=\"hero\"><div><p class=\"meta\">OCR grounded preview</p><h1>");
    html.push_str(&escape_html(&document.filename));
    html.push_str("</h1><p class=\"meta\">");
    html.push_str(&escape_html(&format!(
        "geometry source: {} · regions: {}{}",
        geometry_source_label(document.metadata.geometry_source),
        document
            .pages
            .iter()
            .map(|page| page.regions.len())
            .sum::<usize>(),
        document
            .metadata
            .grounding_preprocess_variant
            .map(|variant| format!(" · grounded via {}", variant.as_str()))
            .unwrap_or_default()
    )));
    html.push_str("</p></div></header>");
    html.push_str("<div class=\"layout\">");
    html.push_str("<section class=\"panel\">");
    if let Some(image_href) = image_href {
        if let Some(first_page) = document.pages.first() {
            render_grounded_image_panel(&mut html, first_page, image_href);
        } else {
            html.push_str("<p class=\"empty\">No OCR pages were available.</p>");
        }
    } else {
        html.push_str("<p class=\"empty\">A source-image preview is not available for this document. The grounded regions are still listed on the right.</p>");
    }
    html.push_str("</section>");
    html.push_str("<aside class=\"panel\"><div class=\"region-list\">");
    let mut rendered_any = false;
    for page in &document.pages {
        for region in &page.regions {
            rendered_any = true;
            render_region_card(&mut html, page.page_number, region);
        }
    }
    if !rendered_any {
        html.push_str(
            "<p class=\"empty\">No grounded OCR regions were captured for this document yet.</p>",
        );
    }
    html.push_str("</div></aside></div></div></body></html>");
    html
}

pub fn match_quote_to_region_id<'a>(
    summary: &'a DocumentOcrGroundingSummary,
    quote: &str,
) -> Option<&'a str> {
    let normalized_quote = normalize_match_text(quote);
    if normalized_quote.is_empty() {
        return None;
    }

    summary
        .regions
        .iter()
        .filter_map(|region| {
            let normalized_region = normalize_match_text(&region.text);
            if normalized_region.is_empty() {
                return None;
            }
            if normalized_quote.contains(&normalized_region)
                || normalized_region.contains(&normalized_quote)
            {
                Some((region.region_id.as_str(), normalized_region.len()))
            } else {
                None
            }
        })
        .max_by_key(|(_, score)| *score)
        .map(|(region_id, _)| region_id)
}

fn render_grounded_image_panel(html: &mut String, page: &TranscribedPage, image_href: &str) {
    html.push_str("<div class=\"image-frame\">");
    html.push_str("<img src=\"");
    html.push_str(&escape_html_attribute(image_href));
    html.push_str("\" alt=\"Grounded OCR source image\">");
    for region in &page.regions {
        if let Some(bbox) = region.bbox {
            html.push_str("<a class=\"region-box\" href=\"#region-");
            html.push_str(&escape_html_attribute(&region.region_id));
            html.push_str("\" style=\"");
            html.push_str(&format!(
                "left:{:.4}%;top:{:.4}%;width:{:.4}%;height:{:.4}%;",
                bbox.left * 100.0,
                bbox.top * 100.0,
                bbox.width * 100.0,
                bbox.height * 100.0
            ));
            html.push_str("\"><span>");
            html.push_str(&escape_html(&region.region_id));
            html.push_str("</span></a>");
        }
    }
    html.push_str("</div>");
}

fn render_region_card(html: &mut String, page_number: u32, region: &TranscribedRegion) {
    html.push_str("<article class=\"region-card\" id=\"region-");
    html.push_str(&escape_html_attribute(&region.region_id));
    html.push_str("\">");
    html.push_str("<div><span class=\"badge\">");
    html.push_str(region.kind.as_str());
    html.push_str("</span><span class=\"badge\">page ");
    html.push_str(&page_number.to_string());
    html.push_str("</span></div>");
    html.push_str("<h3>");
    html.push_str(&escape_html(&region.region_id));
    html.push_str("</h3><p>");
    html.push_str(&escape_html(&region.text));
    html.push_str("</p></article>");
}

fn normalize_match_text(value: &str) -> String {
    value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(|char| char.to_lowercase())
        .collect()
}

fn geometry_source_label(source: OcrGeometrySource) -> &'static str {
    match source {
        OcrGeometrySource::None => "none",
        OcrGeometrySource::Gemini => "gemini",
        OcrGeometrySource::DocumentAi => "document ai",
        OcrGeometrySource::Hybrid => "hybrid",
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
    use super::*;
    use crate::transcribe::{
        OcrBoundingBox, OcrPassKind, OcrPreprocessVariant, PageDimensions, TranscribedPage,
        TranscribedRegion, TranscriptionEngine, TranscriptionMetadata,
    };
    use std::path::PathBuf;

    fn grounded_document() -> TranscribedDocument {
        TranscribedDocument {
            document_id: "receipt_demo".to_owned(),
            filename: "receipt.png".to_owned(),
            source_path: PathBuf::from("receipt.png"),
            engine: TranscriptionEngine::VertexGeminiSdk,
            metadata: TranscriptionMetadata {
                pass_id: "receipt_primary_original".to_owned(),
                pass_kind: OcrPassKind::Primary,
                preprocess_variant: OcrPreprocessVariant::Original,
                grounding_preprocess_variant: Some(OcrPreprocessVariant::Original),
                producer: "google_genai_sdk".to_owned(),
                model: Some("gemini-3-flash-preview".to_owned()),
                geometry_source: OcrGeometrySource::Gemini,
                geometry_available: true,
            },
            pages: vec![TranscribedPage {
                page_number: 1,
                text: "# Merchant Receipt".to_owned(),
                dimensions: Some(PageDimensions {
                    width: 1200,
                    height: 1800,
                }),
                regions: vec![
                    TranscribedRegion {
                        region_id: "merchant_name".to_owned(),
                        kind: OcrRegionKind::ValueCandidate,
                        text: "BOOK TALK".to_owned(),
                        bbox: Some(OcrBoundingBox {
                            left: 0.1,
                            top: 0.2,
                            width: 0.4,
                            height: 0.05,
                        }),
                    },
                    TranscribedRegion {
                        region_id: "total_paid".to_owned(),
                        kind: OcrRegionKind::ValueCandidate,
                        text: "MYR 80.90".to_owned(),
                        bbox: Some(OcrBoundingBox {
                            left: 0.52,
                            top: 0.72,
                            width: 0.24,
                            height: 0.05,
                        }),
                    },
                ],
            }],
        }
    }

    #[test]
    fn summarizes_grounded_document_regions() {
        let summary = summarize_ocr_grounding(
            &grounded_document(),
            Some("artifact/ocr_grounding/receipt_demo/grounded_preview.html".to_owned()),
        );

        assert!(summary.geometry_available);
        assert_eq!(
            summary.grounding_preprocess_variant,
            Some(OcrPreprocessVariant::Original)
        );
        assert_eq!(summary.regions.len(), 2);
        assert_eq!(
            summary.preview_href.as_deref(),
            Some("artifact/ocr_grounding/receipt_demo/grounded_preview.html")
        );
    }

    #[test]
    fn quote_matching_prefers_most_specific_region() {
        let summary = summarize_ocr_grounding(&grounded_document(), None);
        let region_id =
            match_quote_to_region_id(&summary, "Grand Total MYR 80.90 paid on card").unwrap();
        assert_eq!(region_id, "total_paid");
    }

    #[test]
    fn grounding_html_contains_overlay_and_region_links() {
        let rendered = render_ocr_grounding_html(&grounded_document(), Some("source.png"));
        assert!(rendered.contains("Grounded OCR source image"));
        assert!(rendered.contains("href=\"#region-total_paid\""));
        assert!(rendered.contains("source.png"));
        assert!(rendered.contains("BOOK TALK"));
    }
}
