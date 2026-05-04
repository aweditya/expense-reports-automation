//! Render the new workbench HTML from a typed ExpenseReport JSON plus the
//! per-receipt extraction outputs.
//!
//! Args (all optional, sensible defaults for local CLI use):
//!   --report <path>         JSON for the reduced ExpenseReport.
//!                           Default: .scratch/reduced/report.json
//!   --receipts-dir <path>   Directory of per-receipt extraction JSONs
//!                           (one [ExtractedReceipt] array per file).
//!                           Default: .scratch/spike
//!   --out <path>            Where to write the rendered HTML.
//!                           Default: .scratch/spike/workbench.html
//!                           (alongside the extractions, so relative
//!                           "Download JSON" links resolve when opened
//!                           via file://)
//!
//! Validation runs via validate_typed against the reduced report.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use expense_report_schema::expense_report_model::ExpenseReport;
use expense_report_schema::extracted_receipt::ExtractedReceipt;
use expense_report_schema::validator_typed::validate_typed;
use expense_report_schema::workbench_simple::render_workbench_html;

fn parse_args() -> (PathBuf, PathBuf, PathBuf) {
    let mut report = PathBuf::from(".scratch/reduced/report.json");
    let mut receipts_dir = PathBuf::from(".scratch/spike");
    let mut out = PathBuf::from(".scratch/spike/workbench.html");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--report" => report = PathBuf::from(iter.next().expect("--report needs path")),
            "--receipts-dir" => {
                receipts_dir = PathBuf::from(iter.next().expect("--receipts-dir needs path"))
            }
            "--out" => out = PathBuf::from(iter.next().expect("--out needs path")),
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    (report, receipts_dir, out)
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
            .map_err(|err| format!("parse {}: {err}", path.display()))?;
        if let Some(first) = parsed.into_iter().next() {
            receipts.push(first);
        }
    }
    Ok(receipts)
}

fn main() -> ExitCode {
    let (report_path, receipts_dir, out_path) = parse_args();

    let receipts = match read_receipts(&receipts_dir) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };

    let report_json = match fs::read_to_string(&report_path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "error: read {}: {err} — run reduce_extractions first",
                report_path.display()
            );
            return ExitCode::from(1);
        }
    };
    let report: ExpenseReport = match serde_json::from_str(&report_json) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: parse {}: {err}", report_path.display());
            return ExitCode::from(1);
        }
    };

    // M7.d.1: typed validator runs against the typed ExpenseReport directly,
    // walking the tree and looking up FIELD_RULES + CONDITIONAL_RULES per path.
    let validation = validate_typed(&report);

    let html = render_workbench_html(&report, &receipts, &validation);

    if let Some(parent) = out_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!("error: create_dir_all {}: {err}", parent.display());
            return ExitCode::from(2);
        }
    }
    if let Err(err) = fs::write(&out_path, html) {
        eprintln!("error: write {}: {err}", out_path.display());
        return ExitCode::from(2);
    }
    println!(
        "wrote {} ({} receipts, {} transaction lines)",
        out_path.display(),
        receipts.len(),
        report
            .transaction_lines
            .as_ref()
            .map(|v| v.len())
            .unwrap_or(0)
    );
    ExitCode::SUCCESS
}
