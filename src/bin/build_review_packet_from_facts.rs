use std::process::ExitCode;

use expense_report_schema::{
    build_review_packet, parse_document_facts_json_path, render_review_packet_json_pretty,
    render_review_packet_markdown, synthesize_bundle_projection, synthesize_bundle_projection_with_fx,
    StaticFxRateProvider,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FxMode {
    None,
    Demo,
}

impl FxMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "demo" => Some(Self::Demo),
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
    let mut output_format = OutputFormat::Markdown;
    let mut fx_mode = FxMode::None;
    let mut input_paths = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --output".to_owned())?;
                output_format = OutputFormat::parse(&value)
                    .ok_or_else(|| "output format must be one of markdown | json".to_owned())?;
            }
            "--fx" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --fx".to_owned())?;
                fx_mode = FxMode::parse(&value)
                    .ok_or_else(|| "FX mode must be one of none | demo".to_owned())?;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: build_review_packet_from_facts [--output markdown|json] [--fx none|demo] <facts.json>..."
                        .to_owned(),
                );
            }
            other => input_paths.push(other.to_owned()),
        }
    }

    if input_paths.is_empty() {
        return Err(
            "usage: build_review_packet_from_facts [--output markdown|json] [--fx none|demo] <facts.json>..."
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

    let demo_fx_provider = StaticFxRateProvider::demo();
    let projection = match fx_mode {
        FxMode::None => synthesize_bundle_projection(&documents),
        FxMode::Demo => synthesize_bundle_projection_with_fx(&documents, &demo_fx_provider),
    };
    let packet = build_review_packet(&projection.bundle, &projection.draft, &projection.validation)
        .map_err(|err| format!("failed to build review packet: {err}"))?;

    let rendered = match output_format {
        OutputFormat::Markdown => render_review_packet_markdown(&packet),
        OutputFormat::Json => render_review_packet_json_pretty(&packet)
            .map_err(|err| format!("failed to render review packet json: {err}"))?,
    };

    println!("{rendered}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{FxMode, OutputFormat};

    #[test]
    fn parses_output_formats() {
        assert_eq!(OutputFormat::parse("markdown"), Some(OutputFormat::Markdown));
        assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("yaml"), None);
    }

    #[test]
    fn parses_fx_modes() {
        assert_eq!(FxMode::parse("none"), Some(FxMode::None));
        assert_eq!(FxMode::parse("demo"), Some(FxMode::Demo));
        assert_eq!(FxMode::parse("live"), None);
    }
}
