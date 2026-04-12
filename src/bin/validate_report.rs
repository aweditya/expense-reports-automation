use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use expense_report_schema::parse_report_path;
use expense_report_schema::validate_expense_report;
use expense_report_schema::ValidationSeverity;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: cargo run --bin validate_report -- <report.json|report.yaml>");
        return ExitCode::from(2);
    };

    if args.next().is_some() {
        eprintln!("expected exactly one report path");
        return ExitCode::from(2);
    }

    let path = PathBuf::from(path);
    let report = match parse_report_path(&path) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("parse error: {err}");
            return ExitCode::from(2);
        }
    };

    let validation = validate_expense_report(&report);
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
        "validated {}: {} error(s), {} warning(s)",
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
