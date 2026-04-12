use std::process::ExitCode;

use expense_report_schema::verify_curated_corpus;

fn main() -> ExitCode {
    let report = verify_curated_corpus();
    if report.is_clean() {
        println!("curated corpus verified: 0 failure(s)");
        ExitCode::SUCCESS
    } else {
        eprintln!("curated corpus verified: {} failure(s)", report.failures.len());
        for failure in report.failures {
            eprintln!("\n[{}] {}", failure.case_id, failure.relative_path);
            eprintln!("{}", failure.message);
        }
        ExitCode::from(1)
    }
}
