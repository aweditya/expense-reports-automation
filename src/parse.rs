use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Json,
    Yaml,
}

#[derive(Debug)]
pub enum ParseReportError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
    UnsupportedFormat(String),
    InvalidRoot(String),
    NonStringObjectKey(String),
}

impl fmt::Display for ParseReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON parse error: {err}"),
            Self::Yaml(err) => write!(f, "YAML parse error: {err}"),
            Self::UnsupportedFormat(ext) => {
                write!(f, "Unsupported report format {ext:?}; expected .json, .yaml, or .yml")
            }
            Self::InvalidRoot(message) => write!(f, "Invalid report root: {message}"),
            Self::NonStringObjectKey(path) => {
                write!(f, "Object at {path} contains a non-string key")
            }
        }
    }
}

impl std::error::Error for ParseReportError {}

impl From<std::io::Error> for ParseReportError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ParseReportError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<serde_yaml::Error> for ParseReportError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Yaml(value)
    }
}

pub fn parse_report_path(path: impl AsRef<Path>) -> Result<ReportValue, ParseReportError> {
    let root = parse_document_path(path)?;
    normalize_report_root(root)
}

pub fn parse_document_path(path: impl AsRef<Path>) -> Result<ReportValue, ParseReportError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;

    match detect_format(path) {
        Some(format) => parse_document_str(&contents, format),
        None => {
            parse_document_str(&contents, ReportFormat::Json)
                .or_else(|_| parse_document_str(&contents, ReportFormat::Yaml))
        }
    }
}

pub fn parse_report_str(input: &str, format: ReportFormat) -> Result<ReportValue, ParseReportError> {
    let root = parse_document_str(input, format)?;
    normalize_report_root(root)
}

pub fn parse_document_str(input: &str, format: ReportFormat) -> Result<ReportValue, ParseReportError> {
    let root = match format {
        ReportFormat::Json => from_json_value(serde_json::from_str::<JsonValue>(input)?),
        ReportFormat::Yaml => from_yaml_value(serde_yaml::from_str::<YamlValue>(input)?, "$")?,
    };
    Ok(root)
}

fn detect_format(path: &Path) -> Option<ReportFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "json" => Some(ReportFormat::Json),
        "yaml" | "yml" => Some(ReportFormat::Yaml),
        _ => None,
    }
}

fn normalize_report_root(root: ReportValue) -> Result<ReportValue, ParseReportError> {
    match root {
        ReportValue::Object(mut map) => {
            if let Some(report_value) = map.remove("expense_report") {
                if matches!(report_value, ReportValue::Object(_)) {
                    return Ok(report_value);
                }
                return Err(ParseReportError::InvalidRoot(
                    "top-level 'expense_report' must contain an object".to_owned(),
                ));
            }
            Ok(ReportValue::Object(map))
        }
        other => Err(ParseReportError::InvalidRoot(format!(
            "report document must be an object, found {:?}",
            other.kind()
        ))),
    }
}

fn from_json_value(value: JsonValue) -> ReportValue {
    match value {
        JsonValue::Null => ReportValue::Null,
        JsonValue::Bool(value) => ReportValue::Bool(value),
        JsonValue::Number(value) => ReportValue::Number(value.to_string()),
        JsonValue::String(value) => ReportValue::String(value),
        JsonValue::Array(values) => ReportValue::Array(values.into_iter().map(from_json_value).collect()),
        JsonValue::Object(values) => ReportValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, from_json_value(value)))
                .collect::<BTreeMap<_, _>>(),
        ),
    }
}

fn from_yaml_value(value: YamlValue, path: &str) -> Result<ReportValue, ParseReportError> {
    match value {
        YamlValue::Null => Ok(ReportValue::Null),
        YamlValue::Bool(value) => Ok(ReportValue::Bool(value)),
        YamlValue::Number(value) => Ok(ReportValue::Number(value.to_string())),
        YamlValue::String(value) => Ok(ReportValue::String(value)),
        YamlValue::Sequence(values) => Ok(ReportValue::Array(
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| from_yaml_value(value, &format!("{path}[{index}]")))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        YamlValue::Mapping(values) => {
            let mut object = BTreeMap::new();
            for (key, value) in values {
                let YamlValue::String(key) = key else {
                    return Err(ParseReportError::NonStringObjectKey(path.to_owned()));
                };
                let child_path = if path == "$" {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                object.insert(key, from_yaml_value(value, &child_path)?);
            }
            Ok(ReportValue::Object(object))
        }
        YamlValue::Tagged(tagged) => from_yaml_value(tagged.value, path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wrapped_json_report() {
        let parsed = parse_report_str(
            r#"{
              "expense_report": {
                "general_information": {
                  "category": "expenses_domestic"
                }
              }
            }"#,
            ReportFormat::Json,
        )
        .expect("json should parse");

        let ReportValue::Object(object) = parsed else {
            panic!("expected object root");
        };
        assert!(object.contains_key("general_information"));
        assert!(!object.contains_key("expense_report"));
    }

    #[test]
    fn parses_wrapped_yaml_report() {
        let parsed = parse_report_str(
            r#"
schema_version: "0.1.0"
expense_report:
  general_information:
    category: expenses_domestic
"#,
            ReportFormat::Yaml,
        )
        .expect("yaml should parse");

        let ReportValue::Object(object) = parsed else {
            panic!("expected object root");
        };
        assert!(object.contains_key("general_information"));
        assert!(!object.contains_key("expense_report"));
    }
}
