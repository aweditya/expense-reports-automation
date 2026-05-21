//! Render the new workbench HTML from a typed ExpenseReport JSON plus the
//! per-receipt extraction outputs.
//!
//! Args (all optional, sensible defaults for local CLI use):
//!   --report <path>             JSON for the reduced ExpenseReport.
//!                               Default: .scratch/reduced/report.json
//!   --receipts-dir <path>       Directory of per-receipt extraction
//!                               JSONs (one [ExtractedReceipt] array
//!                               per file). Default: .scratch/spike
//!   --out <path>                Where to write the rendered HTML.
//!                               Default: .scratch/spike/workbench.html
//!   --source-docs-url-prefix    URL prefix the workbench uses for
//!     <prefix>                  spot-check links. Default: "files/"
//!                               (matches Flask's /uploads/<id>/files/
//!                               route). For local CLI rendering the
//!                               source documents typically live in
//!                               receipts/ at the repo root; pass
//!                               "../../receipts/" when rendering to
//!                               .scratch/spike/ so spot-check links
//!                               resolve.
//!   --csv-domestic-out <path>   Where to write the domestic-portal CSV
//!                               (7 columns, plain expense-type names).
//!   --csv-foreign-out <path>    Where to write the foreign-portal CSV
//!                               (20 columns, suffixed expense-type names).
//!                               Both CSVs are optional; either file may
//!                               be header-only when the report has no
//!                               lines routed to that portal page.
//!
//! Validation runs via validate_typed against the reduced report.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use expense_report_schema::csv_export::{report_to_domestic_csv, report_to_foreign_csv};
use expense_report_schema::expense_report_model::ExpenseReport;
use expense_report_schema::extracted_receipt::ExtractedReceipt;
use expense_report_schema::validator_typed::validate_typed;
use expense_report_schema::workbench_simple::render_workbench_html;

struct Args {
    report: PathBuf,
    receipts_dir: PathBuf,
    out: PathBuf,
    source_docs_url_prefix: String,
    csv_domestic_out: Option<PathBuf>,
    csv_foreign_out: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args {
        report: PathBuf::from(".scratch/reduced/report.json"),
        receipts_dir: PathBuf::from(".scratch/spike"),
        out: PathBuf::from(".scratch/spike/workbench.html"),
        source_docs_url_prefix: String::from("files/"),
        csv_domestic_out: None,
        csv_foreign_out: None,
    };

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = argv.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--report" => args.report = PathBuf::from(iter.next().expect("--report needs path")),
            "--receipts-dir" => {
                args.receipts_dir = PathBuf::from(iter.next().expect("--receipts-dir needs path"))
            }
            "--out" => args.out = PathBuf::from(iter.next().expect("--out needs path")),
            "--source-docs-url-prefix" => {
                args.source_docs_url_prefix = iter
                    .next()
                    .expect("--source-docs-url-prefix needs a value")
                    .clone();
            }
            "--csv-domestic-out" => {
                args.csv_domestic_out =
                    Some(PathBuf::from(iter.next().expect("--csv-domestic-out needs path")))
            }
            "--csv-foreign-out" => {
                args.csv_foreign_out =
                    Some(PathBuf::from(iter.next().expect("--csv-foreign-out needs path")))
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    args
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

fn write_aux(path: &Path, contents: &str, label: &str) -> Result<(), ExitCode> {
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!("error: {label} create_dir_all {}: {err}", parent.display());
            return Err(ExitCode::from(2));
        }
    }
    if let Err(err) = fs::write(path, contents) {
        eprintln!("error: {label} write {}: {err}", path.display());
        return Err(ExitCode::from(2));
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = parse_args();

    let receipts = match read_receipts(&args.receipts_dir) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };

    let report_json = match fs::read_to_string(&args.report) {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "error: read {}: {err} — run reduce_extractions first",
                args.report.display()
            );
            return ExitCode::from(1);
        }
    };
    let report: ExpenseReport = match serde_json::from_str(&report_json) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: parse {}: {err}", args.report.display());
            return ExitCode::from(1);
        }
    };

    // M7.d.1: typed validator runs against the typed ExpenseReport directly,
    // walking the tree and looking up FIELD_RULES + CONDITIONAL_RULES per path.
    let validation = validate_typed(&report);

    let html = render_workbench_html(&report, &receipts, &validation, &args.source_docs_url_prefix);

    if let Err(code) = write_aux(&args.out, &html, "workbench") {
        return code;
    }

    // Alongside workbench.html, emit one CSV per Stanford portal page
    // (domestic vs foreign). Per-line routing inside csv_export decides
    // which file each transaction line lands in. Either file may be
    // header-only (typical: a purely-domestic report → empty foreign
    // CSV); the workbench hero hides downloads with zero data rows.
    if let Some(path) = &args.csv_domestic_out {
        let csv = report_to_domestic_csv(&report);
        if let Err(code) = write_aux(path, &csv, "csv-domestic") {
            return code;
        }
    }
    if let Some(path) = &args.csv_foreign_out {
        let csv = report_to_foreign_csv(&report);
        if let Err(code) = write_aux(path, &csv, "csv-foreign") {
            return code;
        }
    }

    println!(
        "wrote {} ({} receipts, {} transaction lines)",
        args.out.display(),
        receipts.len(),
        report
            .transaction_lines
            .as_ref()
            .map(|v| v.len())
            .unwrap_or(0)
    );
    ExitCode::SUCCESS
}
