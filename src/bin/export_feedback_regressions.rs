use std::process::ExitCode;

use expense_report_schema::export_feedback_regressions;

fn main() -> ExitCode {
    match export_feedback_regressions() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("failed to export feedback regressions: {err}");
            ExitCode::from(1)
        }
    }
}
