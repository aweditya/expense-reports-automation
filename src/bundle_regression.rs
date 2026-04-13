use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;

use crate::bundle_synthesis::{
    render_canonical_bundle_json_pretty, synthesize_bundle_projection,
    synthesize_bundle_projection_with_fx, BundleProjectionResult, StaticFxRateProvider,
};
use crate::document_facts::parse_document_facts_json_path;
use crate::render::render_draft_report_json_pretty;
use crate::validator::render_validation_report_json_pretty;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleRegressionFxMode {
    None,
    Demo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRegressionCase {
    pub id: &'static str,
    pub fact_paths: &'static [&'static str],
    pub fx_mode: BundleRegressionFxMode,
}

impl BundleRegressionCase {
    pub fn expected_bundle_relative_path(&self) -> String {
        format!("fixtures/bundle_regressions/{}.bundle.json", self.id)
    }

    pub fn expected_draft_relative_path(&self) -> String {
        format!("fixtures/bundle_regressions/{}.draft.json", self.id)
    }

    pub fn expected_validation_relative_path(&self) -> String {
        format!("fixtures/bundle_regressions/{}.validation.json", self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRegressionFailure {
    pub case_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleRegressionVerificationReport {
    pub failures: Vec<BundleRegressionFailure>,
}

impl BundleRegressionVerificationReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn bundle_regression_root() -> PathBuf {
    repo_root().join("fixtures/bundle_regressions")
}

pub fn bundle_regression_cases() -> Vec<BundleRegressionCase> {
    vec![
        BundleRegressionCase {
            id: "curated_packet_no_fx",
            fact_paths: &[
                "fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json",
                "fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json",
                "fixtures/curated/receipt/receipt_card_dotted.md.expected.json",
            ],
            fx_mode: BundleRegressionFxMode::None,
        },
        BundleRegressionCase {
            id: "curated_packet_demo_fx",
            fact_paths: &[
                "fixtures/curated/flight_itinerary/airline_itinerary_classic.md.expected.json",
                "fixtures/curated/hotel_folio/hotel_folio_guest_bill.md.expected.json",
                "fixtures/curated/receipt/receipt_card_dotted.md.expected.json",
            ],
            fx_mode: BundleRegressionFxMode::Demo,
        },
        BundleRegressionCase {
            id: "curated_alt_packet_no_fx",
            fact_paths: &[
                "fixtures/curated/flight_itinerary/airline_itinerary_trip_window.md.expected.json",
                "fixtures/curated/hotel_folio/hotel_folio_property_labeled.md.expected.json",
                "fixtures/curated/receipt/receipt_merchant_labeled.md.expected.json",
            ],
            fx_mode: BundleRegressionFxMode::None,
        },
        BundleRegressionCase {
            id: "curated_alt_packet_demo_fx",
            fact_paths: &[
                "fixtures/curated/flight_itinerary/airline_itinerary_trip_window.md.expected.json",
                "fixtures/curated/hotel_folio/hotel_folio_property_labeled.md.expected.json",
                "fixtures/curated/receipt/receipt_merchant_labeled.md.expected.json",
            ],
            fx_mode: BundleRegressionFxMode::Demo,
        },
    ]
}

pub fn run_bundle_regression_case(
    case: &BundleRegressionCase,
) -> Result<BundleProjectionResult, String> {
    let mut documents = Vec::new();
    for path in case.fact_paths {
        let full_path = repo_root().join(path);
        documents.push(
            parse_document_facts_json_path(&full_path)
                .map_err(|err| format!("failed to parse {}: {err}", full_path.display()))?,
        );
    }

    let result = match case.fx_mode {
        BundleRegressionFxMode::None => synthesize_bundle_projection(&documents),
        BundleRegressionFxMode::Demo => {
            let fx_provider = StaticFxRateProvider::demo();
            synthesize_bundle_projection_with_fx(&documents, &fx_provider)
        }
    };

    Ok(result)
}

pub fn verify_bundle_regressions() -> BundleRegressionVerificationReport {
    let mut failures = Vec::new();

    for case in bundle_regression_cases() {
        match verify_bundle_regression_case(&case) {
            Ok(None) => {}
            Ok(Some(message)) => failures.push(BundleRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
            Err(message) => failures.push(BundleRegressionFailure {
                case_id: case.id.to_owned(),
                message,
            }),
        }
    }

    BundleRegressionVerificationReport { failures }
}

fn verify_bundle_regression_case(case: &BundleRegressionCase) -> Result<Option<String>, String> {
    let result = run_bundle_regression_case(case)?;
    let actual_bundle = parse_rendered_json(
        &render_canonical_bundle_json_pretty(&result.bundle)
            .map_err(|err| format!("failed to render bundle json: {err}"))?,
    )
    .map_err(|err| format!("failed to parse rendered bundle json: {err}"))?;
    let actual_draft = parse_rendered_json(
        &render_draft_report_json_pretty(&result.draft)
            .map_err(|err| format!("failed to render draft json: {err}"))?,
    )
    .map_err(|err| format!("failed to parse rendered draft json: {err}"))?;
    let actual_validation = parse_rendered_json(
        &render_validation_report_json_pretty(&result.validation)
            .map_err(|err| format!("failed to render validation json: {err}"))?,
    )
    .map_err(|err| format!("failed to parse rendered validation json: {err}"))?;

    let expected_bundle = load_expected_json(case.expected_bundle_relative_path())?;
    let expected_draft = load_expected_json(case.expected_draft_relative_path())?;
    let expected_validation = load_expected_json(case.expected_validation_relative_path())?;

    let mut mismatches = Vec::new();
    push_json_mismatch(&mut mismatches, "bundle", &expected_bundle, &actual_bundle);
    push_json_mismatch(&mut mismatches, "draft", &expected_draft, &actual_draft);
    push_json_mismatch(
        &mut mismatches,
        "validation",
        &expected_validation,
        &actual_validation,
    );

    if mismatches.is_empty() {
        Ok(None)
    } else {
        Ok(Some(mismatches.join("\n\n")))
    }
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

fn push_json_mismatch(
    mismatches: &mut Vec<String>,
    label: &str,
    expected: &JsonValue,
    actual: &JsonValue,
) {
    if expected == actual {
        return;
    }

    let expected_pretty = render_json_value(expected);
    let actual_pretty = render_json_value(actual);
    mismatches.push(format!(
        "{label} mismatch\nEXPECTED:\n{expected_pretty}\nACTUAL:\n{actual_pretty}"
    ));
}

fn render_json_value(value: &JsonValue) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|err| format!("unable to render json value: {err}"))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{bundle_regression_cases, verify_bundle_regressions};

    #[test]
    fn bundle_regression_case_ids_are_unique() {
        let mut ids = BTreeSet::new();
        for case in bundle_regression_cases() {
            assert!(
                ids.insert(case.id),
                "duplicate bundle regression case id {:?}",
                case.id
            );
        }
    }

    #[test]
    fn bundle_regression_suite_is_clean() {
        let report = verify_bundle_regressions();
        assert!(
            report.is_clean(),
            "bundle regression failures: {:#?}",
            report.failures
        );
    }
}
