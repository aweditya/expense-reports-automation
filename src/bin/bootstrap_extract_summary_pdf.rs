use std::env;
use std::fs;
use std::process::ExitCode;

use expense_report_schema::{
    extract_stanford_summary_path, render_draft_report_json_pretty, render_draft_report_yaml,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Yaml,
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
    let mut format = OutputFormat::Yaml;
    let mut output_path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --format".to_owned())?;
                format = match value.as_str() {
                    "yaml" => OutputFormat::Yaml,
                    "json" => OutputFormat::Json,
                    _ => {
                        return Err(format!(
                            "unsupported output format {value:?}; expected yaml or json"
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
            _ if arg.starts_with("--") => return Err(format!("unknown flag {arg:?}")),
            _ => {
                if path.replace(arg).is_some() {
                    return Err("expected exactly one input PDF path".to_owned());
                }
            }
        }
    }

    let Some(path) = path else {
        return Err(
            "usage: bootstrap_extract_summary_pdf [--format yaml|json] [--output PATH] <pdf>"
                .to_owned(),
        );
    };

    let draft = extract_stanford_summary_path(&path).map_err(|err| err.to_string())?;
    let rendered = match format {
        OutputFormat::Yaml => render_draft_report_yaml(&draft).map_err(|err| err.to_string())?,
        OutputFormat::Json => {
            render_draft_report_json_pretty(&draft).map_err(|err| err.to_string())?
        }
    };

    if let Some(output_path) = output_path {
        fs::write(output_path, rendered).map_err(|err| err.to_string())?;
    } else {
        print!("{rendered}");
    }

    Ok(())
}
