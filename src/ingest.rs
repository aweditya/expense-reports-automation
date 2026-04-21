use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::bundle_synthesis::{
    render_canonical_bundle_json_pretty, synthesize_bundle_projection,
    synthesize_bundle_projection_with_fx, BundleProjectionResult, StaticFxRateProvider,
};
use crate::document_extract::extract_document_facts;
use crate::document_facts::{render_document_facts_json_pretty, DocumentKind};
use crate::ledger::{
    initialize_review_submission_ledger_with_ocr_artifacts,
    render_review_submission_ledger_json_pretty, ReviewSubmissionLedger,
};
use crate::ocr_compare::{
    compare_ocr_passes, render_ocr_comparison_html, render_ocr_comparison_json_pretty,
    render_ocr_comparison_markdown, summarize_ocr_comparison, OcrComparisonResult,
};
use crate::ocr_grounding::{
    render_ocr_grounding_html, summarize_ocr_grounding, DocumentOcrGroundingSummary,
};
use crate::readiness::{summarize_validation_readiness, ReadinessReport};
use crate::render::render_draft_report_yaml;
use crate::review_packet::{
    build_review_packet_with_ocr_artifacts, render_review_packet_json_pretty, ReviewPacket,
};
use crate::review_workbench::render_review_workbench_html;
use crate::transcribe::{
    render_transcribed_document_json_pretty, transcribe_document_path, TranscribedDocument,
};
use crate::validator::render_validation_report_json_pretty;
use crate::vertex_gemini::{
    resolve_access_token, transcribe_document_path_with_vertex, VertexGeminiConfig,
};
use crate::vertex_gemini_sdk::{
    transcribe_document_path_with_vertex_sdk, transcribe_document_path_with_vertex_sdk_profile,
    VertexGeminiSdkConfig, VertexGeminiSdkPassProfile,
};
use crate::ExtractedDocumentFacts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionFxMode {
    None,
    Demo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestionTranscriber {
    Builtin,
    VertexGemini(VertexGeminiConfig),
    VertexGeminiSdk(VertexGeminiSdkConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionConfig {
    pub bundle_id: Option<String>,
    pub transcriber: IngestionTranscriber,
    pub fx_mode: IngestionFxMode,
    pub compare_receipt_passes: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrPassComparisonArtifact {
    pub document_id: String,
    pub secondary_transcription: TranscribedDocument,
    pub comparison: OcrComparisonResult,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngestionPipelineResult {
    pub bundle_id: String,
    pub transcriptions: Vec<TranscribedDocument>,
    pub ocr_pass_comparisons: Vec<OcrPassComparisonArtifact>,
    pub ocr_groundings: Vec<DocumentOcrGroundingSummary>,
    pub extracted_documents: Vec<ExtractedDocumentFacts>,
    pub projection: BundleProjectionResult,
    pub readiness: ReadinessReport,
    pub review_packet: ReviewPacket,
    pub review_workbench_html: String,
    pub ledger: ReviewSubmissionLedger,
}

#[derive(Debug)]
pub enum IngestionError {
    NoInputDocuments,
    Transcription { path: PathBuf, message: String },
    OcrComparison { path: PathBuf, message: String },
    ReviewPacket(String),
    Ledger(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    RenderDraft(crate::render::RenderDraftReportError),
    RenderBundle(crate::bundle_synthesis::RenderCanonicalBundleError),
}

impl fmt::Display for IngestionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputDocuments => write!(f, "at least one input document is required"),
            Self::Transcription { path, message } => {
                write!(f, "failed to transcribe {}: {message}", path.display())
            }
            Self::OcrComparison { path, message } => {
                write!(
                    f,
                    "failed to compare OCR passes for {}: {message}",
                    path.display()
                )
            }
            Self::ReviewPacket(message) => write!(f, "{message}"),
            Self::Ledger(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::RenderDraft(err) => write!(f, "{err}"),
            Self::RenderBundle(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for IngestionError {}

impl From<std::io::Error> for IngestionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for IngestionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<crate::render::RenderDraftReportError> for IngestionError {
    fn from(value: crate::render::RenderDraftReportError) -> Self {
        Self::RenderDraft(value)
    }
}

impl From<crate::bundle_synthesis::RenderCanonicalBundleError> for IngestionError {
    fn from(value: crate::bundle_synthesis::RenderCanonicalBundleError) -> Self {
        Self::RenderBundle(value)
    }
}

pub fn ingest_expense_documents(
    paths: &[PathBuf],
    config: &IngestionConfig,
) -> Result<IngestionPipelineResult, IngestionError> {
    if paths.is_empty() {
        return Err(IngestionError::NoInputDocuments);
    }

    let mut transcriptions = Vec::new();
    let mut ocr_pass_comparisons = Vec::new();
    let mut ocr_groundings = Vec::new();
    let mut extracted_documents = Vec::new();
    let resolved_vertex_config = match &config.transcriber {
        IngestionTranscriber::VertexGemini(vertex_config) => {
            let mut resolved = vertex_config.clone();
            if resolved.access_token.is_none() {
                resolved.access_token =
                    Some(resolve_access_token(vertex_config).map_err(|err| {
                        transcription_error(Path::new("<vertex-auth>"), err.to_string())
                    })?);
            }
            Some(resolved)
        }
        IngestionTranscriber::Builtin | IngestionTranscriber::VertexGeminiSdk(_) => None,
    };

    for path in paths {
        let document = match &config.transcriber {
            IngestionTranscriber::Builtin => transcribe_document_path(path).map_err(
                |err: crate::transcribe::TranscriptionError| {
                    transcription_error(path, err.to_string())
                },
            )?,
            IngestionTranscriber::VertexGemini(_) => transcribe_document_path_with_vertex(
                path,
                resolved_vertex_config
                    .as_ref()
                    .expect("resolved vertex config should exist"),
            )
            .map_err(|err: crate::vertex_gemini::VertexGeminiError| {
                transcription_error(path, err.to_string())
            })?,
            IngestionTranscriber::VertexGeminiSdk(sdk_config) => {
                transcribe_document_path_with_vertex_sdk(path, sdk_config).map_err(
                    |err: crate::vertex_gemini_sdk::VertexGeminiSdkError| {
                        transcription_error(path, err.to_string())
                    },
                )?
            }
        };
        let facts = extract_document_facts(&document);
        if config.compare_receipt_passes
            && facts.classification.kind == DocumentKind::Receipt
            && matches!(
                &config.transcriber,
                IngestionTranscriber::VertexGeminiSdk(_)
            )
        {
            let sdk_config = match &config.transcriber {
                IngestionTranscriber::VertexGeminiSdk(sdk_config) => sdk_config,
                _ => unreachable!("checked above"),
            };
            let secondary_profile = VertexGeminiSdkPassProfile {
                pass_id: Some(format!("{}_table_focused_binarized", document.document_id)),
                pass_kind: crate::OcrPassKind::TableFocused,
                preprocess_variant: crate::OcrPreprocessVariant::Binarized,
            };
            let secondary_transcription = transcribe_document_path_with_vertex_sdk_profile(
                path,
                sdk_config,
                &secondary_profile,
            )
            .map_err(|err: crate::vertex_gemini_sdk::VertexGeminiSdkError| {
                transcription_error(path, err.to_string())
            })?;
            let comparison =
                compare_ocr_passes(&[document.clone(), secondary_transcription.clone()])
                    .map_err(|err| ocr_comparison_error(path, err.to_string()))?;
            ocr_pass_comparisons.push(OcrPassComparisonArtifact {
                document_id: document.document_id.clone(),
                secondary_transcription,
                comparison,
            });
        }
        if document.metadata.geometry_available {
            ocr_groundings.push(summarize_ocr_grounding(
                &document,
                Some(format!(
                    "artifact/ocr_grounding/{}/grounded_preview.html",
                    document.document_id
                )),
            ));
        }
        transcriptions.push(document);
        extracted_documents.push(facts);
    }

    let projection = match config.fx_mode {
        IngestionFxMode::None => synthesize_bundle_projection(&extracted_documents),
        IngestionFxMode::Demo => {
            let fx_provider = StaticFxRateProvider::demo();
            synthesize_bundle_projection_with_fx(&extracted_documents, &fx_provider)
        }
    };
    let readiness = summarize_validation_readiness(&projection.validation);
    let ocr_comparison_summaries = ocr_pass_comparisons
        .iter()
        .map(|artifact| summarize_ocr_comparison(&artifact.comparison))
        .collect::<Vec<_>>();
    let review_packet = build_review_packet_with_ocr_artifacts(
        &projection.bundle,
        &projection.draft,
        &readiness,
        &ocr_comparison_summaries,
        &ocr_groundings,
    )
    .map_err(|err| IngestionError::ReviewPacket(format!("failed to build review packet: {err}")))?;
    let review_workbench_html = render_review_workbench_html(&review_packet);

    let bundle_id = config
        .bundle_id
        .clone()
        .unwrap_or_else(|| default_bundle_id(paths));
    let ledger = initialize_review_submission_ledger_with_ocr_artifacts(
        &bundle_id,
        &projection.bundle,
        &projection.draft,
        &projection.validation,
        &ocr_comparison_summaries,
        &ocr_groundings,
    )
    .map_err(|err| {
        IngestionError::Ledger(format!(
            "failed to initialize review submission ledger: {err}"
        ))
    })?;

    Ok(IngestionPipelineResult {
        bundle_id,
        transcriptions,
        ocr_pass_comparisons,
        ocr_groundings,
        extracted_documents,
        projection,
        readiness,
        review_packet,
        review_workbench_html,
        ledger,
    })
}

pub fn write_ingestion_artifacts(
    output_dir: impl AsRef<Path>,
    result: &IngestionPipelineResult,
) -> Result<(), IngestionError> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir)?;
    let transcriptions_dir = output_dir.join("transcriptions");
    let facts_dir = output_dir.join("facts");
    let ocr_compare_dir = output_dir.join("ocr_pass_comparisons");
    let ocr_grounding_dir = output_dir.join("ocr_grounding");
    fs::create_dir_all(&transcriptions_dir)?;
    fs::create_dir_all(&facts_dir)?;
    fs::create_dir_all(&ocr_compare_dir)?;
    fs::create_dir_all(&ocr_grounding_dir)?;

    for document in &result.transcriptions {
        let filename = format!("{}.transcribed.json", document.document_id);
        fs::write(
            transcriptions_dir.join(filename),
            render_transcribed_document_json_pretty(document).map_err(|err| {
                IngestionError::ReviewPacket(format!("failed to render transcription json: {err}"))
            })?,
        )?;
    }

    for facts in &result.extracted_documents {
        let filename = format!("{}.facts.json", facts.document_id);
        fs::write(
            facts_dir.join(filename),
            render_document_facts_json_pretty(facts).map_err(|err| {
                IngestionError::ReviewPacket(format!("failed to render document facts json: {err}"))
            })?,
        )?;
    }

    for artifact in &result.ocr_pass_comparisons {
        let comparison_dir = ocr_compare_dir.join(&artifact.document_id);
        fs::create_dir_all(&comparison_dir)?;
        fs::write(
            comparison_dir.join("secondary.transcribed.json"),
            render_transcribed_document_json_pretty(&artifact.secondary_transcription).map_err(
                |err| {
                    IngestionError::ReviewPacket(format!(
                        "failed to render secondary transcription json: {err}"
                    ))
                },
            )?,
        )?;
        fs::write(
            comparison_dir.join("comparison.json"),
            render_ocr_comparison_json_pretty(&artifact.comparison)
                .map_err(IngestionError::Json)?,
        )?;
        fs::write(
            comparison_dir.join("comparison.md"),
            render_ocr_comparison_markdown(&artifact.comparison),
        )?;
        fs::write(
            comparison_dir.join("comparison.html"),
            render_ocr_comparison_html(&artifact.comparison),
        )?;
    }

    for document in &result.transcriptions {
        if !document.metadata.geometry_available {
            continue;
        }
        let grounding_dir = ocr_grounding_dir.join(&document.document_id);
        fs::create_dir_all(&grounding_dir)?;
        let image_href = match document
            .source_path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
        {
            Some("png" | "jpg" | "jpeg") => {
                let copied_source = grounding_dir.join(&document.filename);
                fs::copy(&document.source_path, &copied_source)?;
                Some(document.filename.as_str())
            }
            _ => None,
        };
        fs::write(
            grounding_dir.join("grounded_preview.html"),
            render_ocr_grounding_html(document, image_href),
        )?;
    }

    fs::write(
        output_dir.join("bundle.json"),
        render_canonical_bundle_json_pretty(&result.projection.bundle)?,
    )?;
    fs::write(
        output_dir.join("draft.yaml"),
        render_draft_report_yaml(&result.projection.draft)?,
    )?;
    fs::write(
        output_dir.join("validation.json"),
        render_validation_report_json_pretty(&result.projection.validation)
            .map_err(IngestionError::Json)?,
    )?;
    fs::write(
        output_dir.join("readiness.json"),
        serde_json::to_string_pretty(&result.readiness)?,
    )?;
    fs::write(
        output_dir.join("review_packet.json"),
        render_review_packet_json_pretty(&result.review_packet).map_err(|err| {
            IngestionError::ReviewPacket(format!("failed to render review packet json: {err}"))
        })?,
    )?;
    fs::write(
        output_dir.join("review_workbench.html"),
        &result.review_workbench_html,
    )?;
    fs::write(
        output_dir.join("ledger.json"),
        render_review_submission_ledger_json_pretty(&result.ledger)
            .map_err(IngestionError::Json)?,
    )?;

    let manifest = json!({
        "bundle_id": result.bundle_id,
        "document_count": result.transcriptions.len(),
        "filing_status": format!("{:?}", result.review_packet.summary.filing_status).to_ascii_lowercase(),
        "ledger_state": format!("{:?}", result.ledger.summary.current_state).to_ascii_lowercase(),
        "documents": result.transcriptions.iter().map(|document| json!({
            "document_id": document.document_id,
            "filename": document.filename,
            "engine": format!("{:?}", document.engine).to_ascii_lowercase(),
        })).collect::<Vec<_>>(),
        "ocr_pass_comparison_count": result.ocr_pass_comparisons.len(),
        "ocr_grounding_count": result.ocr_groundings.len(),
    });
    fs::write(
        output_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    Ok(())
}

fn transcription_error(path: &Path, message: String) -> IngestionError {
    IngestionError::Transcription {
        path: path.to_path_buf(),
        message,
    }
}

fn ocr_comparison_error(path: &Path, message: String) -> IngestionError {
    IngestionError::OcrComparison {
        path: path.to_path_buf(),
        message,
    }
}

fn default_bundle_id(paths: &[PathBuf]) -> String {
    let mut joined = String::new();
    for (index, path) in paths.iter().enumerate() {
        if index > 0 {
            joined.push('_');
        }
        joined.push_str(
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("document"),
        );
    }
    sanitize_identifier(&joined)
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
        "bundle".to_owned()
    } else {
        identifier.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};
    use crate::TranscriptionEngine;

    use super::*;

    #[test]
    fn ingests_synthetic_markdown_packet_with_builtin_transcriber() {
        let temp_dir = unique_temp_dir("ingest_builtin");
        let packet = generate_synthetic_packet(SyntheticVariant::Baseline);
        let paths = write_synthetic_packet(&temp_dir, &packet);

        let result = ingest_expense_documents(
            &paths,
            &IngestionConfig {
                bundle_id: Some("builtin_demo".to_owned()),
                transcriber: IngestionTranscriber::Builtin,
                fx_mode: IngestionFxMode::Demo,
                compare_receipt_passes: false,
            },
        )
        .expect("builtin ingestion should succeed");

        assert_eq!(result.transcriptions.len(), 3);
        assert_eq!(result.extracted_documents.len(), 3);
        assert_eq!(result.bundle_id, "builtin_demo");
        assert_eq!(result.ledger.draft_versions.len(), 1);
        assert!(result.review_workbench_html.contains("<!DOCTYPE html>"));
        assert_eq!(result.projection.bundle.expense_lines.len(), 3);
    }

    #[test]
    fn writes_ingestion_artifacts_to_output_dir() {
        let temp_dir = unique_temp_dir("ingest_artifacts");
        let output_dir = temp_dir.join("out");
        let packet = generate_synthetic_packet(SyntheticVariant::Baseline);
        let paths = write_synthetic_packet(&temp_dir, &packet);

        let result = ingest_expense_documents(
            &paths,
            &IngestionConfig {
                bundle_id: Some("artifact_demo".to_owned()),
                transcriber: IngestionTranscriber::Builtin,
                fx_mode: IngestionFxMode::Demo,
                compare_receipt_passes: false,
            },
        )
        .expect("builtin ingestion should succeed");
        write_ingestion_artifacts(&output_dir, &result).expect("artifact write should succeed");

        assert!(output_dir.join("bundle.json").exists());
        assert!(output_dir.join("draft.yaml").exists());
        assert!(output_dir.join("validation.json").exists());
        assert!(output_dir.join("review_packet.json").exists());
        assert!(output_dir.join("review_workbench.html").exists());
        assert!(output_dir.join("ledger.json").exists());
        assert!(output_dir.join("transcriptions").exists());
        assert!(output_dir.join("facts").exists());
    }

    #[test]
    fn ingests_mock_vertex_documents_end_to_end() {
        let temp_dir = unique_temp_dir("ingest_vertex");
        let packet = generate_synthetic_packet(SyntheticVariant::Baseline);
        let flight = packet[0].clone();
        let hotel = packet[1].clone();
        let receipt = packet[2].clone();

        let flight_path = temp_dir.join("flight.png");
        let hotel_path = temp_dir.join("hotel.png");
        let receipt_path = temp_dir.join("receipt.png");
        fs::write(&flight_path, b"flight-bytes").expect("temp png should write");
        fs::write(&hotel_path, b"hotel-bytes").expect("temp png should write");
        fs::write(&receipt_path, b"receipt-bytes").expect("temp png should write");

        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let requests_for_server = Arc::clone(&requests);
        let endpoint = spawn_mock_server(3, move |body| {
            requests_for_server.lock().unwrap().push(body.clone());
            let text = if body.contains("Filename: flight.png") {
                flight.markdown.clone()
            } else if body.contains("Filename: hotel.png") {
                hotel.markdown.clone()
            } else {
                receipt.markdown.clone()
            };
            http_ok(json!({
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {
                                    "text": serde_json::to_string(&json!({
                                        "pages": [{"page_number": 1, "text": text}]
                                    })).unwrap()
                                }
                            ]
                        }
                    }
                ]
            }))
        });

        let result = ingest_expense_documents(
            &[
                flight_path.clone(),
                hotel_path.clone(),
                receipt_path.clone(),
            ],
            &IngestionConfig {
                bundle_id: Some("vertex_demo".to_owned()),
                transcriber: IngestionTranscriber::VertexGemini(VertexGeminiConfig {
                    project_id: "demo-project".to_owned(),
                    location: "us-central1".to_owned(),
                    model: "gemini-3.1-flash-lite-preview".to_owned(),
                    access_token: Some("test-token".to_owned()),
                    service_account_key_path: None,
                    endpoint_override: Some(format!("{endpoint}/generate")),
                    token_endpoint_override: None,
                }),
                fx_mode: IngestionFxMode::Demo,
                compare_receipt_passes: false,
            },
        )
        .expect("vertex ingestion should succeed");

        assert_eq!(result.transcriptions.len(), 3);
        assert!(result
            .transcriptions
            .iter()
            .all(|document| document.engine == TranscriptionEngine::VertexGemini));
        assert_eq!(
            result.extracted_documents[0].classification.kind,
            flight.expected_facts.classification.kind
        );
        assert_eq!(
            result.extracted_documents[1].classification.kind,
            hotel.expected_facts.classification.kind
        );
        assert_eq!(
            result.extracted_documents[2].classification.kind,
            receipt.expected_facts.classification.kind
        );
        match &result.extracted_documents[0].facts {
            crate::DocumentFactsPayload::FlightItinerary(facts) => {
                assert_eq!(
                    facts
                        .traveler_names
                        .first()
                        .map(|value| value.value.as_str()),
                    Some("Olivia Park")
                );
            }
            other => panic!("expected flight itinerary facts, got {:?}", other.kind()),
        }
        match &result.extracted_documents[1].facts {
            crate::DocumentFactsPayload::HotelFolio(facts) => {
                assert_eq!(
                    facts.guest_name.as_ref().map(|value| value.value.as_str()),
                    Some("Olivia Park")
                );
            }
            other => panic!("expected hotel folio facts, got {:?}", other.kind()),
        }
        match &result.extracted_documents[2].facts {
            crate::DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .merchant_name
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("East Bay Bistro")
                );
            }
            other => panic!("expected receipt facts, got {:?}", other.kind()),
        }
        assert!(result.review_workbench_html.contains("<!DOCTYPE html>"));
        assert_eq!(result.ledger.summary.draft_version_count, 1);

        let bodies = requests.lock().unwrap().join("\n");
        assert!(bodies.contains("Filename: flight.png"));
        assert!(bodies.contains("Filename: hotel.png"));
        assert!(bodies.contains("Filename: receipt.png"));
    }

    #[test]
    fn ingests_vertex_documents_with_service_account_key_using_single_token_exchange() {
        let temp_dir = unique_temp_dir("ingest_vertex_service_account");
        let packet = generate_synthetic_packet(SyntheticVariant::Baseline);
        let flight = packet[0].clone();
        let hotel = packet[1].clone();
        let receipt = packet[2].clone();

        let flight_path = temp_dir.join("flight.png");
        let hotel_path = temp_dir.join("hotel.png");
        let receipt_path = temp_dir.join("receipt.png");
        let service_account_key_path = write_test_service_account_key(&temp_dir);
        fs::write(&flight_path, b"flight-bytes").expect("temp png should write");
        fs::write(&hotel_path, b"hotel-bytes").expect("temp png should write");
        fs::write(&receipt_path, b"receipt-bytes").expect("temp png should write");

        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let requests_for_server = Arc::clone(&requests);
        let base_url = spawn_mock_server(4, move |request| {
            requests_for_server.lock().unwrap().push(request.clone());
            if request.starts_with("POST /token ") {
                return http_ok(json!({
                    "access_token": "bundle-token",
                    "token_type": "Bearer",
                    "expires_in": 3600
                }));
            }

            let text = if request.contains("Filename: flight.png") {
                flight.markdown.clone()
            } else if request.contains("Filename: hotel.png") {
                hotel.markdown.clone()
            } else {
                receipt.markdown.clone()
            };
            http_ok(json!({
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {
                                    "text": serde_json::to_string(&json!({
                                        "pages": [{"page_number": 1, "text": text}]
                                    })).unwrap()
                                }
                            ]
                        }
                    }
                ]
            }))
        });

        let result = ingest_expense_documents(
            &[
                flight_path.clone(),
                hotel_path.clone(),
                receipt_path.clone(),
            ],
            &IngestionConfig {
                bundle_id: Some("vertex_service_account_demo".to_owned()),
                transcriber: IngestionTranscriber::VertexGemini(VertexGeminiConfig {
                    project_id: "demo-project".to_owned(),
                    location: "us-central1".to_owned(),
                    model: "gemini-3.1-flash-lite-preview".to_owned(),
                    access_token: None,
                    service_account_key_path: Some(service_account_key_path),
                    endpoint_override: Some(format!("{base_url}/generate")),
                    token_endpoint_override: Some(format!("{base_url}/token")),
                }),
                fx_mode: IngestionFxMode::Demo,
                compare_receipt_passes: false,
            },
        )
        .expect("vertex ingestion with service account should succeed");

        assert_eq!(result.transcriptions.len(), 3);
        assert!(result
            .transcriptions
            .iter()
            .all(|document| document.engine == TranscriptionEngine::VertexGemini));

        let captured = requests.lock().unwrap().clone();
        assert_eq!(
            captured
                .iter()
                .filter(|request| request.starts_with("POST /token "))
                .count(),
            1
        );
        assert_eq!(
            captured
                .iter()
                .filter(|request| request.starts_with("POST /generate "))
                .count(),
            3
        );
        assert!(captured
            .iter()
            .filter(|request| request.starts_with("POST /generate "))
            .all(|request| request.contains("Authorization: Bearer bundle-token")));
    }

    #[test]
    fn ingests_documents_with_sdk_transcriber_mock() {
        let temp_dir = unique_temp_dir("ingest_vertex_sdk");
        let packet = generate_synthetic_packet(SyntheticVariant::Baseline);
        let paths = write_synthetic_packet(&temp_dir, &packet);
        let script_path = temp_dir.join("mock_sdk.py");
        let key_path = temp_dir.join("service_account.json");
        fs::write(&key_path, "{}").expect("key should write");
        fs::write(
            &script_path,
            r###"import json, pathlib, sys
doc = pathlib.Path(sys.argv[-1])
stem = doc.stem
payloads = {
  "synthetic_flight_itinerary_baseline": "# E-Ticket Itinerary / Receipt\n\n## Passenger\n- Traveler Name: Olivia Park\n- Booking Reference: H7K9Q2\n- Ticket Number: 0162459135784\n- Booking Date: 2025-04-10\n- Airline: ANA All Nippon Airways\n- Fare Brand: Economy Basic\n- Baggage Allowance: 1 checked bag\n\n## Trip Summary\n- Origin: San Francisco, CA, United States (SFO)\n- Destination: Singapore (SIN)\n- Trip Window: 2025-04-21 to 2025-04-29\n- Total Paid: USD 1287.44\n\n## Segments\n- Segment 1 | Departure Airport: SFO | Arrival Airport: NRT | Departure Date: 2025-04-21 | Arrival Date: 2025-04-22 | Marketing Carrier: ANA | Flight Number: NH107 | Cabin Class: Economy\n- Segment 2 | Departure Airport: SIN | Arrival Airport: SFO | Departure Date: 2025-04-29 | Arrival Date: 2025-04-29 | Marketing Carrier: ANA | Flight Number: NH108 | Cabin Class: Economy",
  "synthetic_hotel_folio_baseline": "# Hotel Folio\n\n## Stay Summary\n- Property Name: Marina Bay Grand Hotel\n- Guest Name: Olivia Park\n- Folio Number: MBG-88421\n- Confirmation Number: SG88421\n- Room Number: 1814\n- Check-In: 2025-04-21\n- Check-Out: 2025-04-24\n- Property Location: Singapore, Singapore\n- Stay Window: 2025-04-21 to 2025-04-24\n- Total Paid: SGD 778.80\n\n## Nightly Charges\n- Date: 2025-04-21 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60\n- Date: 2025-04-22 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60\n- Date: 2025-04-23 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60\n\n## Meals Included\n- Breakfast\n- Evening Reception",
  "synthetic_receipt_baseline": "# Merchant Receipt\n\n## Purchase Summary\n- Merchant Name: East Bay Bistro\n- Merchant Location: Singapore, Singapore\n- Merchant Address: 18 Battery Road\n- Card: VISA •••• 4242\n- Authorization Code: A1189Q\n- Terminal ID: SG-TERM-07\n- Transaction Date: 2025-04-24\n- Subtotal: SGD 28.00\n- Tax: SGD 2.52\n- Tip: SGD 4.50\n- Total Paid: SGD 35.02\n\n## Line Items\n- Laksa Lunch | SGD 18.00\n- Iced Tea | SGD 6.00\n- Service Charge | SGD 4.00"
}
print(json.dumps({
  "document_id": stem,
  "filename": doc.name,
  "source_path": str(doc),
  "engine": "vertex_gemini_sdk",
  "pages": [{"page_number": 1, "text": payloads[stem]}]
}))
"###,
        )
        .expect("mock sdk script should write");

        let result = ingest_expense_documents(
            &paths,
            &IngestionConfig {
                bundle_id: Some("sdk_demo".to_owned()),
                transcriber: IngestionTranscriber::VertexGeminiSdk(VertexGeminiSdkConfig {
                    project_id: Some("demo-project".to_owned()),
                    location: "global".to_owned(),
                    model: "gemini-3.1-flash-lite-preview".to_owned(),
                    service_account_key_path: key_path,
                    python_bin: PathBuf::from("python3"),
                    script_path,
                }),
                fx_mode: IngestionFxMode::Demo,
                compare_receipt_passes: false,
            },
        )
        .expect("sdk ingestion should succeed");

        assert_eq!(result.transcriptions.len(), 3);
        assert!(result
            .transcriptions
            .iter()
            .all(|document| document.engine == TranscriptionEngine::VertexGeminiSdk));
        assert_eq!(result.bundle_id, "sdk_demo");
        assert_eq!(result.projection.bundle.expense_lines.len(), 3);
        assert!(result.review_workbench_html.contains("<!DOCTYPE html>"));
    }

    fn write_synthetic_packet(
        base_dir: &Path,
        fixtures: &[crate::SyntheticDocumentFixture],
    ) -> Vec<PathBuf> {
        fs::create_dir_all(base_dir).expect("temp dir should create");
        fixtures
            .iter()
            .map(|fixture| {
                let path = base_dir.join(&fixture.filename);
                fs::write(&path, &fixture.markdown).expect("fixture should write");
                path
            })
            .collect()
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

    fn spawn_mock_server(
        expected_requests: usize,
        handler: impl Fn(String) -> String + Send + Sync + 'static,
    ) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener.local_addr().expect("address should exist");
        let handler = Arc::new(handler);

        thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("connection should be accepted");
                let request = read_http_request(&mut stream);
                let response = handler(request);
                use std::io::Write;
                stream
                    .write_all(response.as_bytes())
                    .expect("response should write");
            }
        });

        format!("http://{address}")
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read;

        let mut buffer = Vec::new();
        let mut header_end = None;
        let mut content_length = 0usize;

        loop {
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).expect("request should read");
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if header_end.is_none() {
                if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                    header_end = Some(position + 4);
                    let headers = String::from_utf8_lossy(&buffer[..position + 4]);
                    for line in headers.lines() {
                        if let Some(value) = line.strip_prefix("Content-Length:") {
                            content_length = value.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                }
            }
            if let Some(header_end) = header_end {
                if buffer.len() >= header_end + content_length {
                    break;
                }
            }
        }

        String::from_utf8(buffer).expect("request should be utf8")
    }

    fn http_ok(body: serde_json::Value) -> String {
        let body = serde_json::to_string(&body).expect("body should render");
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn write_test_service_account_key(base_dir: &Path) -> PathBuf {
        let path = base_dir.join("service_account.json");
        let key = json!({
            "type": "service_account",
            "project_id": "demo-project",
            "private_key_id": "test-private-key-id",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAK8y5+xXnjF9T8/G\njFOXcY6zOiEhMbng2mMOt2H0nnzAlD2SfshsvBSvDgnCrIPkxFVJAbjw6zqLS5g+\nwMtj2z9yJR3m+CsQELIjpnNimkHf/X6U5PVCq/JJ5FhQag2tUVpOAghitsyYZ/HK\nPV0rRonfp3ausYYsupvlE21EXGtjAgMBAAECgYBim/1ryhkQ894zLSaYehoBXqFu\nOje5znQ84vCWos99mgsV6NmRR5pI7gqxta/SALX85r2gcYGEjxh6VX/AOrEQwvED\nHxTok3BSu7zpZIPWn/o4mUsdu6e6bx+HHhnXZ3kQX/b1q93aHBgqqxkSZVGpj0LC\nM24tnv1ftKW9tR0BuQJBAOHsXrnWM5k55zOVGM+dPMnB/4T5zIaeqUnP/sXGDkvR\nkjUS6efURNS0VdtjaOz4QQc+8RujSCRSBqAyYtkOmD0CQQDGhc8zS5iI8IaFjaoK\n3stxi2hioDvEFdlAaiRMsYU2OzGLmKeaoBX5hvcKfuOwQVB3U+gL4WGNGoJH38So\ne6wfAkEAqUAUAvK2ux7G1zzmVnr8VEXSsAMXtu5b8qEww2dJxIEfIEWoF/ZNDnB/\nNZk2vPiKduwvYr4jSJpuvkqhBO1LHQJAVpOigidUtVvX/sSCRM1XAgSfGGvyxJgW\nr93aSMweYUE9YTjI10k7bB/s+tnNqF9DnVatWwkGhwfpizjORf/xVwJADqDzk0tj\nJ8epIHPjma+48/Ygv3xDb+STi22O23g9BTL8ijFAVdGO3U0JRYN8XjCdgarXmKjs\nEFZnVwSb8Mb48w==\n-----END PRIVATE KEY-----\n",
            "client_email": "vertex-test@demo-project.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token",
        });
        fs::write(&path, serde_json::to_vec_pretty(&key).unwrap())
            .expect("service account key should write");
        path
    }
}
