use std::process::ExitCode;

use expense_report_schema::{
    parse_document_facts_json_path, render_canonical_bundle_json_pretty,
    render_draft_report_json_pretty, render_draft_report_yaml, synthesize_bundle_projection,
    BundleIssueSeverity, ValidationSeverity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    DraftYaml,
    DraftJson,
    BundleJson,
}

impl OutputFormat {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "draft-yaml" => Some(Self::DraftYaml),
            "draft-json" => Some(Self::DraftJson),
            "bundle-json" => Some(Self::BundleJson),
            _ => None,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut output_format = OutputFormat::DraftYaml;
    let mut input_paths = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --output".to_owned())?;
                output_format = OutputFormat::parse(&value).ok_or_else(|| {
                    "output format must be one of draft-yaml | draft-json | bundle-json".to_owned()
                })?;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: synthesize_bundle_from_facts [--output draft-yaml|draft-json|bundle-json] <facts.json>..."
                        .to_owned(),
                );
            }
            other => input_paths.push(other.to_owned()),
        }
    }

    if input_paths.is_empty() {
        return Err(
            "usage: synthesize_bundle_from_facts [--output draft-yaml|draft-json|bundle-json] <facts.json>..."
                .to_owned(),
        );
    }

    let mut documents = Vec::new();
    for path in &input_paths {
        documents.push(
            parse_document_facts_json_path(path)
                .map_err(|err| format!("failed to parse {path}: {err}"))?,
        );
    }

    let result = synthesize_bundle_projection(&documents);
    let output = match output_format {
        OutputFormat::DraftYaml => render_draft_report_yaml(&result.draft)
            .map_err(|err| format!("failed to render draft yaml: {err}"))?,
        OutputFormat::DraftJson => render_draft_report_json_pretty(&result.draft)
            .map_err(|err| format!("failed to render draft json: {err}"))?,
        OutputFormat::BundleJson => render_canonical_bundle_json_pretty(&result.bundle)
            .map_err(|err| format!("failed to render canonical bundle json: {err}"))?,
    };

    let synthesis_errors = result
        .issues
        .iter()
        .filter(|issue| issue.severity == BundleIssueSeverity::Error)
        .count();
    let synthesis_warnings = result
        .issues
        .iter()
        .filter(|issue| issue.severity == BundleIssueSeverity::Warning)
        .count();
    let validation_errors = result
        .validation
        .issues
        .iter()
        .filter(|issue| issue.severity == ValidationSeverity::Error)
        .count();
    let validation_warnings = result
        .validation
        .issues
        .iter()
        .filter(|issue| issue.severity == ValidationSeverity::Warning)
        .count();

    eprintln!(
        "bundle synthesis: {synthesis_errors} error(s), {synthesis_warnings} warning(s)"
    );
    eprintln!(
        "draft validation: {validation_errors} error(s), {validation_warnings} warning(s)"
    );
    println!("{output}");
    Ok(())
}
