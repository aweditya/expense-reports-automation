use std::process::ExitCode;

use expense_report_schema::verify_feedback_regressions;

fn main() -> ExitCode {
    let report = verify_feedback_regressions();
    if report.is_clean() {
        println!("feedback regressions verified: 0 failure(s)");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "feedback regressions verified: {} failure(s)",
            report.failures.len()
        );
        for failure in report.failures {
            eprintln!("\n[{}]", failure.case_id);
            eprintln!("{}", failure.message);
        }
        ExitCode::from(1)
    }
}
