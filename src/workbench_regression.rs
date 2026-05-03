use std::fs;
use std::path::{Path, PathBuf};

use crate::bundle_regression::{
    bundle_regression_cases, run_bundle_regression_case, BundleRegressionCase,
};
use crate::review_packet::build_review_packet;
use crate::review_workbench::render_review_workbench_html;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchRegressionFailure {
    pub case_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkbenchRegressionVerificationReport {
    pub failures: Vec<WorkbenchRegressionFailure>,
}

impl WorkbenchRegressionVerificationReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn workbench_regression_root() -> PathBuf {
    repo_root().join("old/workbench_regressions")
}

pub fn verify_workbench_regressions() -> WorkbenchRegressionVerificationReport {
    let mut failures = Vec::new();

    for case in bundle_regression_cases() {
        match verify_workbench_regression_case(&case) {
            Ok(None) => {}
            Ok(Some(message)) => failures.push(WorkbenchRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
            Err(message) => failures.push(WorkbenchRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
        }
    }

    WorkbenchRegressionVerificationReport { failures }
}

pub fn export_workbench_regressions() -> Result<(), String> {
    for case in bundle_regression_cases() {
        let rendered = render_workbench_regression_case(&case)?;
        let path = repo_root().join(expected_workbench_relative_path(case.id));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        fs::write(&path, rendered)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    }

    let manifest_path = workbench_regression_root().join("manifest.json");
    let manifest = bundle_regression_cases()
        .into_iter()
        .map(|case| {
            serde_json::json!({
                "id": case.id,
                "workbench_path": expected_workbench_relative_path(case.id),
            })
        })
        .collect::<Vec<_>>();
    let rendered_manifest = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("failed to render workbench regression manifest: {err}"))?;
    fs::create_dir_all(workbench_regression_root())
        .map_err(|err| format!("failed to create workbench regression dir: {err}"))?;
    fs::write(&manifest_path, rendered_manifest)
        .map_err(|err| format!("failed to write {}: {err}", manifest_path.display()))?;

    Ok(())
}

fn verify_workbench_regression_case(case: &BundleRegressionCase) -> Result<Option<String>, String> {
    let actual = render_workbench_regression_case(case)?;
    let expected_path = repo_root().join(expected_workbench_relative_path(case.id));
    let expected = fs::read_to_string(&expected_path)
        .map_err(|err| format!("failed to read {}: {err}", expected_path.display()))?;

    if expected == actual {
        Ok(None)
    } else {
        Ok(Some(render_text_mismatch(&expected, &actual)))
    }
}

fn render_workbench_regression_case(case: &BundleRegressionCase) -> Result<String, String> {
    let projection = run_bundle_regression_case(case)?;
    let packet = build_review_packet(
        &projection.bundle,
        &projection.draft,
        &projection.validation,
    )
    .map_err(|err| format!("failed to build review packet: {err}"))?;
    Ok(render_review_workbench_html(&packet))
}

fn render_text_mismatch(expected: &str, actual: &str) -> String {
    let expected_lines = expected.lines().collect::<Vec<_>>();
    let actual_lines = actual.lines().collect::<Vec<_>>();
    let max_lines = expected_lines.len().max(actual_lines.len());

    for line_index in 0..max_lines {
        let expected_line = expected_lines
            .get(line_index)
            .copied()
            .unwrap_or("[end of file]");
        let actual_line = actual_lines
            .get(line_index)
            .copied()
            .unwrap_or("[end of file]");
        if expected_line != actual_line {
            return format!(
                "workbench html mismatch at line {}\nEXPECTED: {}\nACTUAL: {}",
                line_index + 1,
                expected_line,
                actual_line
            );
        }
    }

    "workbench html mismatch with no line-level diff found".to_owned()
}

fn expected_workbench_relative_path(case_id: &str) -> String {
    format!("old/workbench_regressions/{case_id}.workbench.html")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::verify_workbench_regressions;

    #[test]
    fn workbench_regression_suite_is_clean() {
        let report = verify_workbench_regressions();
        assert!(
            report.is_clean(),
            "workbench regression failures: {:#?}",
            report.failures
        );
    }
}
