use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ingest::{
    ingest_expense_documents, write_ingestion_artifacts, IngestionConfig, IngestionError,
    IngestionFxMode, IngestionPipelineResult, IngestionTranscriber,
};
use crate::ledger::LedgerState;
use crate::review_packet::FilingStatus;
use crate::transcribe::{
    render_transcribed_document_json_pretty, transcribe_document_path, TranscriptionError,
};

const BUNDLES_DIR: &str = "bundles";
const MANIFEST_FILENAME: &str = "bundle_manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleWorkspaceStage {
    Normalized,
    AutomationBlocked,
    UserInputRequired,
    ManualReviewRequired,
    ReadyToFile,
    Submitted,
    Accepted,
    Returned,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedDocumentArtifacts {
    pub document_manifest_path: String,
    pub page_count: u32,
    pub page_image_paths: Vec<String>,
    pub native_text_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDocumentRecord {
    pub document_id: String,
    pub content_sha256: String,
    pub original_filenames: Vec<String>,
    pub stored_filename: String,
    pub raw_path: String,
    pub media_type: String,
    pub byte_count: u64,
    pub uploaded_at_epoch_ms: u64,
    pub normalized: NormalizedDocumentArtifacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRunConfigSummary {
    pub engine: String,
    pub fx_mode: String,
    pub project_id: Option<String>,
    pub location: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRunRecord {
    pub run_id: String,
    pub created_at_epoch_ms: u64,
    pub output_dir: String,
    pub document_count: usize,
    pub filing_status: FilingStatus,
    pub ledger_state: LedgerState,
    pub config: WorkspaceRunConfigSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleWorkspaceManifest {
    pub bundle_id: String,
    pub user_id: Option<String>,
    pub created_at_epoch_ms: u64,
    pub updated_at_epoch_ms: u64,
    pub current_stage: BundleWorkspaceStage,
    pub latest_run_id: Option<String>,
    pub documents: Vec<WorkspaceDocumentRecord>,
    pub runs: Vec<WorkspaceRunRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageBundleConfig {
    pub bundle_id: Option<String>,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRunConfig {
    pub run_id: Option<String>,
    pub transcriber: IngestionTranscriber,
    pub fx_mode: IngestionFxMode,
}

#[derive(Debug)]
pub struct WorkspaceRunResult {
    pub manifest: BundleWorkspaceManifest,
    pub run: WorkspaceRunRecord,
    pub pipeline: IngestionPipelineResult,
}

#[derive(Debug)]
pub enum WorkspaceError {
    NoInputDocuments,
    BundleNotFound(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Ingestion(IngestionError),
    Transcription(TranscriptionError),
    UnsupportedFormat(String),
    MissingTool(&'static str),
    CommandFailed(String),
    InvalidCommandOutput(String),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputDocuments => write!(f, "at least one input document is required"),
            Self::BundleNotFound(bundle_id) => {
                write!(f, "bundle workspace was not found for {bundle_id}")
            }
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::Ingestion(err) => write!(f, "{err}"),
            Self::Transcription(err) => write!(f, "{err}"),
            Self::UnsupportedFormat(message) => write!(f, "{message}"),
            Self::MissingTool(tool) => write!(f, "required preprocessing tool is missing: {tool}"),
            Self::CommandFailed(message) => write!(f, "{message}"),
            Self::InvalidCommandOutput(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<std::io::Error> for WorkspaceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for WorkspaceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<IngestionError> for WorkspaceError {
    fn from(value: IngestionError) -> Self {
        Self::Ingestion(value)
    }
}

impl From<TranscriptionError> for WorkspaceError {
    fn from(value: TranscriptionError) -> Self {
        Self::Transcription(value)
    }
}

pub fn stage_bundle_uploads(
    workspace_root: impl AsRef<Path>,
    paths: &[PathBuf],
    config: &StageBundleConfig,
) -> Result<BundleWorkspaceManifest, WorkspaceError> {
    if paths.is_empty() {
        return Err(WorkspaceError::NoInputDocuments);
    }

    let workspace_root = workspace_root.as_ref();
    let bundle_id = config
        .bundle_id
        .clone()
        .unwrap_or_else(|| default_bundle_id(paths));
    let bundle_root = bundle_root(workspace_root, &bundle_id);
    fs::create_dir_all(bundle_root.join("uploads"))?;
    fs::create_dir_all(bundle_root.join("normalized"))?;
    fs::create_dir_all(bundle_root.join("runs"))?;

    let mut manifest = load_or_initialize_manifest(&bundle_root, &bundle_id, config.user_id.clone())?;

    for path in paths {
        let record = stage_document_into_bundle(&bundle_root, path, &manifest.documents)?;
        upsert_document_record(&mut manifest.documents, record);
    }

    manifest.updated_at_epoch_ms = now_epoch_millis();
    manifest.current_stage = BundleWorkspaceStage::Normalized;
    write_bundle_workspace_manifest(&bundle_root, &manifest)?;
    Ok(manifest)
}

pub fn run_staged_bundle(
    workspace_root: impl AsRef<Path>,
    bundle_id: &str,
    config: &WorkspaceRunConfig,
) -> Result<WorkspaceRunResult, WorkspaceError> {
    let workspace_root = workspace_root.as_ref();
    let bundle_root = bundle_root(workspace_root, bundle_id);
    if !bundle_root.exists() {
        return Err(WorkspaceError::BundleNotFound(bundle_id.to_owned()));
    }

    let mut manifest = load_bundle_workspace_manifest(workspace_root, bundle_id)?;
    if manifest.documents.is_empty() {
        return Err(WorkspaceError::NoInputDocuments);
    }

    let run_id = config.run_id.clone().unwrap_or_else(default_run_id);
    let run_root = bundle_root.join("runs").join(&run_id);
    let artifacts_dir = run_root.join("artifacts");
    fs::create_dir_all(&artifacts_dir)?;

    let raw_paths = manifest
        .documents
        .iter()
        .map(|document| bundle_root.join(&document.raw_path))
        .collect::<Vec<_>>();

    let pipeline = ingest_expense_documents(
        &raw_paths,
        &IngestionConfig {
            bundle_id: Some(bundle_id.to_owned()),
            transcriber: config.transcriber.clone(),
            fx_mode: config.fx_mode,
        },
    )?;
    write_ingestion_artifacts(&artifacts_dir, &pipeline)?;

    let run = WorkspaceRunRecord {
        run_id: run_id.clone(),
        created_at_epoch_ms: now_epoch_millis(),
        output_dir: relative_to_bundle_root(&bundle_root, &artifacts_dir),
        document_count: pipeline.transcriptions.len(),
        filing_status: pipeline.review_packet.summary.filing_status,
        ledger_state: pipeline.ledger.summary.current_state,
        config: summarize_run_config(config),
    };

    manifest.latest_run_id = Some(run_id);
    manifest.current_stage = stage_from_ledger_state(run.ledger_state);
    manifest.updated_at_epoch_ms = now_epoch_millis();
    manifest.runs.push(run.clone());
    write_bundle_workspace_manifest(&bundle_root, &manifest)?;

    Ok(WorkspaceRunResult {
        manifest,
        run,
        pipeline,
    })
}

pub fn stage_and_run_bundle(
    workspace_root: impl AsRef<Path>,
    paths: &[PathBuf],
    stage_config: &StageBundleConfig,
    run_config: &WorkspaceRunConfig,
) -> Result<WorkspaceRunResult, WorkspaceError> {
    let manifest = stage_bundle_uploads(workspace_root.as_ref(), paths, stage_config)?;
    run_staged_bundle(workspace_root, &manifest.bundle_id, run_config)
}

pub fn load_bundle_workspace_manifest(
    workspace_root: impl AsRef<Path>,
    bundle_id: &str,
) -> Result<BundleWorkspaceManifest, WorkspaceError> {
    let bundle_root = bundle_root(workspace_root.as_ref(), bundle_id);
    if !bundle_root.exists() {
        return Err(WorkspaceError::BundleNotFound(bundle_id.to_owned()));
    }
    let manifest_path = bundle_root.join(MANIFEST_FILENAME);
    let manifest = serde_json::from_str::<BundleWorkspaceManifest>(&fs::read_to_string(manifest_path)?)?;
    Ok(manifest)
}

pub fn render_bundle_workspace_manifest_json_pretty(
    manifest: &BundleWorkspaceManifest,
) -> Result<String, WorkspaceError> {
    Ok(serde_json::to_string_pretty(manifest)?)
}

fn load_or_initialize_manifest(
    bundle_root: &Path,
    bundle_id: &str,
    user_id: Option<String>,
) -> Result<BundleWorkspaceManifest, WorkspaceError> {
    let manifest_path = bundle_root.join(MANIFEST_FILENAME);
    if manifest_path.exists() {
        let mut manifest =
            serde_json::from_str::<BundleWorkspaceManifest>(&fs::read_to_string(&manifest_path)?)?;
        if manifest.user_id.is_none() && user_id.is_some() {
            manifest.user_id = user_id;
        }
        return Ok(manifest);
    }

    let now = now_epoch_millis();
    Ok(BundleWorkspaceManifest {
        bundle_id: bundle_id.to_owned(),
        user_id,
        created_at_epoch_ms: now,
        updated_at_epoch_ms: now,
        current_stage: BundleWorkspaceStage::Normalized,
        latest_run_id: None,
        documents: Vec::new(),
        runs: Vec::new(),
    })
}

fn stage_document_into_bundle(
    bundle_root: &Path,
    path: &Path,
    existing_documents: &[WorkspaceDocumentRecord],
) -> Result<WorkspaceDocumentRecord, WorkspaceError> {
    let content_sha256 = sha256_for_path(path)?;
    if let Some(existing) = existing_documents
        .iter()
        .find(|document| document.content_sha256 == content_sha256)
    {
        let mut updated = existing.clone();
        let original_name = file_name_for_path(path);
        if !updated.original_filenames.iter().any(|value| value == &original_name) {
            updated.original_filenames.push(original_name);
            updated.original_filenames.sort();
            updated.original_filenames.dedup();
        }
        return Ok(updated);
    }

    let document_id = document_id_for_path(path, &content_sha256);
    let original_name = file_name_for_path(path);
    let uploads_dir = bundle_root.join("uploads").join(&document_id);
    fs::create_dir_all(&uploads_dir)?;
    let stored_path = uploads_dir.join(&original_name);
    fs::copy(path, &stored_path)?;

    let byte_count = fs::metadata(&stored_path)?.len();
    let media_type = detect_media_type(path)?.to_owned();
    let normalized = normalize_stored_document(bundle_root, &document_id, &stored_path, &media_type)?;

    Ok(WorkspaceDocumentRecord {
        document_id,
        content_sha256,
        original_filenames: vec![original_name.clone()],
        stored_filename: original_name,
        raw_path: relative_to_bundle_root(bundle_root, &stored_path),
        media_type,
        byte_count,
        uploaded_at_epoch_ms: now_epoch_millis(),
        normalized,
    })
}

fn upsert_document_record(
    documents: &mut Vec<WorkspaceDocumentRecord>,
    record: WorkspaceDocumentRecord,
) {
    if let Some(existing) = documents
        .iter_mut()
        .find(|document| document.content_sha256 == record.content_sha256)
    {
        *existing = record;
    } else {
        documents.push(record);
        documents.sort_by(|left, right| left.document_id.cmp(&right.document_id));
    }
}

fn normalize_stored_document(
    bundle_root: &Path,
    document_id: &str,
    stored_path: &Path,
    media_type: &str,
) -> Result<NormalizedDocumentArtifacts, WorkspaceError> {
    let normalized_dir = bundle_root.join("normalized").join(document_id);
    let pages_dir = normalized_dir.join("pages");
    fs::create_dir_all(&pages_dir)?;

    let mut page_image_paths = Vec::new();
    let mut native_text_path = None;
    let extension = stored_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "pdf" => {
            render_pdf_pages_to_dir(stored_path, &pages_dir)?;
            page_image_paths = collect_page_image_paths(bundle_root, &pages_dir)?;
            native_text_path = write_native_text_artifact(bundle_root, &normalized_dir, stored_path)?;
        }
        "png" => {
            let output_path = pages_dir.join("page_0001.png");
            fs::copy(stored_path, &output_path)?;
            page_image_paths.push(relative_to_bundle_root(bundle_root, &output_path));
        }
        "jpg" | "jpeg" => {
            render_image_to_png(stored_path, pages_dir.join("page_0001.png"))?;
            page_image_paths.push(relative_to_bundle_root(
                bundle_root,
                &pages_dir.join("page_0001.png"),
            ));
        }
        "txt" | "md" | "markdown" => {
            native_text_path = write_native_text_artifact(bundle_root, &normalized_dir, stored_path)?;
        }
        _ => {
            return Err(WorkspaceError::UnsupportedFormat(format!(
                "unsupported upload format {:?}; expected pdf, png, jpg, jpeg, txt, md, or markdown",
                extension
            )))
        }
    }

    let page_count = page_image_paths.len() as u32;
    let manifest_payload = serde_json::json!({
        "document_id": document_id,
        "filename": file_name_for_path(stored_path),
        "media_type": media_type,
        "page_count": page_count,
        "page_image_paths": page_image_paths,
        "native_text_path": native_text_path,
    });
    let manifest_path = normalized_dir.join("document_manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest_payload)?)?;

    Ok(NormalizedDocumentArtifacts {
        document_manifest_path: relative_to_bundle_root(bundle_root, &manifest_path),
        page_count,
        page_image_paths,
        native_text_path,
    })
}

fn collect_page_image_paths(
    bundle_root: &Path,
    pages_dir: &Path,
) -> Result<Vec<String>, WorkspaceError> {
    let mut entries = fs::read_dir(pages_dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries
        .into_iter()
        .map(|path| relative_to_bundle_root(bundle_root, &path))
        .collect())
}

fn write_native_text_artifact(
    bundle_root: &Path,
    normalized_dir: &Path,
    stored_path: &Path,
) -> Result<Option<String>, WorkspaceError> {
    let transcribed = transcribe_document_path(stored_path)?;
    let output_path = normalized_dir.join("native_text.json");
    fs::write(
        &output_path,
        render_transcribed_document_json_pretty(&transcribed)?,
    )?;
    Ok(Some(relative_to_bundle_root(bundle_root, &output_path)))
}

fn render_pdf_pages_to_dir(path: &Path, pages_dir: &Path) -> Result<(), WorkspaceError> {
    ensure_tool_available("pdfinfo")?;
    ensure_tool_available("pdftoppm")?;
    let page_count = pdf_page_count(path)?;
    for page_number in 1..=page_count {
        let output_prefix = pages_dir.join(format!("page_{page_number:04}"));
        let output = Command::new("pdftoppm")
            .arg("-png")
            .arg("-f")
            .arg(page_number.to_string())
            .arg("-l")
            .arg(page_number.to_string())
            .arg("-singlefile")
            .arg(path)
            .arg(&output_prefix)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(WorkspaceError::CommandFailed(format!(
                "pdftoppm failed for page {page_number}: {stderr}"
            )));
        }
    }
    Ok(())
}

fn render_image_to_png(path: &Path, output_path: PathBuf) -> Result<(), WorkspaceError> {
    ensure_tool_available("sips")?;
    let output = Command::new("sips")
        .arg("-s")
        .arg("format")
        .arg("png")
        .arg(path)
        .arg("--out")
        .arg(&output_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorkspaceError::CommandFailed(format!(
            "sips failed to normalize image {}: {stderr}",
            path.display()
        )));
    }
    Ok(())
}

fn pdf_page_count(path: &Path) -> Result<u32, WorkspaceError> {
    let output = Command::new("pdfinfo").arg(path).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorkspaceError::CommandFailed(format!(
            "pdfinfo failed: {stderr}"
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("Pages:") {
            let page_count = value
                .trim()
                .parse::<u32>()
                .map_err(|_| {
                    WorkspaceError::InvalidCommandOutput(format!(
                        "failed to parse pdf page count from {:?}",
                        line
                    ))
                })?;
            if page_count > 0 {
                return Ok(page_count);
            }
        }
    }
    Err(WorkspaceError::InvalidCommandOutput(
        "pdfinfo output did not contain a positive page count".to_owned(),
    ))
}

fn sha256_for_path(path: &Path) -> Result<String, WorkspaceError> {
    ensure_tool_available("shasum")?;
    let output = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorkspaceError::CommandFailed(format!(
            "shasum failed for {}: {stderr}",
            path.display()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| {
            WorkspaceError::InvalidCommandOutput(format!(
                "shasum output was empty for {}",
                path.display()
            ))
        })?
        .to_owned();
    if value.len() == 64 {
        Ok(value)
    } else {
        Err(WorkspaceError::InvalidCommandOutput(format!(
            "unexpected sha256 output for {}: {stdout}",
            path.display()
        )))
    }
}

fn detect_media_type(path: &Path) -> Result<&'static str, WorkspaceError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => Ok("application/pdf"),
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "txt" => Ok("text/plain"),
        "md" | "markdown" => Ok("text/markdown"),
        _ => Err(WorkspaceError::UnsupportedFormat(format!(
            "unsupported upload format {:?}; expected pdf, png, jpg, jpeg, txt, md, or markdown",
            extension
        ))),
    }
}

fn summarize_run_config(config: &WorkspaceRunConfig) -> WorkspaceRunConfigSummary {
    match &config.transcriber {
        IngestionTranscriber::Builtin => WorkspaceRunConfigSummary {
            engine: "builtin".to_owned(),
            fx_mode: fx_mode_name(config.fx_mode).to_owned(),
            project_id: None,
            location: None,
            model: None,
        },
        IngestionTranscriber::VertexGemini(vertex) => WorkspaceRunConfigSummary {
            engine: "vertex-gemini".to_owned(),
            fx_mode: fx_mode_name(config.fx_mode).to_owned(),
            project_id: Some(vertex.project_id.clone()),
            location: Some(vertex.location.clone()),
            model: Some(vertex.model.clone()),
        },
        IngestionTranscriber::VertexGeminiSdk(vertex) => WorkspaceRunConfigSummary {
            engine: "vertex-gemini-sdk".to_owned(),
            fx_mode: fx_mode_name(config.fx_mode).to_owned(),
            project_id: vertex.project_id.clone(),
            location: Some(vertex.location.clone()),
            model: Some(vertex.model.clone()),
        },
    }
}

fn stage_from_ledger_state(state: LedgerState) -> BundleWorkspaceStage {
    match state {
        LedgerState::AutomationBlocked => BundleWorkspaceStage::AutomationBlocked,
        LedgerState::UserInputRequired => BundleWorkspaceStage::UserInputRequired,
        LedgerState::ManualReviewRequired => BundleWorkspaceStage::ManualReviewRequired,
        LedgerState::ReadyToFile => BundleWorkspaceStage::ReadyToFile,
        LedgerState::Submitted => BundleWorkspaceStage::Submitted,
        LedgerState::Accepted => BundleWorkspaceStage::Accepted,
        LedgerState::Returned => BundleWorkspaceStage::Returned,
        LedgerState::Rejected => BundleWorkspaceStage::Rejected,
    }
}

fn fx_mode_name(value: IngestionFxMode) -> &'static str {
    match value {
        IngestionFxMode::None => "none",
        IngestionFxMode::Demo => "demo",
    }
}

fn bundle_root(workspace_root: &Path, bundle_id: &str) -> PathBuf {
    workspace_root.join(BUNDLES_DIR).join(bundle_id)
}

fn write_bundle_workspace_manifest(
    bundle_root: &Path,
    manifest: &BundleWorkspaceManifest,
) -> Result<(), WorkspaceError> {
    fs::write(
        bundle_root.join(MANIFEST_FILENAME),
        serde_json::to_string_pretty(manifest)?,
    )?;
    Ok(())
}

fn relative_to_bundle_root(bundle_root: &Path, path: &Path) -> String {
    path.strip_prefix(bundle_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn ensure_tool_available(tool: &'static str) -> Result<(), WorkspaceError> {
    let output = Command::new("which").arg(tool).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WorkspaceError::MissingTool(tool))
    }
}

fn default_bundle_id(paths: &[PathBuf]) -> String {
    let stem = paths
        .first()
        .and_then(|path| path.file_stem())
        .and_then(|value| value.to_str())
        .unwrap_or("bundle");
    format!("{}_{}", sanitize_identifier(stem), now_epoch_millis())
}

fn default_run_id() -> String {
    format!("run_{}", now_epoch_millis())
}

fn document_id_for_path(path: &Path, sha256: &str) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    format!("{}_{}", sanitize_identifier(stem), &sha256[..12])
}

fn file_name_for_path(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("document")
        .to_owned()
}

fn sanitize_identifier(value: &str) -> String {
    let mut identifier = String::new();
    let mut previous_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            identifier.push(ch.to_ascii_lowercase());
            previous_separator = false;
        } else if !previous_separator {
            identifier.push('_');
            previous_separator = true;
        }
    }
    let identifier = identifier.trim_matches('_');
    if identifier.is_empty() {
        "document".to_owned()
    } else {
        identifier.to_owned()
    }
}

fn now_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be valid")
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("expense_report_schema_{label}_{unique}"));
        fs::create_dir_all(&dir).expect("temp dir should be creatable");
        dir
    }

    fn write_synthetic_packet(temp_root: &Path) -> Vec<PathBuf> {
        let input_dir = temp_root.join("input");
        fs::create_dir_all(&input_dir).expect("input dir should be creatable");
        generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| {
                let path = input_dir.join(&fixture.filename);
                fs::write(&path, fixture.markdown).expect("fixture should be writable");
                path
            })
            .collect()
    }

    #[test]
    fn stages_markdown_bundle_into_workspace() {
        let temp_root = unique_temp_dir("workspace_stage");
        let input_paths = write_synthetic_packet(&temp_root);

        let manifest = stage_bundle_uploads(
            &temp_root,
            &input_paths,
            &StageBundleConfig {
                bundle_id: Some("workspace_demo".to_owned()),
                user_id: Some("olivia.park".to_owned()),
            },
        )
        .expect("bundle staging should succeed");

        assert_eq!(manifest.bundle_id, "workspace_demo");
        assert_eq!(manifest.current_stage, BundleWorkspaceStage::Normalized);
        assert_eq!(manifest.documents.len(), 3);
        assert_eq!(manifest.user_id.as_deref(), Some("olivia.park"));
        for document in &manifest.documents {
            let bundle_root = bundle_root(&temp_root, &manifest.bundle_id);
            assert!(bundle_root.join(&document.raw_path).exists());
            let native_text_path = document
                .normalized
                .native_text_path
                .as_ref()
                .expect("markdown uploads should have native text");
            assert!(bundle_root.join(native_text_path).exists());
            assert_eq!(document.normalized.page_count, 0);
        }
    }

    #[test]
    fn staging_deduplicates_identical_files_by_hash() {
        let temp_root = unique_temp_dir("workspace_dedupe");
        let input_dir = temp_root.join("input");
        fs::create_dir_all(&input_dir).expect("input dir should exist");
        let first = input_dir.join("receipt_a.md");
        let second = input_dir.join("receipt_b.md");
        let content = "# Merchant Receipt\n\n- Merchant Name: Blue Bottle\n- Total: USD 12.40\n";
        fs::write(&first, content).expect("first file should be writable");
        fs::write(&second, content).expect("second file should be writable");

        let manifest = stage_bundle_uploads(
            &temp_root,
            &[first, second],
            &StageBundleConfig {
                bundle_id: Some("dedupe_demo".to_owned()),
                user_id: None,
            },
        )
        .expect("bundle staging should succeed");

        assert_eq!(manifest.documents.len(), 1);
        assert_eq!(
            manifest.documents[0].original_filenames,
            vec!["receipt_a.md".to_owned(), "receipt_b.md".to_owned()]
        );
    }

    #[test]
    fn runs_staged_bundle_and_persists_artifacts() {
        let temp_root = unique_temp_dir("workspace_run");
        let input_paths = write_synthetic_packet(&temp_root);
        let stage_manifest = stage_bundle_uploads(
            &temp_root,
            &input_paths,
            &StageBundleConfig {
                bundle_id: Some("run_demo".to_owned()),
                user_id: Some("olivia.park".to_owned()),
            },
        )
        .expect("bundle staging should succeed");

        let result = run_staged_bundle(
            &temp_root,
            &stage_manifest.bundle_id,
            &WorkspaceRunConfig {
                run_id: Some("baseline_builtin".to_owned()),
                transcriber: IngestionTranscriber::Builtin,
                fx_mode: IngestionFxMode::Demo,
            },
        )
        .expect("workspace run should succeed");

        assert_eq!(result.run.run_id, "baseline_builtin");
        assert_eq!(result.run.filing_status, FilingStatus::UserInputRequired);
        assert_eq!(result.run.ledger_state, LedgerState::UserInputRequired);
        assert_eq!(result.manifest.runs.len(), 1);
        assert_eq!(
            result.manifest.current_stage,
            BundleWorkspaceStage::UserInputRequired
        );

        let bundle_root = bundle_root(&temp_root, &stage_manifest.bundle_id);
        let artifacts_dir = bundle_root.join(&result.run.output_dir);
        assert!(artifacts_dir.join("bundle.json").exists());
        assert!(artifacts_dir.join("draft.yaml").exists());
        assert!(artifacts_dir.join("review_workbench.html").exists());
        assert!(artifacts_dir.join("ledger.json").exists());
    }

    #[test]
    fn stage_and_run_bundle_accumulates_run_history_without_restaging_docs() {
        let temp_root = unique_temp_dir("workspace_stage_run");
        let input_paths = write_synthetic_packet(&temp_root);

        let first = stage_and_run_bundle(
            &temp_root,
            &input_paths,
            &StageBundleConfig {
                bundle_id: Some("history_demo".to_owned()),
                user_id: None,
            },
            &WorkspaceRunConfig {
                run_id: Some("run_one".to_owned()),
                transcriber: IngestionTranscriber::Builtin,
                fx_mode: IngestionFxMode::Demo,
            },
        )
        .expect("first run should succeed");

        let second = run_staged_bundle(
            &temp_root,
            &first.manifest.bundle_id,
            &WorkspaceRunConfig {
                run_id: Some("run_two".to_owned()),
                transcriber: IngestionTranscriber::Builtin,
                fx_mode: IngestionFxMode::Demo,
            },
        )
        .expect("second run should succeed");

        assert_eq!(second.manifest.documents.len(), 3);
        assert_eq!(second.manifest.runs.len(), 2);
        assert_eq!(second.manifest.latest_run_id.as_deref(), Some("run_two"));
    }

    #[test]
    fn loads_bundle_manifest_from_disk_after_run() {
        let temp_root = unique_temp_dir("workspace_load");
        let input_paths = write_synthetic_packet(&temp_root);

        let result = stage_and_run_bundle(
            &temp_root,
            &input_paths,
            &StageBundleConfig {
                bundle_id: Some("load_demo".to_owned()),
                user_id: Some("olivia.park".to_owned()),
            },
            &WorkspaceRunConfig {
                run_id: Some("run_one".to_owned()),
                transcriber: IngestionTranscriber::Builtin,
                fx_mode: IngestionFxMode::Demo,
            },
        )
        .expect("stage-and-run should succeed");

        let reloaded = load_bundle_workspace_manifest(&temp_root, "load_demo")
            .expect("manifest should load from disk");

        assert_eq!(reloaded.bundle_id, result.manifest.bundle_id);
        assert_eq!(reloaded.latest_run_id, result.manifest.latest_run_id);
        assert_eq!(reloaded.documents.len(), 3);
        assert_eq!(reloaded.runs.len(), 1);
    }
}
