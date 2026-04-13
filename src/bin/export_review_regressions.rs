use std::process::ExitCode;

use expense_report_schema::export_review_regressions;

fn main() -> ExitCode {
    match export_review_regressions() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("failed to export review regressions: {err}");
            ExitCode::from(1)
        }
    }
}
