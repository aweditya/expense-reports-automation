use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use expense_report_schema::{
    bundle_regression_cases, bundle_regression_root, render_canonical_bundle_json_pretty,
    render_draft_report_json_pretty, render_validation_report_json_pretty,
    run_bundle_regression_case,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct BundleRegressionManifestEntry {
    id: String,
    fx_mode: String,
    fact_paths: Vec<String>,
    expected_bundle_path: String,
    expected_draft_path: String,
    expected_validation_path: String,
}

fn main() -> ExitCode {
    if let Err(err) = export_bundle_regression_expectations() {
        eprintln!("failed to export bundle regression expectations: {err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

fn export_bundle_regression_expectations() -> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = Vec::new();

    for case in bundle_regression_cases() {
        let result = run_bundle_regression_case(&case)?;
        let bundle_path = repo_root().join(case.expected_bundle_relative_path());
        let draft_path = repo_root().join(case.expected_draft_relative_path());
        let validation_path = repo_root().join(case.expected_validation_relative_path());

        write_output(&bundle_path, &render_canonical_bundle_json_pretty(&result.bundle)?)?;
        write_output(&draft_path, &render_draft_report_json_pretty(&result.draft)?)?;
        write_output(
            &validation_path,
            &render_validation_report_json_pretty(&result.validation)?,
        )?;

        manifest.push(BundleRegressionManifestEntry {
            id: case.id.to_owned(),
            fx_mode: match case.fx_mode {
                expense_report_schema::BundleRegressionFxMode::None => "none".to_owned(),
                expense_report_schema::BundleRegressionFxMode::Demo => "demo".to_owned(),
            },
            fact_paths: case.fact_paths.iter().map(|path| (*path).to_owned()).collect(),
            expected_bundle_path: case.expected_bundle_relative_path(),
            expected_draft_path: case.expected_draft_relative_path(),
            expected_validation_path: case.expected_validation_relative_path(),
        });
    }

    let manifest_path = bundle_regression_root().join("manifest.json");
    let rendered_manifest = serde_json::to_string_pretty(&manifest)?;
    write_output(&manifest_path, &rendered_manifest)?;

    Ok(())
}

fn write_output(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
