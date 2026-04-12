use std::env;
use std::process::ExitCode;

use expense_report_schema::{extract_document_facts_path, render_document_facts_json_pretty};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let path = args
        .next()
        .ok_or_else(|| "usage: extract_document_facts <document-path>".to_owned())?;

    if args.next().is_some() {
        return Err("usage: extract_document_facts <document-path>".to_owned());
    }

    let facts = extract_document_facts_path(&path).map_err(|err| err.to_string())?;
    facts.validate_contract().map_err(|err| err.to_string())?;
    let rendered = render_document_facts_json_pretty(&facts).map_err(|err| err.to_string())?;
    print!("{rendered}");
    Ok(())
}
