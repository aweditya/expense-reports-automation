use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::transcribe::{TranscribedDocument, TranscribedPage, TranscriptionEngine};

const DEFAULT_GEMINI_MODEL: &str = "gemini-3-flash-preview";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexGeminiSdkConfig {
    pub project_id: Option<String>,
    pub location: String,
    pub model: String,
    pub service_account_key_path: PathBuf,
    pub python_bin: PathBuf,
    pub script_path: PathBuf,
}

impl VertexGeminiSdkConfig {
    pub fn resolve_from_sources(
        project_id: Option<String>,
        location: Option<String>,
        model: Option<String>,
        service_account_key_path: Option<PathBuf>,
        python_bin: Option<PathBuf>,
        script_path: Option<PathBuf>,
    ) -> Result<Self, VertexGeminiSdkError> {
        let service_account_key_path = service_account_key_path
            .or_else(|| {
                std::env::var("VERTEX_SERVICE_ACCOUNT_KEY")
                    .ok()
                    .map(PathBuf::from)
            })
            .ok_or_else(|| {
                VertexGeminiSdkError::MissingConfiguration(
                    "VERTEX_SERVICE_ACCOUNT_KEY or --service-account-key".to_owned(),
                )
            })?;
        let location = location
            .or_else(|| std::env::var("VERTEX_LOCATION").ok())
            .unwrap_or_else(|| "global".to_owned());
        let model = model
            .or_else(|| std::env::var("VERTEX_GEMINI_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_owned());
        let project_id = project_id.or_else(|| std::env::var("VERTEX_PROJECT_ID").ok());
        let python_bin = python_bin
            .or_else(|| {
                std::env::var("VERTEX_GEMINI_SDK_PYTHON")
                    .ok()
                    .map(PathBuf::from)
            })
            .unwrap_or_else(default_python_path);
        let script_path = script_path
            .or_else(|| {
                std::env::var("VERTEX_GEMINI_SDK_SCRIPT")
                    .ok()
                    .map(PathBuf::from)
            })
            .unwrap_or_else(default_script_path);

        Ok(Self {
            project_id,
            location,
            model,
            service_account_key_path,
            python_bin,
            script_path,
        })
    }
}

#[derive(Debug)]
pub enum VertexGeminiSdkError {
    Io(std::io::Error),
    Json(serde_json::Error),
    MissingConfiguration(String),
    CommandFailed(String),
    InvalidUtf8(String),
    InvalidResponse(String),
}

impl fmt::Display for VertexGeminiSdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::MissingConfiguration(message) => write!(f, "{message}"),
            Self::CommandFailed(message) => write!(f, "Gemini SDK transcription failed: {message}"),
            Self::InvalidUtf8(message) => write!(f, "invalid UTF-8 output: {message}"),
            Self::InvalidResponse(message) => write!(f, "invalid Gemini SDK response: {message}"),
        }
    }
}

impl std::error::Error for VertexGeminiSdkError {}

impl From<std::io::Error> for VertexGeminiSdkError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for VertexGeminiSdkError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Deserialize)]
struct RawTranscribedDocument {
    document_id: String,
    filename: String,
    source_path: String,
    pages: Vec<RawTranscribedPage>,
}

#[derive(Debug, Deserialize)]
struct RawTranscribedPage {
    page_number: u32,
    text: String,
}

pub fn transcribe_document_path_with_vertex_sdk(
    path: impl AsRef<Path>,
    config: &VertexGeminiSdkConfig,
) -> Result<TranscribedDocument, VertexGeminiSdkError> {
    let output = run_sdk_transcription_command(path.as_ref(), config)?;
    parse_sdk_transcribed_document(&output)
}

fn run_sdk_transcription_command(
    path: &Path,
    config: &VertexGeminiSdkConfig,
) -> Result<String, VertexGeminiSdkError> {
    let mut command = Command::new(&config.python_bin);
    command.arg(&config.script_path);
    command.arg("--service-account-key");
    command.arg(&config.service_account_key_path);
    command.arg("--location");
    command.arg(&config.location);
    command.arg("--model");
    command.arg(&config.model);
    if let Some(project_id) = &config.project_id {
        command.arg("--project");
        command.arg(project_id);
    }
    command.arg(path);

    let output = command.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(VertexGeminiSdkError::CommandFailed(stderr));
    }

    String::from_utf8(output.stdout)
        .map_err(|err| VertexGeminiSdkError::InvalidUtf8(err.to_string()))
}

fn parse_sdk_transcribed_document(text: &str) -> Result<TranscribedDocument, VertexGeminiSdkError> {
    let payload: RawTranscribedDocument = serde_json::from_str(text)?;
    if payload.pages.is_empty() {
        return Err(VertexGeminiSdkError::InvalidResponse(
            "transcribed document did not contain any pages".to_owned(),
        ));
    }

    Ok(TranscribedDocument {
        document_id: payload.document_id,
        filename: payload.filename,
        source_path: PathBuf::from(payload.source_path),
        engine: TranscriptionEngine::VertexGeminiSdk,
        pages: payload
            .pages
            .into_iter()
            .map(|page| TranscribedPage {
                page_number: page.page_number,
                text: page.text,
            })
            .collect(),
    })
}

fn default_python_path() -> PathBuf {
    let venv_python = PathBuf::from("/tmp/expense_report_genai_venv/bin/python");
    if venv_python.exists() {
        venv_python
    } else {
        PathBuf::from("python3")
    }
}

fn default_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/transcribe_with_google_genai.py")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sdk_transcriber_parses_mock_script_output() {
        let temp_dir = unique_temp_dir("vertex_sdk_ok");
        let script_path = temp_dir.join("mock_sdk.py");
        let key_path = temp_dir.join("service_account.json");
        let doc_path = temp_dir.join("receipt.png");
        fs::write(&key_path, "{}").expect("key should write");
        fs::write(&doc_path, b"png-bytes").expect("doc should write");
        fs::write(
            &script_path,
            r###"import json, pathlib, sys
doc = pathlib.Path(sys.argv[-1])
assert "--service-account-key" in sys.argv
assert "--location" in sys.argv
assert "--model" in sys.argv
print(json.dumps({
  "document_id": "mock_receipt",
  "filename": doc.name,
  "source_path": str(doc),
  "engine": "vertex_gemini_sdk",
  "pages": [{"page_number": 1, "text": "# Merchant Receipt\n\n- Merchant Name: Mock Bistro"}]
}))
"###,
        )
        .expect("mock script should write");

        let document = transcribe_document_path_with_vertex_sdk(
            &doc_path,
            &VertexGeminiSdkConfig {
                project_id: Some("demo-project".to_owned()),
                location: "global".to_owned(),
                model: "gemini-3-flash-preview".to_owned(),
                service_account_key_path: key_path,
                python_bin: PathBuf::from("python3"),
                script_path,
            },
        )
        .expect("sdk transcription should succeed");

        assert_eq!(document.engine, TranscriptionEngine::VertexGeminiSdk);
        assert_eq!(document.document_id, "mock_receipt");
        assert_eq!(document.pages.len(), 1);
        assert!(document.pages[0].text.contains("Mock Bistro"));
    }

    #[test]
    fn sdk_transcriber_surfaces_script_failures() {
        let temp_dir = unique_temp_dir("vertex_sdk_fail");
        let script_path = temp_dir.join("mock_fail.py");
        let key_path = temp_dir.join("service_account.json");
        let doc_path = temp_dir.join("receipt.png");
        fs::write(&key_path, "{}").expect("key should write");
        fs::write(&doc_path, b"png-bytes").expect("doc should write");
        fs::write(
            &script_path,
            "import sys\nsys.stderr.write('sdk failed intentionally\\n')\nsys.exit(2)\n",
        )
        .expect("mock script should write");

        let error = transcribe_document_path_with_vertex_sdk(
            &doc_path,
            &VertexGeminiSdkConfig {
                project_id: None,
                location: "global".to_owned(),
                model: "gemini-3-flash-preview".to_owned(),
                service_account_key_path: key_path,
                python_bin: PathBuf::from("python3"),
                script_path,
            },
        )
        .expect_err("sdk transcription should fail");

        assert!(error.to_string().contains("sdk failed intentionally"));
    }

    #[test]
    fn sdk_transcriber_rejects_empty_page_lists() {
        let error = parse_sdk_transcribed_document(
            r#"{"document_id":"x","filename":"x.png","source_path":"/tmp/x.png","pages":[]}"#,
        )
        .expect_err("empty pages should fail");
        assert!(error.to_string().contains("did not contain any pages"));
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("expense_report_schema_{prefix}_{unique}"));
        fs::create_dir_all(&path).expect("temp dir should create");
        path
    }
}
