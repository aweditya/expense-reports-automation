use std::process::ExitCode;

use expense_report_schema::verify_bundle_regressions;

fn main() -> ExitCode {
    let report = verify_bundle_regressions();
    if report.is_clean() {
        println!("bundle regressions verified: 0 failure(s)");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "bundle regressions verified: {} failure(s)",
            report.failures.len()
        );
        for failure in report.failures {
            eprintln!("\n[{}]", failure.case_id);
            eprintln!("{}", failure.message);
        }
        ExitCode::from(1)
    }
}
