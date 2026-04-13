use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use expense_report_schema::{
    generate_synthetic_corpus, render_document_facts_json_pretty,
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
    let mut args = std::env::args().skip(1);
    let mut output_dir = None;
    let mut packet_count = 64usize;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output-dir" => {
                output_dir = Some(
                    args.next()
                        .ok_or_else(|| "missing value for --output-dir".to_owned())?,
                );
            }
            "--packets" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --packets".to_owned())?;
                packet_count = value
                    .parse::<usize>()
                    .map_err(|_| "packet count must be a positive integer".to_owned())?;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: generate_synthetic_corpus --output-dir <dir> [--packets <count>]"
                        .to_owned(),
                )
            }
            _ => return Err(format!("unknown flag {arg:?}")),
        }
    }

    let output_dir = output_dir
        .map(PathBuf::from)
        .ok_or_else(|| {
            "usage: generate_synthetic_corpus --output-dir <dir> [--packets <count>]".to_owned()
        })?;
    fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;

    let packets = generate_synthetic_corpus(packet_count);
    let mut manifest_packets = Vec::new();

    for packet in packets {
        let packet_dir = output_dir.join(&packet.packet_id);
        fs::create_dir_all(&packet_dir).map_err(|err| err.to_string())?;

        let mut manifest_documents = Vec::new();
        for fixture in &packet.fixtures {
            fs::write(packet_dir.join(&fixture.filename), &fixture.markdown)
                .map_err(|err| err.to_string())?;
            fs::write(
                packet_dir.join(format!("{}.expected.json", fixture.filename)),
                render_document_facts_json_pretty(&fixture.expected_facts)
                    .map_err(|err| err.to_string())?,
            )
            .map_err(|err| err.to_string())?;
            manifest_documents.push(json!({
                "kind": fixture.kind.as_str(),
                "filename": fixture.filename,
                "expected_facts_file": format!("{}.expected.json", fixture.filename),
            }));
        }

        manifest_packets.push(json!({
            "packet_id": packet.packet_id,
            "variant": packet.variant,
            "scenario": packet.scenario.as_str(),
            "destination_city": packet.destination_city,
            "destination_country": packet.destination_country,
            "currency": packet.currency,
            "trip_window_mode": packet.trip_window_mode.as_str(),
            "stay_window_mode": packet.stay_window_mode.as_str(),
            "hotel_total_mode": packet.hotel_total_mode.as_str(),
            "receipt_total_mode": packet.receipt_total_mode.as_str(),
            "alcohol_receipt": packet.alcohol_receipt,
            "documents": manifest_documents,
        }));
    }

    let manifest = json!({
        "packet_count": packet_count,
        "packets": manifest_packets,
    });
    fs::write(
        output_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}
