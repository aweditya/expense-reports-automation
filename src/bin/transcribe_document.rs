use std::env;
use std::fs;
use std::process::ExitCode;

use expense_report_schema::{
    transcribe_document_path_with_document_ai_profile,
    render_transcribed_document_json_pretty, render_transcribed_document_markdown,
    transcribe_document_path, transcribe_document_path_with_vertex,
    transcribe_document_path_with_vertex_sdk_profile, OcrPassKind, OcrPreprocessVariant,
    DocumentAiConfig, DocumentAiPassProfile, VertexGeminiConfig, VertexGeminiSdkConfig,
    VertexGeminiSdkPassProfile,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Markdown,
    Json,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut path = None;
    let mut format = OutputFormat::Markdown;
    let mut output_path = None;
    let mut engine = "builtin".to_owned();
    let mut project_id = None;
    let mut location = None;
    let mut model = None;
    let mut access_token = None;
    let mut service_account_key_path = None;
    let mut endpoint_override = None;
    let mut token_endpoint_override = None;
    let mut sdk_python = None;
    let mut sdk_script = None;
    let mut processor_id = None;
    let mut processor_version = None;
    let mut pass_id = None;
    let mut pass_kind = OcrPassKind::Primary;
    let mut preprocess_variant = OcrPreprocessVariant::Original;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --format".to_owned())?;
                format = match value.as_str() {
                    "markdown" => OutputFormat::Markdown,
                    "json" => OutputFormat::Json,
                    _ => {
                        return Err(format!(
                            "unsupported output format {value:?}; expected markdown or json"
                        ))
                    }
                };
            }
            "--output" => {
                output_path = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --output".to_owned())?,
                );
            }
            "--engine" => {
                engine = args
                    .next()
                    .ok_or_else(|| "missing value for --engine".to_owned())?;
            }
            "--project" => {
                project_id = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --project".to_owned())?,
                );
            }
            "--location" => {
                location = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --location".to_owned())?,
                );
            }
            "--model" => {
                model = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --model".to_owned())?,
                );
            }
            "--access-token" => {
                access_token = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --access-token".to_owned())?,
                );
            }
            "--service-account-key" => {
                service_account_key_path = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --service-account-key".to_owned())?,
                );
            }
            "--endpoint" => {
                endpoint_override = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --endpoint".to_owned())?,
                );
            }
            "--token-endpoint" => {
                token_endpoint_override = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --token-endpoint".to_owned())?,
                );
            }
            "--sdk-python" => {
                sdk_python = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --sdk-python".to_owned())?,
                );
            }
            "--sdk-script" => {
                sdk_script = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --sdk-script".to_owned())?,
                );
            }
            "--processor-id" => {
                processor_id = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --processor-id".to_owned())?,
                );
            }
            "--processor-version" => {
                processor_version = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --processor-version".to_owned())?,
                );
            }
            "--pass-id" => {
                pass_id = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --pass-id".to_owned())?,
                );
            }
            "--pass-kind" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --pass-kind".to_owned())?;
                pass_kind = match value.as_str() {
                    "primary" => OcrPassKind::Primary,
                    "verification" => OcrPassKind::Verification,
                    "table_focused" => OcrPassKind::TableFocused,
                    "geometry_assist" => OcrPassKind::GeometryAssist,
                    _ => {
                        return Err(format!(
                            "unsupported pass kind {value:?}; expected primary, verification, table_focused, or geometry_assist"
                        ))
                    }
                };
            }
            "--preprocess-variant" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --preprocess-variant".to_owned())?;
                preprocess_variant = match value.as_str() {
                    "original" => OcrPreprocessVariant::Original,
                    "contrast_boosted" => OcrPreprocessVariant::ContrastBoosted,
                    "grayscale" => OcrPreprocessVariant::Grayscale,
                    "binarized" => OcrPreprocessVariant::Binarized,
                    "deskewed" => OcrPreprocessVariant::Deskewed,
                    _ => {
                        return Err(format!(
                            "unsupported preprocess variant {value:?}; expected original, contrast_boosted, grayscale, binarized, or deskewed"
                        ))
                    }
                };
            }
            _ if arg.starts_with("--") => {
                return Err(format!("unknown flag {arg:?}"));
            }
            _ => {
                if path.replace(arg).is_some() {
                    return Err("expected exactly one input path".to_owned());
                }
            }
        }
    }

    let Some(path) = path else {
        return Err("usage: transcribe_document [--format markdown|json] [--output PATH] [--engine builtin|vertex-gemini|vertex-gemini-sdk|document-ai] [--project PROJECT] [--location LOCATION] [--model MODEL] [--access-token TOKEN] [--service-account-key PATH] [--endpoint URL] [--token-endpoint URL] [--sdk-python PATH] [--sdk-script PATH] [--processor-id ID] [--processor-version ID] [--pass-id ID] [--pass-kind primary|verification|table_focused|geometry_assist] [--preprocess-variant original|contrast_boosted|grayscale|binarized|deskewed] <document>".to_owned());
    };

    let document = match engine.as_str() {
        "builtin" => transcribe_document_path(&path).map_err(|err| err.to_string())?,
        "vertex-gemini" => {
            let vertex_config = VertexGeminiConfig::resolve_from_sources(
                project_id,
                location,
                model,
                access_token,
                service_account_key_path.map(Into::into),
                endpoint_override,
                token_endpoint_override,
            )
            .map_err(|err| err.to_string())?;
            transcribe_document_path_with_vertex(&path, &vertex_config)
                .map_err(|err| err.to_string())?
        }
        "vertex-gemini-sdk" => {
            let sdk_config = VertexGeminiSdkConfig::resolve_from_sources(
                project_id,
                location,
                model,
                service_account_key_path.map(Into::into),
                sdk_python.map(Into::into),
                sdk_script.map(Into::into),
            )
            .map_err(|err| err.to_string())?;
            transcribe_document_path_with_vertex_sdk_profile(
                &path,
                &sdk_config,
                &VertexGeminiSdkPassProfile {
                    pass_id,
                    pass_kind,
                    preprocess_variant,
                },
            )
            .map_err(|err| err.to_string())?
        }
        "document-ai" => {
            let document_ai_config = DocumentAiConfig::resolve_from_sources(
                project_id,
                location,
                processor_id,
                processor_version,
                service_account_key_path.map(Into::into),
                sdk_python.map(Into::into),
                sdk_script.map(Into::into),
            )
            .map_err(|err| err.to_string())?;
            transcribe_document_path_with_document_ai_profile(
                &path,
                &document_ai_config,
                &DocumentAiPassProfile {
                    pass_id,
                    pass_kind,
                    preprocess_variant,
                },
            )
            .map_err(|err| err.to_string())?
        }
        _ => {
            return Err(
                "engine must be one of builtin | vertex-gemini | vertex-gemini-sdk | document-ai".to_owned(),
            )
        }
    };
    let rendered = match format {
        OutputFormat::Markdown => render_transcribed_document_markdown(&document),
        OutputFormat::Json => {
            render_transcribed_document_json_pretty(&document).map_err(|err| err.to_string())?
        }
    };

    if let Some(output_path) = output_path {
        fs::write(output_path, rendered).map_err(|err| err.to_string())?;
    } else {
        print!("{rendered}");
    }

    Ok(())
}
