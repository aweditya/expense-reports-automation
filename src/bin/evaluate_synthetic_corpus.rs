use std::process::ExitCode;

use expense_report_schema::{
    evaluate_synthetic_corpus, render_synthetic_corpus_evaluation_json_pretty,
    render_synthetic_corpus_evaluation_markdown,
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
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args = std::env::args().skip(1);
    let mut packet_count = 256usize;
    let mut output_format = OutputFormat::Markdown;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--packets" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --packets".to_owned())?;
                packet_count = value
                    .parse::<usize>()
                    .map_err(|_| "packet count must be a positive integer".to_owned())?;
            }
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --output".to_owned())?;
                output_format = OutputFormat::parse(&value)
                    .ok_or_else(|| "output format must be one of markdown | json".to_owned())?;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: evaluate_synthetic_corpus [--packets <count>] [--output markdown|json]"
                        .to_owned(),
                )
            }
            _ => return Err(format!("unknown flag {arg:?}")),
        }
    }

    let report = evaluate_synthetic_corpus(packet_count);
    let rendered = match output_format {
        OutputFormat::Markdown => render_synthetic_corpus_evaluation_markdown(&report),
        OutputFormat::Json => render_synthetic_corpus_evaluation_json_pretty(&report)
            .map_err(|err| err.to_string())?,
    };
    println!("{rendered}");

    if report.is_clean() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

#[cfg(test)]
mod tests {
    use super::OutputFormat;

    #[test]
    fn parses_output_formats() {
        assert_eq!(
            OutputFormat::parse("markdown"),
            Some(OutputFormat::Markdown)
        );
        assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("yaml"), None);
    }
}
