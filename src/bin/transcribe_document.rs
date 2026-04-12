use std::env;
use std::fs;
use std::process::ExitCode;

use expense_report_schema::{
    render_transcribed_document_json_pretty, render_transcribed_document_markdown,
    transcribe_document_path,
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
        return Err("usage: transcribe_document [--format markdown|json] [--output PATH] <document>".to_owned());
    };

    let document = transcribe_document_path(&path).map_err(|err| err.to_string())?;
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
