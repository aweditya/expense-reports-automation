use std::process::ExitCode;

use expense_report_schema::export_ledger_regressions;

fn main() -> ExitCode {
    match export_ledger_regressions() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("failed to export ledger regressions: {err}");
            ExitCode::from(1)
        }
    }
}
