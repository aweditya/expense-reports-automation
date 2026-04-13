use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{json, Value as JsonValue};

use crate::transcribe::{TranscribedDocument, TranscribedPage, TranscriptionEngine};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexGeminiConfig {
    pub project_id: String,
    pub location: String,
    pub model: String,
    pub access_token: Option<String>,
    pub endpoint_override: Option<String>,
}

impl VertexGeminiConfig {
    pub fn from_env() -> Result<Self, VertexGeminiError> {
        let project_id = std::env::var("VERTEX_PROJECT_ID")
            .map_err(|_| VertexGeminiError::MissingConfiguration("VERTEX_PROJECT_ID".to_owned()))?;
        let location = std::env::var("VERTEX_LOCATION")
            .map_err(|_| VertexGeminiError::MissingConfiguration("VERTEX_LOCATION".to_owned()))?;
        let model =
            std::env::var("VERTEX_GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_owned());
        let access_token = std::env::var("VERTEX_ACCESS_TOKEN").ok();
        let endpoint_override = std::env::var("VERTEX_ENDPOINT_OVERRIDE").ok();

        Ok(Self {
            project_id,
            location,
            model,
            access_token,
            endpoint_override,
        })
    }

    pub fn endpoint(&self) -> String {
        if let Some(endpoint_override) = &self.endpoint_override {
            endpoint_override.clone()
        } else {
            format!(
                "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
                self.location, self.project_id, self.location, self.model
            )
        }
    }
}

#[derive(Debug)]
pub enum VertexGeminiError {
    Io(std::io::Error),
    Json(serde_json::Error),
    MissingConfiguration(String),
    MissingTool(&'static str),
    UnsupportedDocumentFormat(String),
    CommandFailed(String),
    HttpStatus(u16, String),
    InvalidUtf8(String),
    InvalidResponse(String),
}

impl fmt::Display for VertexGeminiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::MissingConfiguration(key) => {
                write!(f, "missing Vertex Gemini configuration for {key}")
            }
            Self::MissingTool(tool) => write!(f, "required tool is missing: {tool}"),
            Self::UnsupportedDocumentFormat(ext) => write!(
                f,
                "unsupported Vertex Gemini document format {ext:?}; expected .pdf, .png, .jpg, or .jpeg"
            ),
            Self::CommandFailed(message) => write!(f, "Vertex Gemini request failed: {message}"),
            Self::HttpStatus(status, body) => {
                write!(f, "Vertex Gemini returned HTTP {status}: {body}")
            }
            Self::InvalidUtf8(message) => write!(f, "invalid UTF-8 response: {message}"),
            Self::InvalidResponse(message) => write!(f, "invalid Vertex Gemini response: {message}"),
        }
    }
}

impl std::error::Error for VertexGeminiError {}

impl From<std::io::Error> for VertexGeminiError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for VertexGeminiError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Deserialize)]
struct GeminiMarkdownResponse {
    pages: Option<Vec<GeminiMarkdownPage>>,
    document_markdown_pages: Option<Vec<GeminiMarkdownPage>>,
    markdown: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiMarkdownPage {
    page_number: Option<u32>,
    text: String,
}

pub fn transcribe_document_path_with_vertex(
    path: impl AsRef<Path>,
    config: &VertexGeminiConfig,
) -> Result<TranscribedDocument, VertexGeminiError> {
    ensure_tool_available("curl")?;
    let path = path.as_ref();
    let mime_type = detect_supported_mime_type(path)?;
    let bytes = fs::read(path)?;
    let request = build_generate_content_request(path, mime_type, &bytes);
    let response = send_generate_content_request(config, &request)?;
    let pages = parse_generate_content_response(&response)?;

    Ok(TranscribedDocument {
        document_id: document_id_for_path(path),
        filename: file_name_for_path(path),
        source_path: path.to_path_buf(),
        engine: TranscriptionEngine::VertexGemini,
        pages,
    })
}

fn build_generate_content_request(path: &Path, mime_type: &str, bytes: &[u8]) -> JsonValue {
    let prompt = build_transcription_prompt(path, mime_type);
    json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    { "text": prompt },
                    {
                        "inlineData": {
                            "mimeType": mime_type,
                            "data": base64_encode(bytes),
                        }
                    }
                ]
            }
        ],
        "generationConfig": {
            "temperature": 0,
            "responseMimeType": "application/json",
            "maxOutputTokens": 8192
        }
    })
}

fn build_transcription_prompt(path: &Path, mime_type: &str) -> String {
    format!(
        concat!(
            "Transcribe this financial document into extractor-friendly markdown and return only JSON.\n",
            "Use this schema exactly:\n",
            "{{\"pages\":[{{\"page_number\":1,\"text\":\"...markdown...\"}}]}}\n",
            "Rules:\n",
            "- Preserve amounts, currencies, dates, names, confirmation codes, and IDs exactly.\n",
            "- Do not invent values or infer missing fields.\n",
            "- Use markdown headings and bullet points.\n",
            "- Prefer `Label: Value` bullets for standalone facts.\n",
            "- For repeated rows, use one bullet per row or pipe-delimited lines.\n",
            "- Keep one `pages[]` entry per source page when page boundaries are visible; otherwise use a single page.\n",
            "- Keep the top heading faithful to the source document.\n",
            "- If the source is clearly a flight itinerary, hotel folio, or merchant/card receipt, normalize the layout so headings and labels stay easy to parse downstream.\n",
            "Filename: {}\n",
            "Mime type: {}\n"
        ),
        file_name_for_path(path),
        mime_type,
    )
}

fn send_generate_content_request(
    config: &VertexGeminiConfig,
    request: &JsonValue,
) -> Result<JsonValue, VertexGeminiError> {
    let token = resolve_access_token(config)?;
    let endpoint = config.endpoint();
    let request_body = serde_json::to_vec(request)?;

    let mut child = Command::new("curl")
        .arg("-sS")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg(format!("Authorization: Bearer {token}"))
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("--data-binary")
        .arg("@-")
        .arg("-w")
        .arg("\n%{http_code}")
        .arg(endpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| VertexGeminiError::CommandFailed("failed to open curl stdin".to_owned()))?;
    stdin.write_all(&request_body)?;
    drop(stdin);

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(VertexGeminiError::CommandFailed(stderr));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| VertexGeminiError::InvalidUtf8(err.to_string()))?;
    let Some((body, status_line)) = stdout.rsplit_once('\n') else {
        return Err(VertexGeminiError::InvalidResponse(
            "curl output did not include an HTTP status line".to_owned(),
        ));
    };
    let status = status_line
        .trim()
        .parse::<u16>()
        .map_err(|_| VertexGeminiError::InvalidResponse("failed to parse HTTP status".to_owned()))?;
    if !(200..300).contains(&status) {
        return Err(VertexGeminiError::HttpStatus(status, body.trim().to_owned()));
    }

    Ok(serde_json::from_str(body)?)
}

fn parse_generate_content_response(response: &JsonValue) -> Result<Vec<TranscribedPage>, VertexGeminiError> {
    let text = extract_candidate_text(response)?;
    let payload = parse_json_payload(&text)?;
    let pages = payload
        .pages
        .or(payload.document_markdown_pages)
        .or_else(|| {
            payload
                .markdown
                .or(payload.text)
                .map(|text| vec![GeminiMarkdownPage { page_number: Some(1), text }])
        })
        .ok_or_else(|| {
            VertexGeminiError::InvalidResponse(
                "response JSON did not contain `pages`, `document_markdown_pages`, `markdown`, or `text`".to_owned(),
            )
        })?;

    let normalized = pages
        .into_iter()
        .enumerate()
        .map(|(index, page)| TranscribedPage {
            page_number: page.page_number.unwrap_or(index as u32 + 1),
            text: page.text,
        })
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        return Err(VertexGeminiError::InvalidResponse(
            "response JSON did not contain any transcribed pages".to_owned(),
        ));
    }

    Ok(normalized)
}

fn extract_candidate_text(response: &JsonValue) -> Result<String, VertexGeminiError> {
    let Some(candidates) = response.get("candidates").and_then(|value| value.as_array()) else {
        return Err(VertexGeminiError::InvalidResponse(
            "missing `candidates` array".to_owned(),
        ));
    };

    for candidate in candidates {
        let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(|value| value.as_array())
        else {
            continue;
        };

        let text = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return Ok(text);
        }
    }

    Err(VertexGeminiError::InvalidResponse(
        "no text response found in candidate parts".to_owned(),
    ))
}

fn parse_json_payload(text: &str) -> Result<GeminiMarkdownResponse, VertexGeminiError> {
    if let Ok(payload) = serde_json::from_str::<GeminiMarkdownResponse>(text) {
        return Ok(payload);
    }

    let trimmed = strip_code_fences(text).trim();
    if let Ok(payload) = serde_json::from_str::<GeminiMarkdownResponse>(trimmed) {
        return Ok(payload);
    }

    let Some(start) = trimmed.find('{') else {
        return Err(VertexGeminiError::InvalidResponse(
            "transcription response did not contain a JSON object".to_owned(),
        ));
    };
    let Some(end) = trimmed.rfind('}') else {
        return Err(VertexGeminiError::InvalidResponse(
            "transcription response did not contain a complete JSON object".to_owned(),
        ));
    };

    Ok(serde_json::from_str::<GeminiMarkdownResponse>(&trimmed[start..=end])?)
}

fn strip_code_fences(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    trimmed
}

fn detect_supported_mime_type(path: &Path) -> Result<&'static str, VertexGeminiError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => Ok("application/pdf"),
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        _ => Err(VertexGeminiError::UnsupportedDocumentFormat(extension)),
    }
}

fn resolve_access_token(config: &VertexGeminiConfig) -> Result<String, VertexGeminiError> {
    if let Some(token) = config
        .access_token
        .clone()
        .or_else(|| std::env::var("VERTEX_ACCESS_TOKEN").ok())
    {
        return Ok(token);
    }

    ensure_tool_available("gcloud")?;
    let output = Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(VertexGeminiError::CommandFailed(stderr));
    }

    let token = String::from_utf8(output.stdout)
        .map_err(|err| VertexGeminiError::InvalidUtf8(err.to_string()))?;
    let token = token.trim().to_owned();
    if token.is_empty() {
        return Err(VertexGeminiError::MissingConfiguration(
            "VERTEX_ACCESS_TOKEN or gcloud auth print-access-token".to_owned(),
        ));
    }
    Ok(token)
}

fn ensure_tool_available(tool: &'static str) -> Result<(), VertexGeminiError> {
    let status = Command::new("which").arg(tool).output()?;
    if status.status.success() {
        Ok(())
    } else {
        Err(VertexGeminiError::MissingTool(tool))
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

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut index = 0usize;

    while index + 3 <= bytes.len() {
        let chunk = &bytes[index..index + 3];
        encoded.push(TABLE[(chunk[0] >> 2) as usize] as char);
        encoded.push(TABLE[(((chunk[0] & 0b0000_0011) << 4) | (chunk[1] >> 4)) as usize] as char);
        encoded.push(TABLE[(((chunk[1] & 0b0000_1111) << 2) | (chunk[2] >> 6)) as usize] as char);
        encoded.push(TABLE[(chunk[2] & 0b0011_1111) as usize] as char);
        index += 3;
    }

    let remainder = bytes.len() - index;
    if remainder == 1 {
        let byte = bytes[index];
        encoded.push(TABLE[(byte >> 2) as usize] as char);
        encoded.push(TABLE[((byte & 0b0000_0011) << 4) as usize] as char);
        encoded.push('=');
        encoded.push('=');
    } else if remainder == 2 {
        let first = bytes[index];
        let second = bytes[index + 1];
        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        encoded.push(TABLE[((second & 0b0000_1111) << 2) as usize] as char);
        encoded.push('=');
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn detects_supported_mime_types() {
        assert_eq!(
            detect_supported_mime_type(Path::new("receipt.png")).unwrap(),
            "image/png"
        );
        assert_eq!(
            detect_supported_mime_type(Path::new("hotel.jpeg")).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            detect_supported_mime_type(Path::new("folio.pdf")).unwrap(),
            "application/pdf"
        );
    }

    #[test]
    fn parses_response_payload_wrapped_in_code_fences() {
        let payload = parse_json_payload(
            "```json\n{\"pages\":[{\"page_number\":1,\"text\":\"# Merchant Receipt\"}]}\n```",
        )
        .expect("code fenced json should parse");
        assert_eq!(payload.pages.unwrap()[0].text, "# Merchant Receipt");
    }

    #[test]
    fn transcribes_document_with_mock_vertex_server() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vertex_mock_{unique}.png"));
        fs::write(&path, b"not-a-real-png").expect("temp input should be writable");

        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let requests_for_server = Arc::clone(&requests);
        let endpoint = spawn_mock_server(1, move |body| {
            requests_for_server.lock().unwrap().push(body);
            http_ok(json!({
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {
                                    "text": "{\"pages\":[{\"page_number\":1,\"text\":\"# Merchant Receipt\\n\\n- Merchant Name: Test Bistro\"}]}"
                                }
                            ]
                        }
                    }
                ]
            }))
        });

        let document = transcribe_document_path_with_vertex(
            &path,
            &VertexGeminiConfig {
                project_id: "demo-project".to_owned(),
                location: "us-central1".to_owned(),
                model: "gemini-2.5-flash".to_owned(),
                access_token: Some("test-token".to_owned()),
                endpoint_override: Some(endpoint),
            },
        )
        .expect("mock vertex transcription should succeed");

        assert_eq!(document.engine, TranscriptionEngine::VertexGemini);
        assert_eq!(document.pages.len(), 1);
        assert!(document.pages[0].text.contains("Merchant Name: Test Bistro"));

        let request_body = requests.lock().unwrap().join("\n");
        assert!(request_body.contains("\"mimeType\":\"image/png\""));
        assert!(request_body.contains("vertex_mock_"));
        assert!(request_body.contains("\"responseMimeType\":\"application/json\""));

        fs::remove_file(path).expect("temp input should be removable");
    }

    fn spawn_mock_server(
        expected_requests: usize,
        handler: impl Fn(String) -> String + Send + Sync + 'static,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener.local_addr().expect("address should exist");
        let handler = Arc::new(handler);

        thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("connection should be accepted");
                let request = read_http_request(&mut stream);
                let response = handler(request);
                stream
                    .write_all(response.as_bytes())
                    .expect("response should write");
            }
        });

        format!("http://{address}/v1/projects/test/locations/us-central1/publishers/google/models/gemini-2.5-flash:generateContent")
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
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

    fn http_ok(body: JsonValue) -> String {
        let body = serde_json::to_string(&body).expect("body should render");
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }
}
