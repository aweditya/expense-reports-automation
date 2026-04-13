use std::env;
use std::fs;
use std::process::ExitCode;

use expense_report_schema::{
    render_transcribed_document_json_pretty, render_transcribed_document_markdown,
    transcribe_document_path, transcribe_document_path_with_vertex, VertexGeminiConfig,
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
    let mut endpoint_override = None;

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
            "--endpoint" => {
                endpoint_override = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --endpoint".to_owned())?,
                );
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
        return Err("usage: transcribe_document [--format markdown|json] [--output PATH] [--engine builtin|vertex-gemini] [--project PROJECT] [--location LOCATION] [--model MODEL] [--access-token TOKEN] [--endpoint URL] <document>".to_owned());
    };

    let document = match engine.as_str() {
        "builtin" => transcribe_document_path(&path).map_err(|err| err.to_string())?,
        "vertex-gemini" => transcribe_document_path_with_vertex(
            &path,
            &VertexGeminiConfig {
                project_id: project_id
                    .or_else(|| std::env::var("VERTEX_PROJECT_ID").ok())
                    .ok_or_else(|| "Vertex Gemini requires --project or VERTEX_PROJECT_ID".to_owned())?,
                location: location
                    .or_else(|| std::env::var("VERTEX_LOCATION").ok())
                    .ok_or_else(|| "Vertex Gemini requires --location or VERTEX_LOCATION".to_owned())?,
                model: model
                    .or_else(|| std::env::var("VERTEX_GEMINI_MODEL").ok())
                    .unwrap_or_else(|| "gemini-2.5-flash".to_owned()),
                access_token: access_token.or_else(|| std::env::var("VERTEX_ACCESS_TOKEN").ok()),
                endpoint_override: endpoint_override.or_else(|| std::env::var("VERTEX_ENDPOINT_OVERRIDE").ok()),
            },
        )
        .map_err(|err| err.to_string())?,
        _ => return Err("engine must be one of builtin | vertex-gemini".to_owned()),
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
