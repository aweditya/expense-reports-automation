use std::process::ExitCode;

use expense_report_schema::{
    build_review_packet, parse_document_facts_json_path, render_review_workbench_html,
    synthesize_bundle_projection, synthesize_bundle_projection_with_fx, StaticFxRateProvider,
};

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
    let mut fx_mode = FxMode::None;
    let mut input_paths = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fx" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --fx".to_owned())?;
                fx_mode = FxMode::parse(&value)
                    .ok_or_else(|| "FX mode must be one of none | demo".to_owned())?;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: build_review_workbench_from_facts [--fx none|demo] <facts.json>..."
                        .to_owned(),
                );
            }
            other => input_paths.push(other.to_owned()),
        }
    }

    if input_paths.is_empty() {
        return Err(
            "usage: build_review_workbench_from_facts [--fx none|demo] <facts.json>...".to_owned(),
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
    let packet = build_review_packet(
        &projection.bundle,
        &projection.draft,
        &projection.validation,
    )
    .map_err(|err| format!("failed to build review packet: {err}"))?;

    println!("{}", render_review_workbench_html(&packet));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::FxMode;

    #[test]
    fn parses_fx_modes() {
        assert_eq!(FxMode::parse("none"), Some(FxMode::None));
        assert_eq!(FxMode::parse("demo"), Some(FxMode::Demo));
        assert_eq!(FxMode::parse("live"), None);
    }
}
