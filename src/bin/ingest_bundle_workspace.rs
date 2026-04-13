use std::path::PathBuf;
use std::process::ExitCode;

use expense_report_schema::{
    load_bundle_workspace_manifest, render_bundle_workspace_manifest_json_pretty,
    run_staged_bundle, stage_and_run_bundle, stage_bundle_uploads, IngestionFxMode,
    IngestionTranscriber, StageBundleConfig, VertexGeminiConfig, VertexGeminiSdkConfig,
    WorkspaceRunConfig,
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
    let Some(subcommand) = args.next() else {
        return Err(usage().to_owned());
    };

    match subcommand.as_str() {
        "stage" => run_stage(args.collect()),
        "run" => run_run(args.collect()),
        "stage-and-run" => run_stage_and_run(args.collect()),
        "status" => run_status(args.collect()),
        "--help" | "-h" => Err(usage().to_owned()),
        _ => Err(usage().to_owned()),
    }
}

fn run_stage(args: Vec<String>) -> Result<(), String> {
    let mut parsed = ParsedArgs::default();
    parse_args(&args, &mut parsed)?;
    let workspace_root = parsed
        .workspace_root
        .clone()
        .ok_or_else(|| usage().to_owned())?;
    if parsed.input_paths.is_empty() {
        return Err(usage().to_owned());
    }

    let manifest = stage_bundle_uploads(
        workspace_root,
        &parsed.input_paths,
        &StageBundleConfig {
            bundle_id: parsed.bundle_id,
            user_id: parsed.user_id,
        },
    )
    .map_err(|err| err.to_string())?;

    println!(
        "staged bundle {}, documents={}, stage={:?}",
        manifest.bundle_id,
        manifest.documents.len(),
        manifest.current_stage
    );
    Ok(())
}

fn run_run(args: Vec<String>) -> Result<(), String> {
    let mut parsed = ParsedArgs::default();
    parse_args(&args, &mut parsed)?;
    let workspace_root = parsed
        .workspace_root
        .clone()
        .ok_or_else(|| usage().to_owned())?;
    let bundle_id = parsed.bundle_id.clone().ok_or_else(|| usage().to_owned())?;
    let transcriber = resolve_transcriber(&parsed)?;

    let result = run_staged_bundle(
        workspace_root,
        &bundle_id,
        &WorkspaceRunConfig {
            run_id: parsed.run_id,
            transcriber,
            fx_mode: parsed.fx_mode,
        },
    )
    .map_err(|err| err.to_string())?;

    println!(
        "processed bundle {}, run={}, filing_status={:?}, ledger_state={:?}",
        result.manifest.bundle_id,
        result.run.run_id,
        result.run.filing_status,
        result.run.ledger_state
    );
    Ok(())
}

fn run_stage_and_run(args: Vec<String>) -> Result<(), String> {
    let mut parsed = ParsedArgs::default();
    parse_args(&args, &mut parsed)?;
    let workspace_root = parsed
        .workspace_root
        .clone()
        .ok_or_else(|| usage().to_owned())?;
    if parsed.input_paths.is_empty() {
        return Err(usage().to_owned());
    }
    let transcriber = resolve_transcriber(&parsed)?;

    let result = stage_and_run_bundle(
        workspace_root,
        &parsed.input_paths,
        &StageBundleConfig {
            bundle_id: parsed.bundle_id,
            user_id: parsed.user_id,
        },
        &WorkspaceRunConfig {
            run_id: parsed.run_id,
            transcriber,
            fx_mode: parsed.fx_mode,
        },
    )
    .map_err(|err| err.to_string())?;

    println!(
        "processed bundle {}, run={}, documents={}, filing_status={:?}, ledger_state={:?}",
        result.manifest.bundle_id,
        result.run.run_id,
        result.pipeline.transcriptions.len(),
        result.run.filing_status,
        result.run.ledger_state
    );
    Ok(())
}

fn run_status(args: Vec<String>) -> Result<(), String> {
    let mut parsed = ParsedArgs::default();
    parse_args(&args, &mut parsed)?;
    let workspace_root = parsed
        .workspace_root
        .clone()
        .ok_or_else(|| usage().to_owned())?;
    let bundle_id = parsed.bundle_id.clone().ok_or_else(|| usage().to_owned())?;
    let manifest =
        load_bundle_workspace_manifest(workspace_root, &bundle_id).map_err(|err| err.to_string())?;

    match parsed.output_format.as_deref().unwrap_or("summary") {
        "summary" => {
            println!("bundle_id={}", manifest.bundle_id);
            println!("documents={}", manifest.documents.len());
            println!("runs={}", manifest.runs.len());
            println!("stage={:?}", manifest.current_stage);
            if let Some(run_id) = manifest.latest_run_id {
                println!("latest_run_id={run_id}");
            }
        }
        "json" => {
            println!(
                "{}",
                render_bundle_workspace_manifest_json_pretty(&manifest)
                    .map_err(|err| err.to_string())?
            );
        }
        _ => return Err("status format must be one of summary | json".to_owned()),
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedArgs {
    workspace_root: Option<PathBuf>,
    bundle_id: Option<String>,
    run_id: Option<String>,
    user_id: Option<String>,
    output_format: Option<String>,
    fx_mode: IngestionFxMode,
    engine: Option<String>,
    project_id: Option<String>,
    location: Option<String>,
    model: Option<String>,
    access_token: Option<String>,
    service_account_key_path: Option<String>,
    endpoint_override: Option<String>,
    token_endpoint_override: Option<String>,
    sdk_python: Option<String>,
    sdk_script: Option<String>,
    input_paths: Vec<PathBuf>,
}

impl Default for ParsedArgs {
    fn default() -> Self {
        Self {
            workspace_root: None,
            bundle_id: None,
            run_id: None,
            user_id: None,
            output_format: None,
            fx_mode: IngestionFxMode::None,
            engine: None,
            project_id: None,
            location: None,
            model: None,
            access_token: None,
            service_account_key_path: None,
            endpoint_override: None,
            token_endpoint_override: None,
            sdk_python: None,
            sdk_script: None,
            input_paths: Vec::new(),
        }
    }
}

fn parse_args(args: &[String], parsed: &mut ParsedArgs) -> Result<(), String> {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--workspace-root" => {
                index += 1;
                parsed.workspace_root = Some(PathBuf::from(required_value(args, index, arg)?));
            }
            "--bundle-id" => {
                index += 1;
                parsed.bundle_id = Some(required_value(args, index, arg)?.to_owned());
            }
            "--run-id" => {
                index += 1;
                parsed.run_id = Some(required_value(args, index, arg)?.to_owned());
            }
            "--user-id" => {
                index += 1;
                parsed.user_id = Some(required_value(args, index, arg)?.to_owned());
            }
            "--format" => {
                index += 1;
                parsed.output_format = Some(required_value(args, index, arg)?.to_owned());
            }
            "--fx" => {
                index += 1;
                parsed.fx_mode = match required_value(args, index, arg)? {
                    "none" => IngestionFxMode::None,
                    "demo" => IngestionFxMode::Demo,
                    _ => return Err("FX mode must be one of none | demo".to_owned()),
                };
            }
            "--engine" => {
                index += 1;
                parsed.engine = Some(required_value(args, index, arg)?.to_owned());
            }
            "--project" => {
                index += 1;
                parsed.project_id = Some(required_value(args, index, arg)?.to_owned());
            }
            "--location" => {
                index += 1;
                parsed.location = Some(required_value(args, index, arg)?.to_owned());
            }
            "--model" => {
                index += 1;
                parsed.model = Some(required_value(args, index, arg)?.to_owned());
            }
            "--access-token" => {
                index += 1;
                parsed.access_token = Some(required_value(args, index, arg)?.to_owned());
            }
            "--service-account-key" => {
                index += 1;
                parsed.service_account_key_path = Some(required_value(args, index, arg)?.to_owned());
            }
            "--endpoint" => {
                index += 1;
                parsed.endpoint_override = Some(required_value(args, index, arg)?.to_owned());
            }
            "--token-endpoint" => {
                index += 1;
                parsed.token_endpoint_override = Some(required_value(args, index, arg)?.to_owned());
            }
            "--sdk-python" => {
                index += 1;
                parsed.sdk_python = Some(required_value(args, index, arg)?.to_owned());
            }
            "--sdk-script" => {
                index += 1;
                parsed.sdk_script = Some(required_value(args, index, arg)?.to_owned());
            }
            "--help" | "-h" => return Err(usage().to_owned()),
            other if other.starts_with("--") => return Err(format!("unknown flag {other:?}")),
            other => parsed.input_paths.push(PathBuf::from(other)),
        }
        index += 1;
    }
    Ok(())
}

fn required_value<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn resolve_transcriber(parsed: &ParsedArgs) -> Result<IngestionTranscriber, String> {
    match parsed.engine.as_deref().unwrap_or("builtin") {
        "builtin" => Ok(IngestionTranscriber::Builtin),
        "vertex-gemini" => Ok(IngestionTranscriber::VertexGemini(
            VertexGeminiConfig::resolve_from_sources(
                parsed.project_id.clone(),
                parsed.location.clone(),
                parsed.model.clone(),
                parsed.access_token.clone(),
                parsed.service_account_key_path.clone().map(Into::into),
                parsed.endpoint_override.clone(),
                parsed.token_endpoint_override.clone(),
            )
            .map_err(|err| err.to_string())?,
        )),
        "vertex-gemini-sdk" => Ok(IngestionTranscriber::VertexGeminiSdk(
            VertexGeminiSdkConfig::resolve_from_sources(
                parsed.project_id.clone(),
                parsed.location.clone(),
                parsed.model.clone(),
                parsed.service_account_key_path.clone().map(Into::into),
                parsed.sdk_python.clone().map(Into::into),
                parsed.sdk_script.clone().map(Into::into),
            )
            .map_err(|err| err.to_string())?,
        )),
        _ => Err("engine must be one of builtin | vertex-gemini | vertex-gemini-sdk".to_owned()),
    }
}

fn usage() -> &'static str {
    "usage:
  ingest_bundle_workspace stage --workspace-root <dir> [--bundle-id ID] [--user-id USER] <document>...
  ingest_bundle_workspace run --workspace-root <dir> --bundle-id ID [--run-id ID] [--fx none|demo] [--engine builtin|vertex-gemini|vertex-gemini-sdk] [--project PROJECT] [--location LOCATION] [--model MODEL] [--access-token TOKEN] [--service-account-key PATH] [--endpoint URL] [--token-endpoint URL] [--sdk-python PATH] [--sdk-script PATH]
  ingest_bundle_workspace stage-and-run --workspace-root <dir> [--bundle-id ID] [--user-id USER] [--run-id ID] [--fx none|demo] [--engine builtin|vertex-gemini|vertex-gemini-sdk] [--project PROJECT] [--location LOCATION] [--model MODEL] [--access-token TOKEN] [--service-account-key PATH] [--endpoint URL] [--token-endpoint URL] [--sdk-python PATH] [--sdk-script PATH] <document>...
  ingest_bundle_workspace status --workspace-root <dir> --bundle-id ID [--format summary|json]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_format() {
        let args = vec![
            "--workspace-root".to_owned(),
            "/tmp/demo".to_owned(),
            "--bundle-id".to_owned(),
            "bundle_1".to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ];
        let mut parsed = ParsedArgs::default();
        parse_args(&args, &mut parsed).expect("args should parse");
        assert_eq!(parsed.output_format.as_deref(), Some("json"));
    }

    #[test]
    fn resolves_builtin_transcriber_by_default() {
        let parsed = ParsedArgs::default();
        let transcriber = resolve_transcriber(&parsed).expect("builtin should resolve");
        assert_eq!(transcriber, IngestionTranscriber::Builtin);
    }

    #[test]
    fn rejects_unknown_engine() {
        let parsed = ParsedArgs {
            engine: Some("mystery-engine".to_owned()),
            ..ParsedArgs::default()
        };
        assert!(resolve_transcriber(&parsed).is_err());
    }
}
