use std::process::ExitCode;

use expense_report_schema::verify_ledger_regressions;

fn main() -> ExitCode {
    let report = verify_ledger_regressions();
    if report.is_clean() {
        println!("ledger regressions verified: 0 failure(s)");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "ledger regressions verified: {} failure(s)",
            report.failures.len()
        );
        for failure in report.failures {
            eprintln!("\n[{}]", failure.case_id);
            eprintln!("{}", failure.message);
        }
        ExitCode::from(1)
    }
}
