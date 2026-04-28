use expense_report_schema::{
    build_review_packet_with_ocr_artifacts, load_review_submission_ledger_path,
    render_fa_workbench_html, render_review_preview_html, render_review_workbench_html,
    DraftVersionRecord, ReviewPacket, ReviewSubmissionLedger,
};
use expense_report_schema::review_packet::apply_confirmed_review_paths;
use std::collections::BTreeSet;

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
    let packet = build_current_review_packet(&ledger, current_version)
        .map_err(|err| format!("failed to rebuild review packet: {err}"))?;

    match surface.as_str() {
        "fa" | "workbench" => Ok(render_fa_workbench_html(&packet)),
        "developer" => Ok(render_review_workbench_html(&packet)),
        "preview" => Ok(render_review_preview_html(&packet)),
        other => Err(format!("unknown surface: {other}\n{}", usage())),
    }
}

fn build_current_review_packet(
    ledger: &ReviewSubmissionLedger,
    current_version: &DraftVersionRecord,
) -> Result<ReviewPacket, String> {
    let mut packet = build_review_packet_with_ocr_artifacts(
        &ledger.bundle,
        &current_version.draft,
        &current_version.readiness,
        &ledger.ocr_pass_comparisons,
        &ledger.ocr_groundings,
    )
    .map_err(|err| err.to_string())?;
    let confirmed_review_paths = current_version
        .confirmed_review_paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    apply_confirmed_review_paths(&mut packet, &confirmed_review_paths);
    Ok(packet)
}

fn usage() -> String {
    "usage: render_current_review_surface --artifacts-dir <dir> [--surface fa|developer|preview]"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::build_current_review_packet;
    use expense_report_schema::{
        apply_review_revision, generate_synthetic_packet, initialize_review_submission_ledger,
        synthesize_bundle_projection_with_fx, ActorRole, DraftRevisionInput,
        StaticFxRateProvider, SyntheticVariant,
    };

    #[test]
    fn build_current_review_packet_respects_confirmed_review_paths() {
        let documents = generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect::<Vec<_>>();
        let projection = synthesize_bundle_projection_with_fx(&documents, &StaticFxRateProvider::demo());
        let mut ledger = initialize_review_submission_ledger(
            "render-current-review-surface-test",
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("ledger should build");

        let revision = DraftRevisionInput {
            actor_role: ActorRole::FinancialAdministrator,
            label: "confirmed manual review".to_owned(),
            field_edits: Vec::new(),
            confirmed_review_paths: vec![
                "expense_report.transaction_lines[0].common.date".to_owned(),
                "expense_report.transaction_lines[1].common.date".to_owned(),
                "expense_report.transaction_lines[2].common.date".to_owned(),
            ],
            annotations: Vec::new(),
        };
        let version_id = apply_review_revision(&mut ledger, 1, revision)
            .expect("revision should apply");
        let current_version = ledger
            .draft_versions
            .iter()
            .find(|version| version.version_id == version_id)
            .expect("current version should exist");

        let packet = build_current_review_packet(&ledger, current_version)
            .expect("packet should rebuild");

        assert!(!packet
            .copy_sections
            .iter()
            .flat_map(|section| section.instances.iter())
            .flat_map(|instance| instance.fields.iter())
            .any(|field| {
                matches!(
                    field.path.as_str(),
                    "expense_report.transaction_lines[0].common.date"
                        | "expense_report.transaction_lines[1].common.date"
                        | "expense_report.transaction_lines[2].common.date"
                ) && field.needs_review
            }));
    }
}
