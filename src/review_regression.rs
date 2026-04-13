use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;

use crate::bundle_regression::{
    bundle_regression_cases, run_bundle_regression_case, BundleRegressionCase,
};
use crate::review_packet::{build_review_packet, render_review_packet_json_pretty};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRegressionFailure {
    pub case_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewRegressionVerificationReport {
    pub failures: Vec<ReviewRegressionFailure>,
}

impl ReviewRegressionVerificationReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn review_regression_root() -> PathBuf {
    repo_root().join("fixtures/review_regressions")
}

pub fn verify_review_regressions() -> ReviewRegressionVerificationReport {
    let mut failures = Vec::new();

    for case in bundle_regression_cases() {
        match verify_review_regression_case(&case) {
            Ok(None) => {}
            Ok(Some(message)) => failures.push(ReviewRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
            Err(message) => failures.push(ReviewRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
        }
    }

    ReviewRegressionVerificationReport { failures }
}

pub fn export_review_regressions() -> Result<(), String> {
    for case in bundle_regression_cases() {
        let rendered = render_review_regression_case(&case)?;
        let path = repo_root().join(expected_review_relative_path(case.id));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        fs::write(&path, rendered)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    }

    let manifest_path = review_regression_root().join("manifest.json");
    let manifest = bundle_regression_cases()
        .into_iter()
        .map(|case| {
            serde_json::json!({
                "id": case.id,
                "review_packet_path": expected_review_relative_path(case.id),
            })
        })
        .collect::<Vec<_>>();
    let rendered_manifest = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("failed to render review regression manifest: {err}"))?;
    fs::create_dir_all(review_regression_root())
        .map_err(|err| format!("failed to create review regression dir: {err}"))?;
    fs::write(&manifest_path, rendered_manifest)
        .map_err(|err| format!("failed to write {}: {err}", manifest_path.display()))?;

    Ok(())
}

fn verify_review_regression_case(case: &BundleRegressionCase) -> Result<Option<String>, String> {
    let actual = parse_rendered_json(&render_review_regression_case(case)?)
        .map_err(|err| format!("failed to parse rendered review packet json: {err}"))?;
    let expected = load_expected_json(expected_review_relative_path(case.id))?;

    if expected == actual {
        Ok(None)
    } else {
        Ok(Some(format!(
            "review packet mismatch\nEXPECTED:\n{}\nACTUAL:\n{}",
            render_json_value(&expected),
            render_json_value(&actual)
        )))
    }
}

fn render_review_regression_case(case: &BundleRegressionCase) -> Result<String, String> {
    let projection = run_bundle_regression_case(case)?;
    let packet = build_review_packet(
        &projection.bundle,
        &projection.draft,
        &projection.validation,
    )
    .map_err(|err| format!("failed to build review packet: {err}"))?;
    render_review_packet_json_pretty(&packet)
        .map_err(|err| format!("failed to render review packet json: {err}"))
}

fn load_expected_json(relative_path: String) -> Result<JsonValue, String> {
    let path = repo_root().join(relative_path);
    let value = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&value).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn parse_rendered_json(value: &str) -> Result<JsonValue, serde_json::Error> {
    serde_json::from_str(value)
}

fn render_json_value(value: &JsonValue) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|err| format!("unable to render json value: {err}"))
}

fn expected_review_relative_path(case_id: &str) -> String {
    format!("fixtures/review_regressions/{case_id}.review_packet.json")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::verify_review_regressions;

    #[test]
    fn review_regression_suite_is_clean() {
        let report = verify_review_regressions();
        assert!(
            report.is_clean(),
            "review regression failures: {:#?}",
            report.failures
        );
    }
}
