use std::process::ExitCode;

use expense_report_schema::export_workbench_regressions;

fn main() -> ExitCode {
    match export_workbench_regressions() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("failed to export workbench regressions: {err}");
            ExitCode::from(1)
        }
    }
}
