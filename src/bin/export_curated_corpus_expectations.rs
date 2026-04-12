use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use expense_report_schema::{curated_corpus_cases, curated_corpus_root, render_document_facts_json_pretty};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct CuratedCorpusManifestEntry {
    id: String,
    markdown_path: String,
    expected_path: String,
}

fn main() -> ExitCode {
    if let Err(err) = export_curated_corpus_expectations() {
        eprintln!("failed to export curated corpus expectations: {err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

fn export_curated_corpus_expectations() -> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = Vec::new();

    for case in curated_corpus_cases() {
        let expected_json = render_document_facts_json_pretty(&case.expected_facts)?;
        let relative_markdown_path = PathBuf::from(case.relative_path);
        let relative_expected_path = PathBuf::from(format!("{}.expected.json", case.relative_path));
        let expected_path = repo_root().join(&relative_expected_path);

        if let Some(parent) = expected_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&expected_path, expected_json)?;

        manifest.push(CuratedCorpusManifestEntry {
            id: case.id.to_owned(),
            markdown_path: relative_markdown_path.to_string_lossy().into_owned(),
            expected_path: relative_expected_path.to_string_lossy().into_owned(),
        });
    }

    let manifest_path = curated_corpus_root().join("manifest.json");
    let rendered_manifest = serde_json::to_string_pretty(&manifest)?;
    fs::write(manifest_path, rendered_manifest)?;

    Ok(())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
