//! IATA code → Stanford-portal full airport display string.
//!
//! Stanford's foreign-page Expense Lines CSV expects airport cells
//! in the format `SFO - San Francisco International (San Francisco, CA)`,
//! not the raw IATA code. The lookup table (9208 entries) is
//! extracted at codegen time by `scripts/generate_airport_codes.py`
//! from `reference/ers-template-foreign-example-filled.xlsx` and
//! embedded here via `include_str!`.
//!
//! For unknown IATA codes the lookup returns `None`; the CSV emitter
//! falls back to the raw code so the row still uploads and the FA
//! sees + fixes in Excel.

use std::collections::HashMap;
use std::sync::OnceLock;

const AIRPORT_CODES_JSON: &str = include_str!("../generated/airport_codes.json");

fn map() -> &'static HashMap<String, String> {
    static CELL: OnceLock<HashMap<String, String>> = OnceLock::new();
    CELL.get_or_init(|| {
        serde_json::from_str(AIRPORT_CODES_JSON)
            .expect("generated/airport_codes.json must be valid JSON (regenerate via scripts/generate_airport_codes.py)")
    })
}

/// Look up the Stanford-portal display string for an IATA code.
/// Case-insensitive on input (IATA codes are canonical uppercase but
/// receipts sometimes lower-case them). Returns `None` for unknown
/// codes — caller is expected to fall back to the raw input.
pub fn airport_code_to_full(iata: &str) -> Option<String> {
    let upper = iata.trim().to_uppercase();
    map().get(&upper).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_loaded_with_expected_size() {
        // 9208 entries from the xlsx; one safety check that the
        // include_str + parse round-trip didn't lose anything.
        assert!(map().len() > 9000, "got {} airport codes", map().len());
    }

    #[test]
    fn lookup_returns_full_display_string_for_known_codes() {
        // Spot-check codes that show up on real Stanford trips.
        assert!(airport_code_to_full("SFO")
            .expect("SFO should be in table")
            .starts_with("SFO - San Francisco International"));
        assert!(airport_code_to_full("ZRH")
            .expect("ZRH should be in table")
            .contains("Zurich"));
        assert!(airport_code_to_full("BOM")
            .expect("BOM should be in table")
            .contains("Mumbai") || airport_code_to_full("BOM")
            .expect("BOM should be in table").contains("Bombay"));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let upper = airport_code_to_full("SFO");
        let lower = airport_code_to_full("sfo");
        let mixed = airport_code_to_full("  SfO  ");
        assert_eq!(upper, lower);
        assert_eq!(upper, mixed);
    }

    #[test]
    fn lookup_returns_none_for_garbage() {
        assert_eq!(airport_code_to_full("ZZZ123"), None);
        assert_eq!(airport_code_to_full(""), None);
    }
}
