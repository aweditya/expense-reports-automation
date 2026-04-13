use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::bundle_synthesis::{
    render_canonical_bundle_json_pretty, synthesize_bundle_projection,
    synthesize_bundle_projection_with_fx, BundleProjectionResult, StaticFxRateProvider,
};
use crate::document_extract::extract_document_facts;
use crate::document_facts::render_document_facts_json_pretty;
use crate::ledger::{
    initialize_review_submission_ledger, render_review_submission_ledger_json_pretty,
    ReviewSubmissionLedger,
};
use crate::readiness::{summarize_validation_readiness, ReadinessReport};
use crate::render::render_draft_report_yaml;
use crate::review_packet::{build_review_packet, render_review_packet_json_pretty, ReviewPacket};
use crate::review_workbench::render_review_workbench_html;
use crate::transcribe::{
    render_transcribed_document_json_pretty, transcribe_document_path, TranscribedDocument,
};
use crate::validator::render_validation_report_json_pretty;
use crate::vertex_gemini::{transcribe_document_path_with_vertex, VertexGeminiConfig};
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionConfig {
    pub bundle_id: Option<String>,
    pub transcriber: IngestionTranscriber,
    pub fx_mode: IngestionFxMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionPipelineResult {
    pub bundle_id: String,
    pub transcriptions: Vec<TranscribedDocument>,
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
    Transcription {
        path: PathBuf,
        message: String,
    },
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
    let mut extracted_documents = Vec::new();

    for path in paths {
        let document = match &config.transcriber {
            IngestionTranscriber::Builtin => {
                transcribe_document_path(path).map_err(|err| transcription_error(path, err.to_string()))?
            }
            IngestionTranscriber::VertexGemini(vertex_config) => transcribe_document_path_with_vertex(path, vertex_config)
                .map_err(|err| transcription_error(path, err.to_string()))?,
        };
        let facts = extract_document_facts(&document);
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
    let review_packet = build_review_packet(&projection.bundle, &projection.draft, &projection.validation)
        .map_err(|err| IngestionError::ReviewPacket(format!("failed to build review packet: {err}")))?;
    let review_workbench_html = render_review_workbench_html(&review_packet);

    let bundle_id = config
        .bundle_id
        .clone()
        .unwrap_or_else(|| default_bundle_id(paths));
    let ledger = initialize_review_submission_ledger(
        &bundle_id,
        &projection.bundle,
        &projection.draft,
        &projection.validation,
    )
    .map_err(|err| IngestionError::Ledger(format!("failed to initialize review submission ledger: {err}")))?;

    Ok(IngestionPipelineResult {
        bundle_id,
        transcriptions,
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
    fs::create_dir_all(&transcriptions_dir)?;
    fs::create_dir_all(&facts_dir)?;

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
        render_review_submission_ledger_json_pretty(&result.ledger).map_err(IngestionError::Json)?,
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
            &[flight_path.clone(), hotel_path.clone(), receipt_path.clone()],
            &IngestionConfig {
                bundle_id: Some("vertex_demo".to_owned()),
                transcriber: IngestionTranscriber::VertexGemini(VertexGeminiConfig {
                    project_id: "demo-project".to_owned(),
                    location: "us-central1".to_owned(),
                    model: "gemini-2.5-flash".to_owned(),
                    access_token: Some("test-token".to_owned()),
                    endpoint_override: Some(endpoint),
                }),
                fx_mode: IngestionFxMode::Demo,
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
                    facts.traveler_names.first().map(|value| value.value.as_str()),
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
                    facts.merchant_name.as_ref().map(|value| value.value.as_str()),
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

    fn write_synthetic_packet(base_dir: &Path, fixtures: &[crate::SyntheticDocumentFixture]) -> Vec<PathBuf> {
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

        format!("http://{address}/v1/projects/test/locations/us-central1/publishers/google/models/gemini-2.5-flash:generateContent")
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
}
