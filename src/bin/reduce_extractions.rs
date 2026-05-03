//! Reduce a directory of per-receipt extraction JSONs into one ExpenseReport.
//!
//! Reads every `*.json` file under `--in`, deserializes each as a
//! single-element list of `ExtractedReceipt` (the shape Python's
//! spike_extract.py writes), and runs the reduction library over the
//! collected receipts. Writes the result to `--out` as pretty JSON.
//!
//! Pure I/O wrapper around `expense_report_schema::reduce`. All cross-
//! document logic lives in the library; this binary just walks the
//! directory and writes the file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use expense_report_schema::extracted_receipt::ExtractedReceipt;
use expense_report_schema::reduce::reduce_to_expense_report;

fn parse_args() -> (PathBuf, PathBuf) {
    let mut input = PathBuf::from(".scratch/spike");
    let mut output = PathBuf::from(".scratch/reduced/report.json");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--in" => {
                input = PathBuf::from(iter.next().expect("--in requires a path"));
            }
            "--out" => {
                output = PathBuf::from(iter.next().expect("--out requires a path"));
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    (input, output)
}

fn read_receipts(dir: &Path) -> Result<Vec<ExtractedReceipt>, String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|err| format!("read_dir({}): {err}", dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut receipts = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let json = fs::read_to_string(&path)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        let parsed: Vec<ExtractedReceipt> = serde_json::from_str(&json)
            .map_err(|err| format!("deserialize {}: {err}", path.display()))?;
        if parsed.is_empty() {
            return Err(format!("empty array in {}", path.display()));
        }
        // spike_extract.py writes a single-element list per file.
        receipts.push(parsed.into_iter().next().unwrap());
    }
    Ok(receipts)
}

fn main() -> ExitCode {
    let (input_dir, output_path) = parse_args();

    let receipts = match read_receipts(&input_dir) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    if receipts.is_empty() {
        eprintln!("error: no JSON files found under {}", input_dir.display());
        return ExitCode::from(1);
    }

    let report = reduce_to_expense_report(&receipts);

    if let Some(parent) = output_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!("error: create_dir_all {}: {err}", parent.display());
            return ExitCode::from(2);
        }
    }
    let serialized = match serde_json::to_string_pretty(&report) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: serialize report: {err}");
            return ExitCode::from(2);
        }
    };
    if let Err(err) = fs::write(&output_path, serialized) {
        eprintln!("error: write {}: {err}", output_path.display());
        return ExitCode::from(2);
    }

    let line_count = report
        .transaction_lines
        .as_ref()
        .map(|lines| lines.len())
        .unwrap_or(0);
    let total = report
        .transaction_summary
        .total_usd
        .unwrap_or(0.0);
    let earliest = report
        .transaction_summary
        .transaction_date
        .value
        .as_ref()
        .map(|d| d.0.as_str())
        .unwrap_or("?");
    let category = report
        .general_information
        .category
        .value
        .as_ref()
        .map(|c| c.as_str())
        .unwrap_or("?");

    println!(
        "wrote {} — {line_count} lines, total ${:.2}, earliest {earliest}, category {category}",
        output_path.display(),
        total,
    );
    ExitCode::SUCCESS
}
