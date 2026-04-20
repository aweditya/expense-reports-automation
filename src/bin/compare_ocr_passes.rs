use std::env;
use std::fs;
use std::process::ExitCode;

use expense_report_schema::{
    compare_ocr_passes_json_paths, render_ocr_comparison_html, render_ocr_comparison_json_pretty,
    render_ocr_comparison_markdown,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Markdown,
    Html,
}

impl OutputFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "json" => Ok(Self::Json),
            "markdown" | "md" => Ok(Self::Markdown),
            "html" => Ok(Self::Html),
            _ => Err(format!(
                "unsupported format {value:?}; expected json, markdown, or html"
            )),
        }
    }
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
    let mut format = OutputFormat::Markdown;
    let mut output_path: Option<String> = None;
    let mut paths = Vec::<String>::new();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --format".to_owned())?;
                format = OutputFormat::parse(&value)?;
            }
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --output".to_owned())?;
                output_path = Some(value);
            }
            "--help" | "-h" => {
                return Err(usage());
            }
            _ if argument.starts_with("--") => {
                return Err(format!("unknown argument: {argument}\n\n{}", usage()));
            }
            _ => paths.push(argument),
        }
    }

    if paths.len() < 2 {
        return Err(usage());
    }

    let comparison = compare_ocr_passes_json_paths(&paths).map_err(|err| err.to_string())?;
    let rendered = match format {
        OutputFormat::Json => {
            render_ocr_comparison_json_pretty(&comparison).map_err(|err| err.to_string())?
        }
        OutputFormat::Markdown => render_ocr_comparison_markdown(&comparison),
        OutputFormat::Html => render_ocr_comparison_html(&comparison),
    };

    if let Some(path) = output_path {
        fs::write(path, rendered).map_err(|err| err.to_string())?;
    } else {
        print!("{rendered}");
    }

    Ok(())
}

fn usage() -> String {
    "usage: compare_ocr_passes [--format json|markdown|html] [--output <path>] <pass-json> <pass-json> [more-pass-json ...]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_output_formats() {
        assert_eq!(OutputFormat::parse("json").unwrap(), OutputFormat::Json);
        assert_eq!(
            OutputFormat::parse("markdown").unwrap(),
            OutputFormat::Markdown
        );
        assert_eq!(OutputFormat::parse("md").unwrap(), OutputFormat::Markdown);
        assert_eq!(OutputFormat::parse("html").unwrap(), OutputFormat::Html);
    }

    #[test]
    fn rejects_unknown_output_format() {
        let error = OutputFormat::parse("yaml").expect_err("yaml should be rejected");
        assert!(error.contains("expected json, markdown, or html"));
    }
}
