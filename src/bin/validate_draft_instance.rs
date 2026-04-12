use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use expense_report_schema::parse_draft_report_path;
use expense_report_schema::validate_draft_report;
use expense_report_schema::ValidationSeverity;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: cargo run --bin validate_draft_instance -- <draft-instance.json|draft-instance.yaml>");
        return ExitCode::from(2);
    };

    if args.next().is_some() {
        eprintln!("expected exactly one draft instance path");
        return ExitCode::from(2);
    }

    let path = PathBuf::from(path);
    let draft = match parse_draft_report_path(&path) {
        Ok(draft) => draft,
        Err(err) => {
            eprintln!("parse error: {err}");
            return ExitCode::from(2);
        }
    };

    let validation = validate_draft_report(&draft);
    let error_count = validation
        .issues
        .iter()
        .filter(|issue| issue.severity == ValidationSeverity::Error)
        .count();
    let warning_count = validation
        .issues
        .iter()
        .filter(|issue| issue.severity == ValidationSeverity::Warning)
        .count();

    println!(
        "validated draft {}: {} error(s), {} warning(s)",
        path.display(),
        error_count,
        warning_count
    );

    for issue in &validation.issues {
        let severity = match issue.severity {
            ValidationSeverity::Error => "ERROR",
            ValidationSeverity::Warning => "WARN",
        };
        println!(
            "[{severity}] {} ({}) {}",
            issue.path, issue.schema_path, issue.message
        );
    }

    if error_count > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
