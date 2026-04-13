use std::process::ExitCode;

use expense_report_schema::verify_workbench_regressions;

fn main() -> ExitCode {
    let report = verify_workbench_regressions();
    if report.is_clean() {
        println!("workbench regressions verified: 0 failure(s)");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "workbench regressions verified: {} failure(s)",
            report.failures.len()
        );
        for failure in report.failures {
            eprintln!("\n[{}]", failure.case_id);
            eprintln!("{}", failure.message);
        }
        ExitCode::from(1)
    }
}
