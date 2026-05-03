//! M7.b preview tool: read the existing spike outputs + reduced report,
//! render the new workbench HTML, write to `.scratch/workbench_preview/`.
//!
//! Throwaway — just for visual iteration on the renderer before M7.c
//! wires it into the production path. Run with `cargo run --bin
//! render_workbench_preview` after `scripts/spike_acceptance_check.py
//! --end-to-end` has populated `.scratch/`.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use expense_report_schema::expense_report_model::ExpenseReport;
use expense_report_schema::extracted_receipt::ExtractedReceipt;
use expense_report_schema::validator::ValidationReport;
use expense_report_schema::workbench_simple::render_workbench_html;

fn main() -> ExitCode {
    let spike_dir = Path::new(".scratch/spike");
    let report_path = Path::new(".scratch/reduced/report.json");
    let out_path = Path::new(".scratch/workbench_preview/index.html");

    let mut entries: Vec<_> = match fs::read_dir(spike_dir) {
        Ok(it) => it.filter_map(Result::ok).collect(),
        Err(err) => {
            eprintln!("read .scratch/spike: {err}");
            return ExitCode::from(1);
        }
    };
    entries.sort_by_key(|e| e.file_name());

    let mut receipts: Vec<ExtractedReceipt> = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let json = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) => {
                eprintln!("read {}: {err}", path.display());
                return ExitCode::from(1);
            }
        };
        let parsed: Vec<ExtractedReceipt> = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("parse {}: {err}", path.display());
                return ExitCode::from(1);
            }
        };
        if let Some(first) = parsed.into_iter().next() {
            receipts.push(first);
        }
    }

    let report: ExpenseReport = match fs::read_to_string(report_path) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("parse {}: {err}", report_path.display());
                return ExitCode::from(1);
            }
        },
        Err(err) => {
            eprintln!("read {}: {err} — run reduce_extractions first", report_path.display());
            return ExitCode::from(1);
        }
    };

    // Empty validation report for the preview — Pass 2 hookup is M7.c.
    let validation = ValidationReport { issues: vec![] };

    let html = render_workbench_html(&report, &receipts, &validation);

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).expect("create preview dir");
    }
    fs::write(out_path, html).expect("write preview");
    println!("wrote {} ({} receipts)", out_path.display(), receipts.len());
    ExitCode::SUCCESS
}
