use std::path::PathBuf;
use std::process::ExitCode;

use expense_report_schema::{
    ingest_expense_documents, write_ingestion_artifacts, IngestionConfig, IngestionFxMode,
    IngestionTranscriber, VertexGeminiConfig,
};

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
    let mut args = std::env::args().skip(1);
    let mut output_dir = None;
    let mut bundle_id = None;
    let mut fx_mode = IngestionFxMode::None;
    let mut engine = "builtin".to_owned();
    let mut project_id = None;
    let mut location = None;
    let mut model = None;
    let mut access_token = None;
    let mut service_account_key_path = None;
    let mut endpoint_override = None;
    let mut token_endpoint_override = None;
    let mut input_paths = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output-dir" => {
                output_dir = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --output-dir".to_owned())?,
                );
            }
            "--bundle-id" => {
                bundle_id = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --bundle-id".to_owned())?,
                );
            }
            "--fx" => {
                let value = args.next().ok_or_else(|| "missing value for --fx".to_owned())?;
                fx_mode = match value.as_str() {
                    "none" => IngestionFxMode::None,
                    "demo" => IngestionFxMode::Demo,
                    _ => return Err("FX mode must be one of none | demo".to_owned()),
                };
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
            "--help" | "-h" => {
                return Err(
                    "usage: ingest_expense_documents --output-dir <dir> [--bundle-id ID] [--fx none|demo] [--engine builtin|vertex-gemini] [--project PROJECT] [--location LOCATION] [--model MODEL] [--access-token TOKEN] [--service-account-key PATH] [--endpoint URL] [--token-endpoint URL] <document>..."
                        .to_owned(),
                )
            }
            other if other.starts_with("--") => return Err(format!("unknown flag {other:?}")),
            other => input_paths.push(PathBuf::from(other)),
        }
    }

    let output_dir = output_dir
        .map(PathBuf::from)
        .ok_or_else(|| {
            "usage: ingest_expense_documents --output-dir <dir> [--bundle-id ID] [--fx none|demo] [--engine builtin|vertex-gemini] [--project PROJECT] [--location LOCATION] [--model MODEL] [--access-token TOKEN] [--service-account-key PATH] [--endpoint URL] [--token-endpoint URL] <document>..."
                .to_owned()
        })?;
    if input_paths.is_empty() {
        return Err(
            "usage: ingest_expense_documents --output-dir <dir> [--bundle-id ID] [--fx none|demo] [--engine builtin|vertex-gemini] [--project PROJECT] [--location LOCATION] [--model MODEL] [--access-token TOKEN] [--service-account-key PATH] [--endpoint URL] [--token-endpoint URL] <document>..."
                .to_owned(),
        );
    }

    let transcriber = match engine.as_str() {
        "builtin" => IngestionTranscriber::Builtin,
        "vertex-gemini" => IngestionTranscriber::VertexGemini(
            VertexGeminiConfig::resolve_from_sources(
                project_id,
                location,
                model,
                access_token,
                service_account_key_path.map(Into::into),
                endpoint_override,
                token_endpoint_override,
            )
            .map_err(|err| err.to_string())?,
        ),
        _ => return Err("engine must be one of builtin | vertex-gemini".to_owned()),
    };

    let result = ingest_expense_documents(
        &input_paths,
        &IngestionConfig {
            bundle_id,
            transcriber,
            fx_mode,
        },
    )
    .map_err(|err| err.to_string())?;
    write_ingestion_artifacts(&output_dir, &result).map_err(|err| err.to_string())?;

    println!("wrote ingestion artifacts to {}", output_dir.display());
    println!(
        "documents={}, filing_status={:?}, ledger_state={:?}",
        result.transcriptions.len(),
        result.review_packet.summary.filing_status,
        result.ledger.summary.current_state
    );
    Ok(())
}
