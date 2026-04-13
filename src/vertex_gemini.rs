use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{json, Value as JsonValue};

use crate::transcribe::{TranscribedDocument, TranscribedPage, TranscriptionEngine};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEFAULT_GEMINI_MODEL: &str = "gemini-3.1-flash-lite-preview";
const DEFAULT_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexGeminiConfig {
    pub project_id: String,
    pub location: String,
    pub model: String,
    pub access_token: Option<String>,
    pub service_account_key_path: Option<PathBuf>,
    pub endpoint_override: Option<String>,
    pub token_endpoint_override: Option<String>,
}

impl VertexGeminiConfig {
    pub fn from_env() -> Result<Self, VertexGeminiError> {
        Self::resolve_from_sources(None, None, None, None, None, None, None)
    }

    pub fn resolve_from_sources(
        project_id: Option<String>,
        location: Option<String>,
        model: Option<String>,
        access_token: Option<String>,
        service_account_key_path: Option<PathBuf>,
        endpoint_override: Option<String>,
        token_endpoint_override: Option<String>,
    ) -> Result<Self, VertexGeminiError> {
        let service_account_key_path = service_account_key_path.or_else(|| {
            std::env::var("VERTEX_SERVICE_ACCOUNT_KEY")
                .ok()
                .map(PathBuf::from)
        });
        let project_id = match project_id.or_else(|| std::env::var("VERTEX_PROJECT_ID").ok()) {
            Some(project_id) => project_id,
            None => service_account_key_path
                .as_deref()
                .map(infer_project_id_from_service_account_key_path)
                .transpose()?
                .flatten()
                .ok_or_else(|| {
                    VertexGeminiError::MissingConfiguration(
                        "VERTEX_PROJECT_ID or service account project_id".to_owned(),
                    )
                })?,
        };
        let location = location
            .or_else(|| std::env::var("VERTEX_LOCATION").ok())
            .ok_or_else(|| VertexGeminiError::MissingConfiguration("VERTEX_LOCATION".to_owned()))?;
        let model = model
            .or_else(|| std::env::var("VERTEX_GEMINI_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_owned());
        let access_token = access_token.or_else(|| std::env::var("VERTEX_ACCESS_TOKEN").ok());
        let endpoint_override =
            endpoint_override.or_else(|| std::env::var("VERTEX_ENDPOINT_OVERRIDE").ok());
        let token_endpoint_override = token_endpoint_override
            .or_else(|| std::env::var("VERTEX_TOKEN_ENDPOINT_OVERRIDE").ok());

        Ok(Self {
            project_id,
            location,
            model,
            access_token,
            service_account_key_path,
            endpoint_override,
            token_endpoint_override,
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
    InvalidServiceAccountKey(String),
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
            Self::InvalidServiceAccountKey(message) => {
                write!(f, "invalid service account key: {message}")
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

#[derive(Debug)]
struct ServiceAccountKey {
    project_id: Option<String>,
    private_key_id: Option<String>,
    client_email: String,
    private_key: String,
    token_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawServiceAccountKey {
    r#type: Option<String>,
    project_id: Option<String>,
    private_key_id: Option<String>,
    private_key: Option<String>,
    client_email: Option<String>,
    token_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedPdfPage {
    page_number: u32,
    filename: String,
    bytes: Vec<u8>,
}

pub fn transcribe_document_path_with_vertex(
    path: impl AsRef<Path>,
    config: &VertexGeminiConfig,
) -> Result<TranscribedDocument, VertexGeminiError> {
    transcribe_document_path_with_vertex_and_pdf_renderer(path, config, render_pdf_pages_to_png)
}

fn transcribe_document_path_with_vertex_and_pdf_renderer<F>(
    path: impl AsRef<Path>,
    config: &VertexGeminiConfig,
    pdf_renderer: F,
) -> Result<TranscribedDocument, VertexGeminiError>
where
    F: Fn(&Path) -> Result<Vec<RenderedPdfPage>, VertexGeminiError>,
{
    ensure_tool_available("curl")?;
    let path = path.as_ref();
    let mut resolved_config = config.clone();
    if resolved_config.access_token.is_none() {
        resolved_config.access_token = Some(resolve_access_token(config)?);
    }

    let pages = if is_pdf_path(path) {
        transcribe_pdf_pages_with_vertex(path, &resolved_config, pdf_renderer)?
    } else {
        transcribe_binary_document_with_vertex(path, &resolved_config)?
    };

    Ok(TranscribedDocument {
        document_id: document_id_for_path(path),
        filename: file_name_for_path(path),
        source_path: path.to_path_buf(),
        engine: TranscriptionEngine::VertexGemini,
        pages,
    })
}

fn transcribe_binary_document_with_vertex(
    path: &Path,
    config: &VertexGeminiConfig,
) -> Result<Vec<TranscribedPage>, VertexGeminiError> {
    let mime_type = detect_supported_mime_type(path)?;
    let bytes = fs::read(path)?;
    let request = build_generate_content_request(path, mime_type, &bytes);
    let response = send_generate_content_request(config, &request)?;
    parse_generate_content_response(&response)
}

fn transcribe_pdf_pages_with_vertex<F>(
    path: &Path,
    config: &VertexGeminiConfig,
    pdf_renderer: F,
) -> Result<Vec<TranscribedPage>, VertexGeminiError>
where
    F: Fn(&Path) -> Result<Vec<RenderedPdfPage>, VertexGeminiError>,
{
    let rendered_pages = pdf_renderer(path)?;
    if rendered_pages.is_empty() {
        return Err(VertexGeminiError::InvalidResponse(
            "pdf renderer did not produce any pages".to_owned(),
        ));
    }

    let mut pages = Vec::with_capacity(rendered_pages.len());
    for rendered_page in rendered_pages {
        let request = build_generate_content_request(
            Path::new(&rendered_page.filename),
            "image/png",
            &rendered_page.bytes,
        );
        let response = send_generate_content_request(config, &request)?;
        let page_text = coerce_single_page_text(parse_generate_content_response(&response)?);
        pages.push(TranscribedPage {
            page_number: rendered_page.page_number,
            text: page_text,
        });
    }

    Ok(pages)
}

fn coerce_single_page_text(pages: Vec<TranscribedPage>) -> String {
    if pages.len() == 1 {
        return pages.into_iter().next().unwrap().text;
    }

    pages
        .into_iter()
        .map(|page| page.text)
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
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
            "maxOutputTokens": 16384
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
    let (status, body) = send_http_post_request(
        &endpoint,
        &[
            format!("Authorization: Bearer {token}"),
            "Content-Type: application/json".to_owned(),
        ],
        &request_body,
    )?;
    if !(200..300).contains(&status) {
        return Err(VertexGeminiError::HttpStatus(
            status,
            body.trim().to_owned(),
        ));
    }

    Ok(serde_json::from_str(&body)?)
}

fn parse_generate_content_response(
    response: &JsonValue,
) -> Result<Vec<TranscribedPage>, VertexGeminiError> {
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
    let Some(candidates) = response
        .get("candidates")
        .and_then(|value| value.as_array())
    else {
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

    Ok(serde_json::from_str::<GeminiMarkdownResponse>(
        &trimmed[start..=end],
    )?)
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

fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
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

fn render_pdf_pages_to_png(path: &Path) -> Result<Vec<RenderedPdfPage>, VertexGeminiError> {
    ensure_tool_available("pdfinfo")?;
    ensure_tool_available("pdftoppm")?;

    let page_count = pdf_page_count(path)?;
    let temp_dir = temporary_render_dir("vertex_pdf_pages");
    fs::create_dir_all(&temp_dir)?;
    let render_result = (|| {
        let base_name = document_id_for_path(path);
        let mut rendered_pages = Vec::with_capacity(page_count as usize);
        for page_number in 1..=page_count {
            let output_prefix = temp_dir.join(format!("page_{page_number}"));
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
                return Err(VertexGeminiError::CommandFailed(format!(
                    "pdftoppm failed for page {page_number}: {stderr}"
                )));
            }

            let image_path = output_prefix.with_extension("png");
            let bytes = fs::read(&image_path)?;
            rendered_pages.push(RenderedPdfPage {
                page_number,
                filename: format!("{base_name}_page_{page_number}.png"),
                bytes,
            });
        }
        Ok(rendered_pages)
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    render_result
}

fn pdf_page_count(path: &Path) -> Result<u32, VertexGeminiError> {
    let output = Command::new("pdfinfo").arg(path).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(VertexGeminiError::CommandFailed(format!(
            "pdfinfo failed: {stderr}"
        )));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| VertexGeminiError::InvalidUtf8(err.to_string()))?;
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("Pages:") {
            let page_count = value.trim().parse::<u32>().map_err(|_| {
                VertexGeminiError::InvalidResponse(format!(
                    "failed to parse pdf page count from {:?}",
                    line
                ))
            })?;
            if page_count > 0 {
                return Ok(page_count);
            }
        }
    }

    Err(VertexGeminiError::InvalidResponse(
        "pdfinfo output did not contain a positive page count".to_owned(),
    ))
}

pub(crate) fn resolve_access_token(
    config: &VertexGeminiConfig,
) -> Result<String, VertexGeminiError> {
    if let Some(token) = config
        .access_token
        .clone()
        .or_else(|| std::env::var("VERTEX_ACCESS_TOKEN").ok())
    {
        return Ok(token);
    }

    let service_account_key_path = config.service_account_key_path.clone().or_else(|| {
        std::env::var("VERTEX_SERVICE_ACCOUNT_KEY")
            .ok()
            .map(PathBuf::from)
    });
    if let Some(path) = service_account_key_path.as_deref() {
        return exchange_service_account_key_for_access_token(
            path,
            config.token_endpoint_override.as_deref(),
        );
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
            "VERTEX_ACCESS_TOKEN, VERTEX_SERVICE_ACCOUNT_KEY, or gcloud auth print-access-token"
                .to_owned(),
        ));
    }
    Ok(token)
}

fn exchange_service_account_key_for_access_token(
    path: &Path,
    token_endpoint_override: Option<&str>,
) -> Result<String, VertexGeminiError> {
    let service_account = read_service_account_key(path)?;
    let token_endpoint = token_endpoint_override
        .map(str::to_owned)
        .or(service_account.token_uri.clone())
        .unwrap_or_else(|| DEFAULT_TOKEN_ENDPOINT.to_owned());
    let assertion = build_service_account_assertion(&service_account, &token_endpoint)?;
    let request_body = build_service_account_token_request_body(&assertion);
    let (status, body) = send_http_post_request(
        &token_endpoint,
        &["Content-Type: application/x-www-form-urlencoded".to_owned()],
        request_body.as_bytes(),
    )?;
    if !(200..300).contains(&status) {
        return Err(VertexGeminiError::HttpStatus(
            status,
            body.trim().to_owned(),
        ));
    }

    let token_response: OAuthTokenResponse = serde_json::from_str(&body)?;
    if let Some(access_token) = token_response.access_token {
        if access_token.trim().is_empty() {
            return Err(VertexGeminiError::InvalidResponse(
                "service account token exchange returned an empty access_token".to_owned(),
            ));
        }
        return Ok(access_token);
    }

    let error = token_response.error;
    let error_description = token_response.error_description;
    let message = error
        .clone()
        .zip(error_description.clone())
        .map(|(error, description)| format!("{error}: {description}"))
        .or(error)
        .or(error_description)
        .unwrap_or_else(|| {
            "service account token exchange response did not include access_token".to_owned()
        });
    Err(VertexGeminiError::InvalidResponse(message))
}

fn infer_project_id_from_service_account_key_path(
    path: &Path,
) -> Result<Option<String>, VertexGeminiError> {
    Ok(read_service_account_key(path)?.project_id)
}

fn read_service_account_key(path: &Path) -> Result<ServiceAccountKey, VertexGeminiError> {
    let raw = fs::read_to_string(path)?;
    let parsed = serde_json::from_str::<RawServiceAccountKey>(&raw).map_err(|err| {
        VertexGeminiError::InvalidServiceAccountKey(format!(
            "failed to parse {}: {err}",
            path.display()
        ))
    })?;
    if let Some(key_type) = parsed.r#type.as_deref() {
        if key_type != "service_account" {
            return Err(VertexGeminiError::InvalidServiceAccountKey(format!(
                "{} is type {key_type:?}, expected \"service_account\"",
                path.display()
            )));
        }
    }

    let client_email = parsed
        .client_email
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            VertexGeminiError::InvalidServiceAccountKey(format!(
                "{} is missing client_email",
                path.display()
            ))
        })?;
    let private_key = parsed
        .private_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            VertexGeminiError::InvalidServiceAccountKey(format!(
                "{} is missing private_key",
                path.display()
            ))
        })?;

    Ok(ServiceAccountKey {
        project_id: parsed.project_id.filter(|value| !value.trim().is_empty()),
        private_key_id: parsed
            .private_key_id
            .filter(|value| !value.trim().is_empty()),
        client_email,
        private_key,
        token_uri: parsed.token_uri.filter(|value| !value.trim().is_empty()),
    })
}

fn build_service_account_assertion(
    service_account: &ServiceAccountKey,
    token_endpoint: &str,
) -> Result<String, VertexGeminiError> {
    let issued_at = current_unix_timestamp()?;
    let expires_at = issued_at + 3600;
    let header = if let Some(private_key_id) = &service_account.private_key_id {
        json!({
            "alg": "RS256",
            "typ": "JWT",
            "kid": private_key_id,
        })
    } else {
        json!({
            "alg": "RS256",
            "typ": "JWT",
        })
    };
    let claims = json!({
        "iss": service_account.client_email,
        "scope": CLOUD_PLATFORM_SCOPE,
        "aud": token_endpoint,
        "iat": issued_at,
        "exp": expires_at,
    });
    let signing_input = format!(
        "{}.{}",
        base64_url_encode(serde_json::to_string(&header)?.as_bytes()),
        base64_url_encode(serde_json::to_string(&claims)?.as_bytes())
    );
    let signature = sign_rs256(&service_account.private_key, &signing_input)?;
    Ok(format!("{signing_input}.{}", base64_url_encode(&signature)))
}

fn current_unix_timestamp() -> Result<u64, VertexGeminiError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| VertexGeminiError::InvalidResponse(format!("clock error: {err}")))?
        .as_secs())
}

fn sign_rs256(private_key_pem: &str, signing_input: &str) -> Result<Vec<u8>, VertexGeminiError> {
    ensure_tool_available("openssl")?;
    let temp_key_path = temporary_private_key_path();
    fs::write(&temp_key_path, private_key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&temp_key_path)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&temp_key_path, permissions)?;
    }

    let result = (|| {
        let mut child = Command::new("openssl")
            .args(["dgst", "-binary", "-sha256", "-sign"])
            .arg(&temp_key_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            VertexGeminiError::CommandFailed("failed to open openssl stdin".to_owned())
        })?;
        stdin.write_all(signing_input.as_bytes())?;
        drop(stdin);

        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(VertexGeminiError::CommandFailed(stderr));
        }

        Ok(output.stdout)
    })();

    let _ = fs::remove_file(&temp_key_path);
    result
}

fn temporary_private_key_path() -> PathBuf {
    temporary_timestamped_path("expense_report_schema_vertex_key", "pem")
}

fn temporary_render_dir(prefix: &str) -> PathBuf {
    temporary_timestamped_path(&format!("expense_report_schema_{prefix}"), "")
}

fn temporary_timestamped_path(prefix: &str, extension: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be valid")
        .as_nanos();
    let basename = format!("{prefix}_{}_{}", std::process::id(), timestamp);
    if extension.is_empty() {
        std::env::temp_dir().join(basename)
    } else {
        std::env::temp_dir().join(format!("{basename}.{extension}"))
    }
}

fn build_service_account_token_request_body(assertion: &str) -> String {
    format!(
        "grant_type={}&assertion={}",
        percent_encode_form_component("urn:ietf:params:oauth:grant-type:jwt-bearer"),
        percent_encode_form_component(assertion),
    )
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

fn base64_url_encode(bytes: &[u8]) -> String {
    base64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

fn percent_encode_form_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn send_http_post_request(
    endpoint: &str,
    headers: &[String],
    body: &[u8],
) -> Result<(u16, String), VertexGeminiError> {
    ensure_tool_available("curl")?;

    let mut command = Command::new("curl");
    command.arg("-sS").arg("-X").arg("POST");
    for header in headers {
        command.arg("-H").arg(header);
    }
    let mut child = command
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
    stdin.write_all(body)?;
    drop(stdin);

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(VertexGeminiError::CommandFailed(stderr));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| VertexGeminiError::InvalidUtf8(err.to_string()))?;
    let Some((response_body, status_line)) = stdout.rsplit_once('\n') else {
        return Err(VertexGeminiError::InvalidResponse(
            "curl output did not include an HTTP status line".to_owned(),
        ));
    };
    let status = status_line.trim().parse::<u16>().map_err(|_| {
        VertexGeminiError::InvalidResponse("failed to parse HTTP status".to_owned())
    })?;
    Ok((status, response_body.to_owned()))
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

    const TEST_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----\n\
MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAK8y5+xXnjF9T8/G\n\
jFOXcY6zOiEhMbng2mMOt2H0nnzAlD2SfshsvBSvDgnCrIPkxFVJAbjw6zqLS5g+\n\
wMtj2z9yJR3m+CsQELIjpnNimkHf/X6U5PVCq/JJ5FhQag2tUVpOAghitsyYZ/HK\n\
PV0rRonfp3ausYYsupvlE21EXGtjAgMBAAECgYBim/1ryhkQ894zLSaYehoBXqFu\n\
Oje5znQ84vCWos99mgsV6NmRR5pI7gqxta/SALX85r2gcYGEjxh6VX/AOrEQwvED\n\
HxTok3BSu7zpZIPWn/o4mUsdu6e6bx+HHhnXZ3kQX/b1q93aHBgqqxkSZVGpj0LC\n\
M24tnv1ftKW9tR0BuQJBAOHsXrnWM5k55zOVGM+dPMnB/4T5zIaeqUnP/sXGDkvR\n\
kjUS6efURNS0VdtjaOz4QQc+8RujSCRSBqAyYtkOmD0CQQDGhc8zS5iI8IaFjaoK\n\
3stxi2hioDvEFdlAaiRMsYU2OzGLmKeaoBX5hvcKfuOwQVB3U+gL4WGNGoJH38So\n\
e6wfAkEAqUAUAvK2ux7G1zzmVnr8VEXSsAMXtu5b8qEww2dJxIEfIEWoF/ZNDnB/\n\
NZk2vPiKduwvYr4jSJpuvkqhBO1LHQJAVpOigidUtVvX/sSCRM1XAgSfGGvyxJgW\n\
r93aSMweYUE9YTjI10k7bB/s+tnNqF9DnVatWwkGhwfpizjORf/xVwJADqDzk0tj\n\
J8epIHPjma+48/Ygv3xDb+STi22O23g9BTL8ijFAVdGO3U0JRYN8XjCdgarXmKjs\n\
EFZnVwSb8Mb48w==\n\
-----END PRIVATE KEY-----\n";

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
    fn resolves_project_id_from_service_account_key() {
        let temp_dir = unique_temp_dir("vertex_key_project");
        let service_account_key_path = write_test_service_account_key(&temp_dir);

        let config = VertexGeminiConfig::resolve_from_sources(
            None,
            Some("us-central1".to_owned()),
            None,
            None,
            Some(service_account_key_path.clone()),
            Some("http://127.0.0.1:0/generate".to_owned()),
            Some("http://127.0.0.1:0/token".to_owned()),
        )
        .expect("config should resolve from service account key");

        assert_eq!(config.project_id, "demo-project");
        assert_eq!(config.model, DEFAULT_GEMINI_MODEL);
        assert_eq!(
            config.service_account_key_path,
            Some(service_account_key_path)
        );
    }

    #[test]
    fn exchanges_service_account_key_for_access_token() {
        let temp_dir = unique_temp_dir("vertex_token_exchange");
        let service_account_key_path = write_test_service_account_key(&temp_dir);
        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let requests_for_server = Arc::clone(&requests);
        let base_url = spawn_mock_server(1, move |request| {
            requests_for_server.lock().unwrap().push(request);
            http_ok(json!({
                "access_token": "service-account-token",
                "token_type": "Bearer",
                "expires_in": 3600
            }))
        });

        let token = resolve_access_token(&VertexGeminiConfig {
            project_id: "demo-project".to_owned(),
            location: "us-central1".to_owned(),
            model: DEFAULT_GEMINI_MODEL.to_owned(),
            access_token: None,
            service_account_key_path: Some(service_account_key_path),
            endpoint_override: Some(format!("{base_url}/generate")),
            token_endpoint_override: Some(format!("{base_url}/token")),
        })
        .expect("service account key exchange should succeed");

        assert_eq!(token, "service-account-token");

        let request = requests.lock().unwrap().join("\n");
        assert!(request.starts_with("POST /token "));
        let body = http_request_body(&request);
        let form = parse_form_urlencoded(body);
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("urn:ietf:params:oauth:grant-type:jwt-bearer")
        );
        let assertion = form
            .get("assertion")
            .expect("assertion should be present in token exchange");
        let segments = assertion.split('.').collect::<Vec<_>>();
        assert_eq!(segments.len(), 3);

        let header: JsonValue = serde_json::from_slice(
            &base64_url_decode(segments[0]).expect("jwt header should decode"),
        )
        .expect("jwt header should parse");
        let claims: JsonValue = serde_json::from_slice(
            &base64_url_decode(segments[1]).expect("jwt claims should decode"),
        )
        .expect("jwt claims should parse");
        assert_eq!(header["alg"], "RS256");
        assert_eq!(
            claims["iss"],
            "vertex-test@demo-project.iam.gserviceaccount.com"
        );
        assert_eq!(claims["scope"], CLOUD_PLATFORM_SCOPE);
        assert_eq!(claims["aud"], format!("{base_url}/token"));
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
                model: DEFAULT_GEMINI_MODEL.to_owned(),
                access_token: Some("test-token".to_owned()),
                service_account_key_path: None,
                endpoint_override: Some(format!("{endpoint}/generate")),
                token_endpoint_override: None,
            },
        )
        .expect("mock vertex transcription should succeed");

        assert_eq!(document.engine, TranscriptionEngine::VertexGemini);
        assert_eq!(document.pages.len(), 1);
        assert!(document.pages[0]
            .text
            .contains("Merchant Name: Test Bistro"));

        let request_body = requests.lock().unwrap().join("\n");
        assert!(request_body.contains("\"mimeType\":\"image/png\""));
        assert!(request_body.contains("vertex_mock_"));
        assert!(request_body.contains("\"responseMimeType\":\"application/json\""));

        fs::remove_file(path).expect("temp input should be removable");
    }

    #[test]
    fn transcribes_document_with_service_account_key() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let temp_dir = unique_temp_dir("vertex_key_transcribe");
        let service_account_key_path = write_test_service_account_key(&temp_dir);
        let path = std::env::temp_dir().join(format!("vertex_key_mock_{unique}.png"));
        fs::write(&path, b"not-a-real-png").expect("temp input should be writable");

        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let requests_for_server = Arc::clone(&requests);
        let base_url = spawn_mock_server(2, move |request| {
            requests_for_server.lock().unwrap().push(request.clone());
            if request.starts_with("POST /token ") {
                http_ok(json!({
                    "access_token": "minted-token",
                    "token_type": "Bearer",
                    "expires_in": 3600
                }))
            } else {
                http_ok(json!({
                    "candidates": [
                        {
                            "content": {
                                "parts": [
                                    {
                                        "text": "{\"pages\":[{\"page_number\":1,\"text\":\"# Merchant Receipt\\n\\n- Merchant Name: Token Bistro\"}]}"
                                    }
                                ]
                            }
                        }
                    ]
                }))
            }
        });

        let document = transcribe_document_path_with_vertex(
            &path,
            &VertexGeminiConfig {
                project_id: "demo-project".to_owned(),
                location: "us-central1".to_owned(),
                model: DEFAULT_GEMINI_MODEL.to_owned(),
                access_token: None,
                service_account_key_path: Some(service_account_key_path),
                endpoint_override: Some(format!("{base_url}/generate")),
                token_endpoint_override: Some(format!("{base_url}/token")),
            },
        )
        .expect("mock vertex transcription should succeed");

        assert_eq!(document.engine, TranscriptionEngine::VertexGemini);
        assert_eq!(document.pages.len(), 1);
        assert!(document.pages[0]
            .text
            .contains("Merchant Name: Token Bistro"));

        let captured = requests.lock().unwrap().clone();
        assert_eq!(captured.len(), 2);
        assert!(captured[0].starts_with("POST /token "));
        assert!(captured[1].starts_with("POST /generate "));
        assert!(captured[1].contains("Authorization: Bearer minted-token"));
        assert!(captured[1].contains("\"mimeType\":\"image/png\""));

        fs::remove_file(path).expect("temp input should be removable");
    }

    #[test]
    fn transcribes_pdf_by_rendering_pages_individually() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vertex_pdf_mock_{unique}.pdf"));
        fs::write(&path, b"%PDF-1.4 mock").expect("temp pdf should be writable");

        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let requests_for_server = Arc::clone(&requests);
        let endpoint = spawn_mock_server(2, move |request| {
            requests_for_server.lock().unwrap().push(request.clone());
            if request.contains("Filename: mock_report_page_1.png") {
                http_ok(json!({
                    "candidates": [
                        {
                            "content": {
                                "parts": [
                                    {
                                        "text": "{\"pages\":[{\"page_number\":1,\"text\":\"# Page One\\n\\n- Merchant Name: First Page Cafe\"}]}"
                                    }
                                ]
                            }
                        }
                    ]
                }))
            } else {
                http_ok(json!({
                    "candidates": [
                        {
                            "content": {
                                "parts": [
                                    {
                                        "text": "{\"pages\":[{\"page_number\":1,\"text\":\"# Page Two\\n\\n- Merchant Name: Second Page Cafe\"}]}"
                                    }
                                ]
                            }
                        }
                    ]
                }))
            }
        });

        let document = transcribe_document_path_with_vertex_and_pdf_renderer(
            &path,
            &VertexGeminiConfig {
                project_id: "demo-project".to_owned(),
                location: "us-central1".to_owned(),
                model: DEFAULT_GEMINI_MODEL.to_owned(),
                access_token: Some("test-token".to_owned()),
                service_account_key_path: None,
                endpoint_override: Some(format!("{endpoint}/generate")),
                token_endpoint_override: None,
            },
            |_| {
                Ok(vec![
                    RenderedPdfPage {
                        page_number: 1,
                        filename: "mock_report_page_1.png".to_owned(),
                        bytes: b"page-one".to_vec(),
                    },
                    RenderedPdfPage {
                        page_number: 2,
                        filename: "mock_report_page_2.png".to_owned(),
                        bytes: b"page-two".to_vec(),
                    },
                ])
            },
        )
        .expect("pdf should transcribe via page renderer");

        assert_eq!(document.engine, TranscriptionEngine::VertexGemini);
        assert_eq!(document.pages.len(), 2);
        assert_eq!(document.pages[0].page_number, 1);
        assert_eq!(document.pages[1].page_number, 2);
        assert!(document.pages[0].text.contains("First Page Cafe"));
        assert!(document.pages[1].text.contains("Second Page Cafe"));

        let captured = requests.lock().unwrap().join("\n");
        assert!(!captured.contains("\"mimeType\":\"application/pdf\""));
        assert_eq!(captured.matches("\"mimeType\":\"image/png\"").count(), 2);
        assert!(captured.contains("mock_report_page_1.png"));
        assert!(captured.contains("mock_report_page_2.png"));

        fs::remove_file(path).expect("temp pdf should be removable");
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

        format!("http://{address}")
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

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("expense_report_schema_{prefix}_{unique}"));
        fs::create_dir_all(&path).expect("temp dir should create");
        path
    }

    fn write_test_service_account_key(base_dir: &Path) -> PathBuf {
        let path = base_dir.join("service_account.json");
        let key = json!({
            "type": "service_account",
            "project_id": "demo-project",
            "private_key_id": "test-private-key-id",
            "private_key": TEST_PRIVATE_KEY,
            "client_email": "vertex-test@demo-project.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token",
        });
        fs::write(&path, serde_json::to_vec_pretty(&key).unwrap())
            .expect("service account key should write");
        path
    }

    fn http_request_body(request: &str) -> &str {
        request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or("")
    }

    fn parse_form_urlencoded(body: &str) -> std::collections::HashMap<String, String> {
        body.split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                (
                    percent_decode_form_component(key),
                    percent_decode_form_component(value),
                )
            })
            .collect()
    }

    fn percent_decode_form_component(value: &str) -> String {
        let mut decoded = Vec::new();
        let bytes = value.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'+' => {
                    decoded.push(b' ');
                    index += 1;
                }
                b'%' if index + 2 < bytes.len() => {
                    let high = (bytes[index + 1] as char)
                        .to_digit(16)
                        .expect("percent encoding should be hex");
                    let low = (bytes[index + 2] as char)
                        .to_digit(16)
                        .expect("percent encoding should be hex");
                    decoded.push(((high << 4) + low) as u8);
                    index += 3;
                }
                byte => {
                    decoded.push(byte);
                    index += 1;
                }
            }
        }
        String::from_utf8(decoded).expect("decoded form component should be utf8")
    }

    fn base64_url_decode(segment: &str) -> Result<Vec<u8>, String> {
        let mut normalized = segment.replace('-', "+").replace('_', "/");
        while normalized.len() % 4 != 0 {
            normalized.push('=');
        }
        base64_decode(&normalized)
    }

    fn base64_decode(value: &str) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        let mut block = Vec::with_capacity(4);
        for ch in value.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
            block.push(ch);
            if block.len() == 4 {
                decode_base64_block(&block, &mut bytes)?;
                block.clear();
            }
        }
        if !block.is_empty() {
            return Err("base64 input length was not a multiple of 4".to_owned());
        }
        Ok(bytes)
    }

    fn decode_base64_block(block: &[u8], output: &mut Vec<u8>) -> Result<(), String> {
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        for (index, byte) in block.iter().enumerate() {
            values[index] = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    padding += 1;
                    0
                }
                other => return Err(format!("invalid base64 byte {other:?}")),
            };
        }

        output.push((values[0] << 2) | (values[1] >> 4));
        if padding < 2 {
            output.push((values[1] << 4) | (values[2] >> 2));
        }
        if padding == 0 {
            output.push((values[2] << 6) | values[3]);
        }
        Ok(())
    }
}
