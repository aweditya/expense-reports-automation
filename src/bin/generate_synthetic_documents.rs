use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use expense_report_schema::{
    generate_synthetic_document, generate_synthetic_packet, render_document_facts_json_pretty,
    DocumentKind, SyntheticDocumentFixture, SyntheticVariant,
};
use serde_json::json;

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
    let mut output_dir = None;
    let mut variant = SyntheticVariant::Baseline;
    let mut kind = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output-dir" => {
                output_dir = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --output-dir".to_owned())?,
                );
            }
            "--variant" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --variant".to_owned())?;
                variant = match value.as_str() {
                    "baseline" => SyntheticVariant::Baseline,
                    "noisy" => SyntheticVariant::Noisy,
                    _ => {
                        return Err(format!(
                            "unsupported variant {value:?}; expected baseline or noisy"
                        ))
                    }
                };
            }
            "--kind" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --kind".to_owned())?;
                kind = Some(match value.as_str() {
                    "flight" => DocumentKind::FlightItinerary,
                    "hotel" => DocumentKind::HotelFolio,
                    "receipt" => DocumentKind::Receipt,
                    _ => {
                        return Err(format!(
                            "unsupported kind {value:?}; expected flight, hotel, or receipt"
                        ))
                    }
                });
            }
            _ => return Err(format!("unknown flag {arg:?}")),
        }
    }

    let output_dir = output_dir
        .map(PathBuf::from)
        .ok_or_else(|| "usage: generate_synthetic_documents --output-dir <dir> [--variant baseline|noisy] [--kind flight|hotel|receipt]".to_owned())?;

    fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;

    let fixtures = if let Some(kind) = kind {
        vec![generate_synthetic_document(kind, variant)]
    } else {
        generate_synthetic_packet(variant)
    };

    write_fixtures(&output_dir, &fixtures)?;

    let manifest = json!({
        "variant": variant.as_str(),
        "documents": fixtures.iter().map(|fixture| {
            json!({
                "kind": fixture.kind.as_str(),
                "filename": fixture.filename,
                "expected_facts_file": format!("{}.expected.json", fixture.filename),
            })
        }).collect::<Vec<_>>()
    });

    fs::write(
        output_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn write_fixtures(
    output_dir: &PathBuf,
    fixtures: &[SyntheticDocumentFixture],
) -> Result<(), String> {
    for fixture in fixtures {
        fs::write(output_dir.join(&fixture.filename), &fixture.markdown)
            .map_err(|err| err.to_string())?;
        fs::write(
            output_dir.join(format!("{}.expected.json", fixture.filename)),
            render_document_facts_json_pretty(&fixture.expected_facts)
                .map_err(|err| err.to_string())?,
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}
