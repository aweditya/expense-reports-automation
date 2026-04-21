use expense_report_schema::{
    build_review_packet_with_ocr_artifacts, load_review_submission_ledger_path,
    render_fa_workbench_html, render_review_preview_html, render_review_workbench_html,
};

fn main() {
    match run() {
        Ok(rendered) => println!("{rendered}"),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<String, String> {
    let mut args = std::env::args().skip(1);
    let mut artifacts_dir = None;
    let mut surface = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--artifacts-dir" => {
                artifacts_dir = Some(args.next().ok_or_else(usage)?);
            }
            "--surface" => {
                surface = Some(args.next().ok_or_else(usage)?);
            }
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
    }

    let artifacts_dir = artifacts_dir.ok_or_else(usage)?;
    let surface = surface.unwrap_or_else(|| "developer".to_owned());
    let ledger_path = std::path::Path::new(&artifacts_dir).join("ledger.json");
    let ledger = load_review_submission_ledger_path(&ledger_path)
        .map_err(|err| format!("failed to load ledger {}: {err}", ledger_path.display()))?;
    let current_version = ledger
        .draft_versions
        .iter()
        .find(|version| version.version_id == ledger.summary.current_draft_version_id)
        .ok_or_else(|| {
            format!(
                "current draft version {} was not found in {}",
                ledger.summary.current_draft_version_id,
                ledger_path.display()
            )
        })?;
    let packet = build_review_packet_with_ocr_artifacts(
        &ledger.bundle,
        &current_version.draft,
        &current_version.readiness,
        &ledger.ocr_pass_comparisons,
        &ledger.ocr_groundings,
    )
    .map_err(|err| format!("failed to rebuild review packet: {err}"))?;

    match surface.as_str() {
        "fa" | "workbench" => Ok(render_fa_workbench_html(&packet)),
        "developer" => Ok(render_review_workbench_html(&packet)),
        "preview" => Ok(render_review_preview_html(&packet)),
        other => Err(format!("unknown surface: {other}\n{}", usage())),
    }
}

fn usage() -> String {
    "usage: render_current_review_surface --artifacts-dir <dir> [--surface fa|developer|preview]"
        .to_owned()
}
