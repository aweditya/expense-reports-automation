use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionEngine {
    PdfToText,
    PlainText,
    VertexGemini,
    VertexGeminiSdk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrPassKind {
    Primary,
    Verification,
    TableFocused,
    GeometryAssist,
}

impl OcrPassKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Verification => "verification",
            Self::TableFocused => "table_focused",
            Self::GeometryAssist => "geometry_assist",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrPreprocessVariant {
    Original,
    ContrastBoosted,
    Grayscale,
    Binarized,
    Deskewed,
}

impl OcrPreprocessVariant {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::ContrastBoosted => "contrast_boosted",
            Self::Grayscale => "grayscale",
            Self::Binarized => "binarized",
            Self::Deskewed => "deskewed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrGeometrySource {
    None,
    Gemini,
    DocumentAi,
    Hybrid,
}

impl OcrGeometrySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gemini => "gemini",
            Self::DocumentAi => "document_ai",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrRegionKind {
    Block,
    Line,
    Token,
    Table,
    TableRow,
    TableCell,
    LabelCandidate,
    ValueCandidate,
}

impl OcrRegionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Line => "line",
            Self::Token => "token",
            Self::Table => "table",
            Self::TableRow => "table_row",
            Self::TableCell => "table_cell",
            Self::LabelCandidate => "label_candidate",
            Self::ValueCandidate => "value_candidate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionMetadata {
    pub pass_id: String,
    pub pass_kind: OcrPassKind,
    pub preprocess_variant: OcrPreprocessVariant,
    pub producer: String,
    pub model: Option<String>,
    pub geometry_source: OcrGeometrySource,
    pub geometry_available: bool,
}

impl TranscriptionMetadata {
    pub fn primary_for_engine(
        engine: TranscriptionEngine,
        producer: impl Into<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            pass_id: format!("{}_primary_original", transcription_engine_name(engine)),
            pass_kind: OcrPassKind::Primary,
            preprocess_variant: OcrPreprocessVariant::Original,
            producer: producer.into(),
            model,
            geometry_source: OcrGeometrySource::None,
            geometry_available: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OcrBoundingBox {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscribedRegion {
    pub region_id: String,
    pub kind: OcrRegionKind,
    pub text: String,
    pub bbox: Option<OcrBoundingBox>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscribedPage {
    pub page_number: u32,
    pub text: String,
    pub dimensions: Option<PageDimensions>,
    pub regions: Vec<TranscribedRegion>,
}

impl TranscribedPage {
    pub fn text_only(page_number: u32, text: impl Into<String>) -> Self {
        Self {
            page_number,
            text: text.into(),
            dimensions: None,
            regions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscribedDocument {
    pub document_id: String,
    pub filename: String,
    pub source_path: PathBuf,
    pub engine: TranscriptionEngine,
    pub metadata: TranscriptionMetadata,
    pub pages: Vec<TranscribedPage>,
}

#[derive(Debug)]
pub enum TranscriptionError {
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedFormat(String),
    MissingTool(&'static str),
    CommandFailed(String),
    InvalidUtf8(String),
}

impl fmt::Display for TranscriptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON render error: {err}"),
            Self::UnsupportedFormat(ext) => {
                write!(
                    f,
                    "Unsupported document format {ext:?}; expected .pdf, .txt, .md, or .markdown"
                )
            }
            Self::MissingTool(tool) => write!(f, "Required transcription tool is missing: {tool}"),
            Self::CommandFailed(message) => write!(f, "Transcription command failed: {message}"),
            Self::InvalidUtf8(message) => {
                write!(f, "Transcription output was not valid UTF-8: {message}")
            }
        }
    }
}

impl std::error::Error for TranscriptionError {}

impl From<std::io::Error> for TranscriptionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for TranscriptionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn transcribe_document_path(
    path: impl AsRef<Path>,
) -> Result<TranscribedDocument, TranscriptionError> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "pdf" => transcribe_pdf(path),
        "txt" | "md" | "markdown" => transcribe_plain_text(path),
        _ => Err(TranscriptionError::UnsupportedFormat(extension)),
    }
}

pub fn render_transcribed_document_markdown(document: &TranscribedDocument) -> String {
    let mut rendered = String::new();
    rendered.push_str("# Transcribed Document\n\n");
    rendered.push_str(&format!("- document_id: {}\n", document.document_id));
    rendered.push_str(&format!("- filename: {}\n", document.filename));
    rendered.push_str(&format!(
        "- source_path: {}\n",
        document.source_path.display()
    ));
    rendered.push_str(&format!(
        "- engine: {}\n",
        transcription_engine_name(document.engine)
    ));
    rendered.push_str(&format!("- pass_id: {}\n", document.metadata.pass_id));
    rendered.push_str(&format!(
        "- pass_kind: {}\n",
        document.metadata.pass_kind.as_str()
    ));
    rendered.push_str(&format!(
        "- preprocess_variant: {}\n",
        document.metadata.preprocess_variant.as_str()
    ));
    rendered.push_str(&format!("- producer: {}\n", document.metadata.producer));
    if let Some(model) = document.metadata.model.as_deref() {
        rendered.push_str(&format!("- model: {}\n", model));
    }
    rendered.push_str(&format!(
        "- geometry_source: {}\n",
        document.metadata.geometry_source.as_str()
    ));
    rendered.push_str(&format!(
        "- geometry_available: {}\n",
        document.metadata.geometry_available
    ));

    for page in &document.pages {
        rendered.push_str(&format!("\n## Page {}\n\n", page.page_number));
        rendered.push_str("```text\n");
        rendered.push_str(&page.text);
        if !page.text.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str("```\n");
    }

    rendered
}

pub fn render_transcribed_document_json_pretty(
    document: &TranscribedDocument,
) -> Result<String, TranscriptionError> {
    Ok(serde_json::to_string_pretty(document)?)
}

pub fn transcribed_document_to_json_value(document: &TranscribedDocument) -> JsonValue {
    serde_json::to_value(document).expect("transcribed document should serialize")
}

pub fn parse_transcribed_document_json_str(
    value: &str,
) -> Result<TranscribedDocument, serde_json::Error> {
    serde_json::from_str(value)
}

pub fn parse_transcribed_document_json_path(
    path: impl AsRef<Path>,
) -> Result<TranscribedDocument, Box<dyn std::error::Error>> {
    let value = std::fs::read_to_string(path)?;
    Ok(parse_transcribed_document_json_str(&value)?)
}

fn transcribe_pdf(path: &Path) -> Result<TranscribedDocument, TranscriptionError> {
    ensure_tool_available("pdftotext")?;
    let output = Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TranscriptionError::CommandFailed(stderr.trim().to_owned()));
    }

    let text = String::from_utf8(output.stdout)
        .map_err(|err| TranscriptionError::InvalidUtf8(err.to_string()))?;

    Ok(TranscribedDocument {
        document_id: document_id_for_path(path),
        filename: file_name_for_path(path),
        source_path: path.to_path_buf(),
        engine: TranscriptionEngine::PdfToText,
        metadata: TranscriptionMetadata::primary_for_engine(
            TranscriptionEngine::PdfToText,
            "pdftotext",
            None,
        ),
        pages: split_pages(&text),
    })
}

fn transcribe_plain_text(path: &Path) -> Result<TranscribedDocument, TranscriptionError> {
    let text = fs::read_to_string(path)?;
    Ok(TranscribedDocument {
        document_id: document_id_for_path(path),
        filename: file_name_for_path(path),
        source_path: path.to_path_buf(),
        engine: TranscriptionEngine::PlainText,
        metadata: TranscriptionMetadata::primary_for_engine(
            TranscriptionEngine::PlainText,
            "plain_text",
            None,
        ),
        pages: vec![TranscribedPage::text_only(1, text)],
    })
}

fn ensure_tool_available(tool: &'static str) -> Result<(), TranscriptionError> {
    let status = Command::new("which").arg(tool).output()?;
    if status.status.success() {
        Ok(())
    } else {
        Err(TranscriptionError::MissingTool(tool))
    }
}

fn split_pages(text: &str) -> Vec<TranscribedPage> {
    let mut pages = text.split('\u{000C}').collect::<Vec<_>>();
    while matches!(pages.last(), Some(page) if page.trim().is_empty()) {
        pages.pop();
    }

    pages
        .into_iter()
        .map(str::trim)
        .enumerate()
        .map(|(index, page)| TranscribedPage::text_only(index as u32 + 1, page.to_owned()))
        .collect()
}

fn transcription_engine_name(value: TranscriptionEngine) -> &'static str {
    match value {
        TranscriptionEngine::PdfToText => "pdftotext",
        TranscriptionEngine::PlainText => "plain_text",
        TranscriptionEngine::VertexGemini => "vertex_gemini",
        TranscriptionEngine::VertexGeminiSdk => "vertex_gemini_sdk",
    }
}

fn document_id_for_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    sanitize_identifier(stem)
}

fn file_name_for_path(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("document")
        .to_owned()
}

fn sanitize_identifier(value: &str) -> String {
    let mut identifier = String::new();
    let mut previous_was_separator = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            identifier.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            identifier.push('_');
            previous_was_separator = true;
        }
    }

    let identifier = identifier.trim_matches('_');
    if identifier.is_empty() {
        "document".to_owned()
    } else {
        identifier.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as JsonValue;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn splits_form_feed_text_into_pages() {
        let pages = split_pages("page one\n\u{000C}\npage two\n\u{000C}");
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].page_number, 1);
        assert_eq!(pages[0].text, "page one");
        assert_eq!(pages[1].page_number, 2);
        assert_eq!(pages[1].text, "page two");
    }

    #[test]
    fn sanitizes_document_id_from_filename_stem() {
        assert_eq!(
            sanitize_identifier("ER5499574_Redacted"),
            "er5499574_redacted"
        );
        assert_eq!(sanitize_identifier(" weird---name "), "weird_name");
    }

    #[test]
    fn renders_markdown_with_stable_page_headers() {
        let document = TranscribedDocument {
            document_id: "sample".to_owned(),
            filename: "sample.pdf".to_owned(),
            source_path: PathBuf::from("/tmp/sample.pdf"),
            engine: TranscriptionEngine::VertexGemini,
            metadata: TranscriptionMetadata::primary_for_engine(
                TranscriptionEngine::VertexGemini,
                "vertex_gemini",
                Some("gemini-3-flash-preview".to_owned()),
            ),
            pages: vec![
                TranscribedPage::text_only(1, "first page"),
                TranscribedPage::text_only(2, "second page"),
            ],
        };

        let markdown = render_transcribed_document_markdown(&document);
        assert!(markdown.contains("# Transcribed Document"));
        assert!(markdown.contains("## Page 1"));
        assert!(markdown.contains("## Page 2"));
        assert!(markdown.contains("- pass_kind: primary"));
        assert!(markdown.contains("- preprocess_variant: original"));
        assert!(markdown.contains("```text\nfirst page\n```"));
    }

    #[test]
    fn json_render_includes_metadata_and_empty_geometry_slots() {
        let document = TranscribedDocument {
            document_id: "sample".to_owned(),
            filename: "sample.txt".to_owned(),
            source_path: PathBuf::from("fixtures/sample.txt"),
            engine: TranscriptionEngine::PlainText,
            metadata: TranscriptionMetadata::primary_for_engine(
                TranscriptionEngine::PlainText,
                "plain_text",
                None,
            ),
            pages: vec![TranscribedPage::text_only(1, "hello world")],
        };

        let rendered = transcribed_document_to_json_value(&document);
        assert_eq!(
            rendered
                .get("metadata")
                .and_then(|value| value.get("pass_kind"))
                .and_then(JsonValue::as_str),
            Some("primary")
        );
        assert_eq!(
            rendered
                .get("pages")
                .and_then(JsonValue::as_array)
                .and_then(|pages| pages.first())
                .and_then(|page| page.get("regions"))
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn transcribed_document_json_round_trips_with_metadata() {
        let document = TranscribedDocument {
            document_id: "sample".to_owned(),
            filename: "sample.txt".to_owned(),
            source_path: PathBuf::from("fixtures/sample.txt"),
            engine: TranscriptionEngine::VertexGeminiSdk,
            metadata: TranscriptionMetadata {
                pass_id: "sample_table_focused_binarized".to_owned(),
                pass_kind: OcrPassKind::TableFocused,
                preprocess_variant: OcrPreprocessVariant::Binarized,
                producer: "google_genai_sdk".to_owned(),
                model: Some("gemini-3-flash-preview".to_owned()),
                geometry_source: OcrGeometrySource::Gemini,
                geometry_available: true,
            },
            pages: vec![TranscribedPage {
                page_number: 1,
                text: "hello world".to_owned(),
                dimensions: Some(PageDimensions {
                    width: 1700,
                    height: 2200,
                }),
                regions: vec![TranscribedRegion {
                    region_id: "line_1".to_owned(),
                    kind: OcrRegionKind::Line,
                    text: "hello world".to_owned(),
                    bbox: Some(OcrBoundingBox {
                        left: 10.0,
                        top: 20.0,
                        width: 30.0,
                        height: 40.0,
                    }),
                }],
            }],
        };

        let rendered =
            render_transcribed_document_json_pretty(&document).expect("document should render");
        let parsed = parse_transcribed_document_json_str(&rendered).expect("document should parse");

        assert_eq!(parsed, document);
    }

    #[test]
    fn transcribes_plain_text_documents() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("expense_report_schema_{unique}.txt"));
        fs::write(&path, "hello from text file\n").expect("temp file should be writable");

        let document = transcribe_document_path(&path).expect("plain text should transcribe");
        assert_eq!(document.engine, TranscriptionEngine::PlainText);
        assert_eq!(document.pages.len(), 1);
        assert_eq!(document.pages[0].text, "hello from text file\n");

        fs::remove_file(path).expect("temp file should be removable");
    }
}
