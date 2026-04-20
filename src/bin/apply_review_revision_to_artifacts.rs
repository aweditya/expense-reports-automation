use std::process::ExitCode;

use expense_report_schema::{apply_review_revision_at_artifacts_dir, DraftRevisionInput};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let (artifacts_dir, revision_path, base_version_id) = parse_args(std::env::args().skip(1))?;

    let revision: DraftRevisionInput = serde_json::from_str(
        &std::fs::read_to_string(&revision_path)
            .map_err(|err| format!("failed to read {revision_path}: {err}"))?,
    )
    .map_err(|err| format!("failed to parse {revision_path}: {err}"))?;

    let result = apply_review_revision_at_artifacts_dir(&artifacts_dir, revision, base_version_id)
        .map_err(|err| err.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result)
            .map_err(|err| format!("failed to render save result: {err}"))?
    );
    Ok(())
}

fn parse_args(
    mut args: impl Iterator<Item = String>,
) -> Result<(String, String, Option<u32>), String> {
    let mut artifacts_dir = None;
    let mut revision_path = None;
    let mut base_version_id = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--artifacts-dir" => {
                artifacts_dir = args.next();
            }
            "--revision-json" => {
                revision_path = args.next();
            }
            "--base-version" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after --base-version".to_owned())?;
                base_version_id = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| "base version must be a positive integer".to_owned())?,
                );
            }
            "--help" | "-h" => {
                return Err(
                    "usage: apply_review_revision_to_artifacts --artifacts-dir <dir> --revision-json <path> [--base-version <id>]".to_owned(),
                )
            }
            other => return Err(format!("unexpected argument: {other}")),
        }
    }

    Ok((
        artifacts_dir.ok_or_else(|| "missing required --artifacts-dir".to_owned())?,
        revision_path.ok_or_else(|| "missing required --revision-json".to_owned())?,
        base_version_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    #[test]
    fn rejects_missing_required_arguments() {
        let result = parse_args(Vec::<String>::new().into_iter());
        assert!(result.is_err());
    }

    #[test]
    fn parses_base_version_when_present() {
        let result = parse_args(
            vec![
                "--artifacts-dir".to_owned(),
                "out".to_owned(),
                "--revision-json".to_owned(),
                "revision.json".to_owned(),
                "--base-version".to_owned(),
                "7".to_owned(),
            ]
            .into_iter(),
        )
        .expect("args should parse");
        assert_eq!(result.0, "out");
        assert_eq!(result.1, "revision.json");
        assert_eq!(result.2, Some(7));
    }
}
