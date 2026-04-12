use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionEngine {
    PdfToText,
    PlainText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscribedPage {
    pub page_number: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscribedDocument {
    pub document_id: String,
    pub filename: String,
    pub source_path: PathBuf,
    pub engine: TranscriptionEngine,
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
                write!(f, "Unsupported document format {ext:?}; expected .pdf or .txt")
            }
            Self::MissingTool(tool) => write!(f, "Required transcription tool is missing: {tool}"),
            Self::CommandFailed(message) => write!(f, "Transcription command failed: {message}"),
            Self::InvalidUtf8(message) => write!(f, "Transcription output was not valid UTF-8: {message}"),
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

pub fn transcribe_document_path(path: impl AsRef<Path>) -> Result<TranscribedDocument, TranscriptionError> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "pdf" => transcribe_pdf(path),
        "txt" => transcribe_plain_text(path),
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
    Ok(serde_json::to_string_pretty(&transcribed_document_to_json_value(document))?)
}

pub fn transcribed_document_to_json_value(document: &TranscribedDocument) -> JsonValue {
    let mut root = JsonMap::new();
    root.insert(
        "document_id".to_owned(),
        JsonValue::String(document.document_id.clone()),
    );
    root.insert(
        "filename".to_owned(),
        JsonValue::String(document.filename.clone()),
    );
    root.insert(
        "source_path".to_owned(),
        JsonValue::String(document.source_path.display().to_string()),
    );
    root.insert(
        "engine".to_owned(),
        JsonValue::String(transcription_engine_name(document.engine).to_owned()),
    );
    root.insert(
        "pages".to_owned(),
        JsonValue::Array(
            document
                .pages
                .iter()
                .map(|page| {
                    let mut entry = JsonMap::new();
                    entry.insert(
                        "page_number".to_owned(),
                        JsonValue::Number(page.page_number.into()),
                    );
                    entry.insert("text".to_owned(), JsonValue::String(page.text.clone()));
                    JsonValue::Object(entry)
                })
                .collect(),
        ),
    );
    JsonValue::Object(root)
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
        pages: vec![TranscribedPage {
            page_number: 1,
            text,
        }],
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
        .map(|(index, page)| TranscribedPage {
            page_number: index as u32 + 1,
            text: page.to_owned(),
        })
        .collect()
}

fn transcription_engine_name(value: TranscriptionEngine) -> &'static str {
    match value {
        TranscriptionEngine::PdfToText => "pdftotext",
        TranscriptionEngine::PlainText => "plain_text",
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
        assert_eq!(sanitize_identifier("ER5499574_Redacted"), "er5499574_redacted");
        assert_eq!(sanitize_identifier(" weird---name "), "weird_name");
    }

    #[test]
    fn renders_markdown_with_stable_page_headers() {
        let document = TranscribedDocument {
            document_id: "sample".to_owned(),
            filename: "sample.pdf".to_owned(),
            source_path: PathBuf::from("/tmp/sample.pdf"),
            engine: TranscriptionEngine::PdfToText,
            pages: vec![
                TranscribedPage {
                    page_number: 1,
                    text: "first page".to_owned(),
                },
                TranscribedPage {
                    page_number: 2,
                    text: "second page".to_owned(),
                },
            ],
        };

        let markdown = render_transcribed_document_markdown(&document);
        assert!(markdown.contains("# Transcribed Document"));
        assert!(markdown.contains("## Page 1"));
        assert!(markdown.contains("## Page 2"));
        assert!(markdown.contains("```text\nfirst page\n```"));
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
