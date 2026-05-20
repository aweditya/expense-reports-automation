//! M6.5: round-trip check between Python's per-receipt JSON output and
//! Rust's `ExtractedReceipt` struct.
//!
//! The dangerous failure mode is silent contract drift — Python writes
//! `"line_amount_usd"` but Rust expects `"line_amount_usd_v2"`, and
//! `#[serde(default)]` swallows it as `None`. This binary catches that by
//! deserializing each spike output, re-serializing, and diffing the two
//! JSON trees field-by-field.
//!
//! Run: `cargo run --bin roundtrip_check`
//! Reads from `.scratch/spike/*.json`. Exit 0 if every file round-trips
//! losslessly; non-zero with a list of dropped/added/mutated fields if not.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

use expense_report_schema::extracted_receipt::ExtractedReceipt;

fn main() -> ExitCode {
    let dir = Path::new(".scratch/spike");
    if !dir.exists() {
        eprintln!("error: {} not found — run scripts/extract_meal.py first", dir.display());
        return ExitCode::from(2);
    }

    let mut entries: Vec<_> = match fs::read_dir(dir) {
        Ok(it) => it.filter_map(Result::ok).collect(),
        Err(err) => {
            eprintln!("error: read_dir({}): {err}", dir.display());
            return ExitCode::from(2);
        }
    };
    entries.sort_by_key(|e| e.file_name());

    let mut had_failure = false;

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let json = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) => {
                println!("FAIL  {name}: read error: {err}");
                had_failure = true;
                continue;
            }
        };

        let original: Value = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(err) => {
                println!("FAIL  {name}: input is not valid JSON: {err}");
                had_failure = true;
                continue;
            }
        };

        let receipts: Vec<ExtractedReceipt> = match serde_json::from_value(original.clone()) {
            Ok(v) => v,
            Err(err) => {
                println!("FAIL  {name}: deserialize into Vec<ExtractedReceipt>: {err}");
                had_failure = true;
                continue;
            }
        };

        let reserialized: Value = match serde_json::to_value(&receipts) {
            Ok(v) => v,
            Err(err) => {
                println!("FAIL  {name}: re-serialize: {err}");
                had_failure = true;
                continue;
            }
        };

        let all_mismatches = diff_paths("", &original, &reserialized);
        let (benign, mismatches): (Vec<_>, Vec<_>) = all_mismatches
            .into_iter()
            .partition(|m| is_benign_mismatch(m));
        if mismatches.is_empty() {
            if benign.is_empty() {
                println!("PASS  {name}");
            } else {
                println!("PASS  {name} ({} benign filtered)", benign.len());
            }
        } else {
            println!("FAIL  {name} ({} mismatches):", mismatches.len());
            for m in mismatches.iter().take(20) {
                println!("  {m}");
            }
            if mismatches.len() > 20 {
                println!("  ... and {} more", mismatches.len() - 20);
            }
            had_failure = true;
        }
    }

    if had_failure {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Walks two JSON values in parallel and returns a list of human-readable
/// path:reason mismatches. Each mismatch is one of:
///   - DROPPED: key present in `lhs` but missing in `rhs` (Rust silently lost it)
///   - ADDED:   key present in `rhs` but missing in `lhs` (Rust default-filled it)
///   - TYPE:    same key, different JSON type (e.g. number vs string)
///   - VALUE:   same key, same type, but different scalar value
fn diff_paths(prefix: &str, lhs: &Value, rhs: &Value) -> Vec<String> {
    let mut out = Vec::new();
    diff_at(prefix, lhs, rhs, &mut out);
    out
}

/// Same-value Number comparison that doesn't trip on integer↔float
/// representation drift. Python emits `80896` (Number::Integer) when
/// the model returns a whole-dollar value; Rust types `ticket_amount`
/// as `f64` so re-serialize emits `80896.0` (Number::Float). Both
/// values mean the same thing.
///
/// Tiered: try integer-equality first (no float arithmetic, no
/// precision concern at any magnitude); fall back to f64 only when
/// at least one side is genuinely a float. For expense dollar amounts
/// (nowhere near f64's 2^53 exact-integer range), the f64 path is
/// also exact in practice — but the integer-first tier means we don't
/// rely on that.
fn numbers_equal(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    if let (Some(ai), Some(bi)) = (a.as_i64(), b.as_i64()) {
        return ai == bi;
    }
    if let (Some(au), Some(bu)) = (a.as_u64(), b.as_u64()) {
        return au == bu;
    }
    match (a.as_f64(), b.as_f64()) {
        (Some(af), Some(bf)) => af == bf,
        _ => false,
    }
}

/// Mismatches the codegen documents as expected. Per the
/// `scripts/generate_schema_artifacts.py` comment on bare-struct fields:
/// reduction populates `common.source_document` before serializing the
/// report, so the field is always present on the way out — except in
/// this round-trip check, which operates at the **per-receipt** layer
/// (before reduction runs). The empty-default emit at that layer is
/// correct, not contract drift. Filtering keeps real round-trip
/// failures (token_ids drop, value mismatches, type swaps) visible
/// without the source_document noise drowning them out.
fn is_benign_mismatch(line: &str) -> bool {
    line.contains(".common.source_document ADDED")
}

fn diff_at(path: &str, lhs: &Value, rhs: &Value, out: &mut Vec<String>) {
    match (lhs, rhs) {
        (Value::Object(l), Value::Object(r)) => {
            let lkeys: BTreeSet<&String> = l.keys().collect();
            let rkeys: BTreeSet<&String> = r.keys().collect();
            for k in lkeys.difference(&rkeys) {
                out.push(format!("{}.{} DROPPED (Rust lost a field Python wrote)", path, k));
            }
            for k in rkeys.difference(&lkeys) {
                out.push(format!("{}.{} ADDED (Rust default-filled a field Python omitted)", path, k));
            }
            for k in lkeys.intersection(&rkeys) {
                let child = format!("{path}.{k}");
                diff_at(&child, &l[k.as_str()], &r[k.as_str()], out);
            }
        }
        (Value::Array(l), Value::Array(r)) => {
            if l.len() != r.len() {
                out.push(format!(
                    "{} ARRAY LENGTH (Python {}, Rust {})",
                    path, l.len(), r.len()
                ));
                return;
            }
            for (i, (li, ri)) in l.iter().zip(r.iter()).enumerate() {
                diff_at(&format!("{path}[{i}]"), li, ri, out);
            }
        }
        (Value::Null, Value::Null) => {}
        (Value::Bool(a), Value::Bool(b)) if a == b => {}
        (Value::Number(a), Value::Number(b)) if numbers_equal(a, b) => {}
        (Value::String(a), Value::String(b)) if a == b => {}
        (a, b) if a.is_null() != b.is_null() => {
            out.push(format!("{} NULL MISMATCH (Python {:?}, Rust {:?})", path, a, b));
        }
        (a, b) => {
            // Type or value differs.
            if std::mem::discriminant(a) != std::mem::discriminant(b) {
                out.push(format!("{} TYPE (Python {}, Rust {})", path, kind_name(a), kind_name(b)));
            } else {
                out.push(format!("{} VALUE (Python {:?}, Rust {:?})", path, a, b));
            }
        }
    }
}

fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benign_matcher_catches_per_receipt_source_document_added() {
        assert!(is_benign_mismatch(
            "[0].common.source_document ADDED (Rust default-filled a field Python omitted)"
        ));
        assert!(is_benign_mismatch(
            "[2].common.source_document ADDED (Rust default-filled a field Python omitted)"
        ));
    }

    #[test]
    fn numbers_equal_handles_integer_float_drift() {
        use serde_json::Number;
        let int_80896 = Number::from(80896i64);
        let float_80896 = Number::from_f64(80896.0).unwrap();
        assert!(numbers_equal(&int_80896, &float_80896));
        assert!(numbers_equal(&float_80896, &int_80896));
    }

    #[test]
    fn numbers_equal_rejects_actually_different_values() {
        use serde_json::Number;
        assert!(!numbers_equal(&Number::from(80896i64), &Number::from(80897i64)));
        assert!(!numbers_equal(
            &Number::from_f64(80896.5).unwrap(),
            &Number::from(80896i64),
        ));
    }

    #[test]
    fn benign_matcher_does_not_swallow_real_mismatches() {
        assert!(!is_benign_mismatch(
            "[0].common.date._meta.evidence[0].token_ids DROPPED (Rust lost a field Python wrote)"
        ));
        assert!(!is_benign_mismatch(
            "[0].common.line_amount_usd VALUE (Python 79.59, Rust 80.0)"
        ));
        // ADDED on a different field path is NOT benign.
        assert!(!is_benign_mismatch(
            "[0].common.remarks ADDED (Rust default-filled a field Python omitted)"
        ));
        // DROPPED source_document is the dangerous direction — real drift.
        assert!(!is_benign_mismatch(
            "[0].common.source_document DROPPED (Rust lost a field Python wrote)"
        ));
    }
}
