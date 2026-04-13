use std::process::ExitCode;

use expense_report_schema::verify_review_regressions;

fn main() -> ExitCode {
    let report = verify_review_regressions();
    if report.is_clean() {
        println!("review regressions verified: 0 failure(s)");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "review regressions verified: {} failure(s)",
            report.failures.len()
        );
        for failure in report.failures {
            eprintln!("\n[{}]", failure.case_id);
            eprintln!("{}", failure.message);
        }
        ExitCode::from(1)
    }
}
