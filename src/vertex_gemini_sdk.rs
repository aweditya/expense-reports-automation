use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::transcribe::{
    OcrBoundingBox, OcrGeometrySource, OcrPassKind, OcrPreprocessVariant, OcrRegionKind,
    PageDimensions, TranscribedDocument, TranscribedPage, TranscribedRegion, TranscriptionEngine,
    TranscriptionMetadata,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexGeminiSdkPassProfile {
    pub pass_id: Option<String>,
    pub pass_kind: OcrPassKind,
    pub preprocess_variant: OcrPreprocessVariant,
}

impl Default for VertexGeminiSdkPassProfile {
    fn default() -> Self {
        Self {
            pass_id: None,
            pass_kind: OcrPassKind::Primary,
            preprocess_variant: OcrPreprocessVariant::Original,
        }
    }
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
    metadata: Option<RawTranscriptionMetadata>,
    pages: Vec<RawTranscribedPage>,
}

#[derive(Debug, Deserialize)]
struct RawTranscribedPage {
    page_number: u32,
    text: String,
    dimensions: Option<RawPageDimensions>,
    #[serde(default)]
    regions: Vec<RawTranscribedRegion>,
}

#[derive(Debug, Deserialize)]
struct RawTranscriptionMetadata {
    pass_id: Option<String>,
    pass_kind: Option<String>,
    preprocess_variant: Option<String>,
    grounding_preprocess_variant: Option<String>,
    producer: Option<String>,
    model: Option<String>,
    geometry_source: Option<String>,
    geometry_available: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawPageDimensions {
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
struct RawTranscribedRegion {
    region_id: Option<String>,
    kind: Option<String>,
    text: String,
    bbox: Option<RawBoundingBox>,
}

#[derive(Debug, Deserialize)]
struct RawBoundingBox {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

pub fn transcribe_document_path_with_vertex_sdk(
    path: impl AsRef<Path>,
    config: &VertexGeminiSdkConfig,
) -> Result<TranscribedDocument, VertexGeminiSdkError> {
    transcribe_document_path_with_vertex_sdk_profile(
        path,
        config,
        &VertexGeminiSdkPassProfile::default(),
    )
}

pub fn transcribe_document_path_with_vertex_sdk_profile(
    path: impl AsRef<Path>,
    config: &VertexGeminiSdkConfig,
    profile: &VertexGeminiSdkPassProfile,
) -> Result<TranscribedDocument, VertexGeminiSdkError> {
    let output = run_sdk_transcription_command(path.as_ref(), config, profile)?;
    parse_sdk_transcribed_document(&output)
}

fn run_sdk_transcription_command(
    path: &Path,
    config: &VertexGeminiSdkConfig,
    profile: &VertexGeminiSdkPassProfile,
) -> Result<String, VertexGeminiSdkError> {
    let mut command = Command::new(&config.python_bin);
    command.arg(&config.script_path);
    command.arg("--service-account-key");
    command.arg(&config.service_account_key_path);
    command.arg("--location");
    command.arg(&config.location);
    command.arg("--model");
    command.arg(&config.model);
    command.arg("--pass-kind");
    command.arg(profile.pass_kind.as_str());
    command.arg("--preprocess-variant");
    command.arg(profile.preprocess_variant.as_str());
    if let Some(pass_id) = &profile.pass_id {
        command.arg("--pass-id");
        command.arg(pass_id);
    }
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
        metadata: payload.metadata.map_or_else(
            || {
                TranscriptionMetadata::primary_for_engine(
                    TranscriptionEngine::VertexGeminiSdk,
                    "google_genai_sdk",
                    None,
                )
            },
            parse_raw_transcription_metadata,
        ),
        pages: payload
            .pages
            .into_iter()
            .map(|page| {
                let page_number = page.page_number;
                let dimensions = page.dimensions.map(|dimensions| PageDimensions {
                    width: dimensions.width,
                    height: dimensions.height,
                });
                let regions = page
                    .regions
                    .into_iter()
                    .enumerate()
                    .map(|(index, region)| parse_raw_region(region, page_number, index))
                    .collect();
                TranscribedPage {
                    page_number,
                    text: page.text,
                    dimensions,
                    regions,
                }
            })
            .collect(),
    })
}

fn parse_raw_transcription_metadata(raw: RawTranscriptionMetadata) -> TranscriptionMetadata {
    TranscriptionMetadata {
        pass_id: raw
            .pass_id
            .unwrap_or_else(|| "vertex_gemini_sdk_primary_original".to_owned()),
        pass_kind: parse_pass_kind(raw.pass_kind.as_deref()),
        preprocess_variant: parse_preprocess_variant(raw.preprocess_variant.as_deref()),
        grounding_preprocess_variant: raw
            .grounding_preprocess_variant
            .as_deref()
            .map(|value| parse_preprocess_variant(Some(value))),
        producer: raw
            .producer
            .unwrap_or_else(|| "google_genai_sdk".to_owned()),
        model: raw.model,
        geometry_source: parse_geometry_source(raw.geometry_source.as_deref()),
        geometry_available: raw.geometry_available.unwrap_or(false),
    }
}

fn parse_raw_region(
    raw: RawTranscribedRegion,
    page_number: u32,
    index: usize,
) -> TranscribedRegion {
    TranscribedRegion {
        region_id: raw
            .region_id
            .unwrap_or_else(|| format!("page_{page_number}_region_{}", index + 1)),
        kind: parse_region_kind(raw.kind.as_deref()),
        text: raw.text,
        bbox: raw.bbox.map(|bbox| OcrBoundingBox {
            left: bbox.left,
            top: bbox.top,
            width: bbox.width,
            height: bbox.height,
        }),
    }
}

fn parse_pass_kind(value: Option<&str>) -> OcrPassKind {
    match value.unwrap_or("primary") {
        "verification" => OcrPassKind::Verification,
        "table_focused" => OcrPassKind::TableFocused,
        "geometry_assist" => OcrPassKind::GeometryAssist,
        _ => OcrPassKind::Primary,
    }
}

fn parse_preprocess_variant(value: Option<&str>) -> OcrPreprocessVariant {
    match value.unwrap_or("original") {
        "contrast_boosted" => OcrPreprocessVariant::ContrastBoosted,
        "grayscale" => OcrPreprocessVariant::Grayscale,
        "binarized" => OcrPreprocessVariant::Binarized,
        "deskewed" => OcrPreprocessVariant::Deskewed,
        _ => OcrPreprocessVariant::Original,
    }
}

fn parse_geometry_source(value: Option<&str>) -> OcrGeometrySource {
    match value.unwrap_or("none") {
        "gemini" => OcrGeometrySource::Gemini,
        "document_ai" => OcrGeometrySource::DocumentAi,
        "hybrid" => OcrGeometrySource::Hybrid,
        _ => OcrGeometrySource::None,
    }
}

fn parse_region_kind(value: Option<&str>) -> OcrRegionKind {
    match value.unwrap_or("line") {
        "block" => OcrRegionKind::Block,
        "token" => OcrRegionKind::Token,
        "table" => OcrRegionKind::Table,
        "table_row" => OcrRegionKind::TableRow,
        "table_cell" => OcrRegionKind::TableCell,
        "label_candidate" => OcrRegionKind::LabelCandidate,
        "value_candidate" => OcrRegionKind::ValueCandidate,
        _ => OcrRegionKind::Line,
    }
}

fn default_python_path() -> PathBuf {
    let venv_python = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".venv/bin/python");
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
  "metadata": {
    "pass_id": "receipt_primary_original",
    "pass_kind": "primary",
    "preprocess_variant": "contrast_boosted",
    "grounding_preprocess_variant": "binarized",
    "producer": "google_genai_sdk",
    "model": "gemini-3-flash-preview",
    "geometry_source": "gemini",
    "geometry_available": True
  },
  "pages": [{
    "page_number": 1,
    "text": "# Merchant Receipt\n\n- Merchant Name: Mock Bistro",
    "dimensions": {"width": 1024, "height": 1536},
    "regions": [{
      "region_id": "merchant_line",
      "kind": "line",
      "text": "Merchant Name: Mock Bistro",
      "bbox": {"left": 0.1, "top": 0.2, "width": 0.6, "height": 0.05}
    }]
  }]
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
        assert_eq!(document.metadata.pass_id, "receipt_primary_original");
        assert_eq!(
            document.metadata.preprocess_variant,
            OcrPreprocessVariant::ContrastBoosted
        );
        assert_eq!(
            document.metadata.grounding_preprocess_variant,
            Some(OcrPreprocessVariant::Binarized)
        );
        assert_eq!(document.metadata.geometry_source, OcrGeometrySource::Gemini);
        assert!(document.metadata.geometry_available);
        assert_eq!(
            document.pages[0].dimensions,
            Some(PageDimensions {
                width: 1024,
                height: 1536
            })
        );
        assert_eq!(document.pages[0].regions.len(), 1);
        assert!(document.pages[0].text.contains("Mock Bistro"));
    }

    #[test]
    fn sdk_transcriber_passes_non_default_profile_arguments() {
        let temp_dir = unique_temp_dir("vertex_sdk_profile");
        let script_path = temp_dir.join("mock_profile.py");
        let key_path = temp_dir.join("service_account.json");
        let doc_path = temp_dir.join("receipt.png");
        fs::write(&key_path, "{}").expect("key should write");
        fs::write(&doc_path, b"png-bytes").expect("doc should write");
        fs::write(
            &script_path,
            r###"import json, pathlib, sys
doc = pathlib.Path(sys.argv[-1])
assert sys.argv[sys.argv.index("--pass-kind") + 1] == "table_focused"
assert sys.argv[sys.argv.index("--preprocess-variant") + 1] == "binarized"
assert sys.argv[sys.argv.index("--pass-id") + 1] == "receipt_table_pass"
print(json.dumps({
  "document_id": "mock_receipt",
  "filename": doc.name,
  "source_path": str(doc),
  "pages": [{"page_number": 1, "text": "# Merchant Receipt"}]
}))
"###,
        )
        .expect("mock script should write");

        let document = transcribe_document_path_with_vertex_sdk_profile(
            &doc_path,
            &VertexGeminiSdkConfig {
                project_id: Some("demo-project".to_owned()),
                location: "global".to_owned(),
                model: "gemini-3-flash-preview".to_owned(),
                service_account_key_path: key_path,
                python_bin: PathBuf::from("python3"),
                script_path,
            },
            &VertexGeminiSdkPassProfile {
                pass_id: Some("receipt_table_pass".to_owned()),
                pass_kind: OcrPassKind::TableFocused,
                preprocess_variant: OcrPreprocessVariant::Binarized,
            },
        )
        .expect("profiled transcription should succeed");

        assert_eq!(document.pages.len(), 1);
        assert_eq!(document.metadata.pass_kind, OcrPassKind::Primary);
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
