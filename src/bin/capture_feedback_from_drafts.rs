use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde::de::DeserializeOwned;

use expense_report_schema::{
    capture_feedback, parse_correction_annotations_path, parse_draft_report_path,
    parse_submission_feedback_path, render_feedback_capture_json_pretty,
    render_feedback_capture_markdown, ValidationReport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Markdown,
    Json,
}

impl OutputFormat {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "markdown" => Some(Self::Markdown),
            "json" => Some(Self::Json),
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
    let mut original_path = None;
    let mut corrected_path = None;
    let mut annotations_path = None;
    let mut site_feedback_path = None;
    let mut validation_path = None;
    let mut output_format = OutputFormat::Markdown;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--original" => {
                original_path = Some(
                    args.next()
                        .ok_or_else(|| "missing value after --original".to_owned())?,
                );
            }
            "--corrected" => {
                corrected_path = Some(
                    args.next()
                        .ok_or_else(|| "missing value after --corrected".to_owned())?,
                );
            }
            "--annotations" => {
                annotations_path = Some(
                    args.next()
                        .ok_or_else(|| "missing value after --annotations".to_owned())?,
                );
            }
            "--site-feedback" => {
                site_feedback_path = Some(
                    args.next()
                        .ok_or_else(|| "missing value after --site-feedback".to_owned())?,
                );
            }
            "--validation" => {
                validation_path = Some(
                    args.next()
                        .ok_or_else(|| "missing value after --validation".to_owned())?,
                );
            }
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --output".to_owned())?;
                output_format = OutputFormat::parse(&value)
                    .ok_or_else(|| "output format must be one of markdown | json".to_owned())?;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: capture_feedback_from_drafts --original <draft.yaml> --corrected <draft.yaml> [--annotations <annotations.yaml>] [--site-feedback <feedback.yaml>] [--validation <validation.json>] [--output markdown|json]"
                        .to_owned(),
                );
            }
            other => return Err(format!("unexpected argument: {other}")),
        }
    }

    let original_path = original_path.ok_or_else(|| {
        "usage: capture_feedback_from_drafts --original <draft.yaml> --corrected <draft.yaml> [--annotations <annotations.yaml>] [--site-feedback <feedback.yaml>] [--validation <validation.json>] [--output markdown|json]"
            .to_owned()
    })?;
    let corrected_path = corrected_path.ok_or_else(|| {
        "usage: capture_feedback_from_drafts --original <draft.yaml> --corrected <draft.yaml> [--annotations <annotations.yaml>] [--site-feedback <feedback.yaml>] [--validation <validation.json>] [--output markdown|json]"
            .to_owned()
    })?;

    let original = parse_draft_report_path(&original_path)
        .map_err(|err| format!("failed to parse original draft {original_path}: {err}"))?;
    let corrected = parse_draft_report_path(&corrected_path)
        .map_err(|err| format!("failed to parse corrected draft {corrected_path}: {err}"))?;
    let annotations = annotations_path
        .as_deref()
        .map(parse_correction_annotations_path)
        .transpose()
        .map_err(|err| format!("failed to parse annotations: {err}"))?
        .unwrap_or_default();
    let site_feedback = site_feedback_path
        .as_deref()
        .map(parse_submission_feedback_path)
        .transpose()
        .map_err(|err| format!("failed to parse site feedback: {err}"))?;
    let validation = validation_path
        .as_deref()
        .map(parse_validation_report_path)
        .transpose()
        .map_err(|err| format!("failed to parse validation report: {err}"))?;

    let capture = capture_feedback(
        &original,
        &corrected,
        validation.as_ref(),
        &annotations,
        site_feedback,
    );

    let rendered = match output_format {
        OutputFormat::Markdown => render_feedback_capture_markdown(&capture),
        OutputFormat::Json => render_feedback_capture_json_pretty(&capture)
            .map_err(|err| format!("failed to render feedback capture json: {err}"))?,
    };

    println!("{rendered}");
    Ok(())
}

fn parse_validation_report_path(path: impl AsRef<Path>) -> Result<ValidationReport, String> {
    parse_json_or_yaml_path(path)
}

fn parse_json_or_yaml_path<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, String> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_str(&contents)
            .map_err(|err| format!("failed to parse {} as json: {err}", path.display())),
        Some("yaml") | Some("yml") => serde_yaml::from_str(&contents)
            .map_err(|err| format!("failed to parse {} as yaml: {err}", path.display())),
        _ => serde_json::from_str(&contents)
            .or_else(|_| serde_yaml::from_str(&contents))
            .map_err(|err| format!("failed to parse {} as json or yaml: {err}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::OutputFormat;

    #[test]
    fn parses_output_formats() {
        assert_eq!(OutputFormat::parse("markdown"), Some(OutputFormat::Markdown));
        assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("yaml"), None);
    }
}
