use std::collections::BTreeMap;

use crate::expense_report_model::{DecimalAmount, IsoDate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Null,
    String,
    Bool,
    Array,
    Object,
    Number,
    Date,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportValue {
    Null,
    String(String),
    Bool(bool),
    Array(Vec<ReportValue>),
    Object(BTreeMap<String, ReportValue>),
    Number(String),
    Date(String),
}

impl ReportValue {
    pub fn empty_object() -> Self {
        Self::Object(BTreeMap::new())
    }

    pub fn object(fields: impl IntoIterator<Item = (impl Into<String>, ReportValue)>) -> Self {
        let mut map = BTreeMap::new();
        for (key, value) in fields {
            map.insert(key.into(), value);
        }
        Self::Object(map)
    }

    pub fn array(items: impl IntoIterator<Item = ReportValue>) -> Self {
        Self::Array(items.into_iter().collect())
    }

    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Null => ValueKind::Null,
            Self::String(_) => ValueKind::String,
            Self::Bool(_) => ValueKind::Bool,
            Self::Array(_) => ValueKind::Array,
            Self::Object(_) => ValueKind::Object,
            Self::Number(_) => ValueKind::Number,
            Self::Date(_) => ValueKind::Date,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, ReportValue>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[ReportValue]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::String(value) | Self::Number(value) | Self::Date(value) => Some(value),
            _ => None,
        }
    }
}

impl From<String> for ReportValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for ReportValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<bool> for ReportValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<Vec<ReportValue>> for ReportValue {
    fn from(value: Vec<ReportValue>) -> Self {
        Self::Array(value)
    }
}

impl From<BTreeMap<String, ReportValue>> for ReportValue {
    fn from(value: BTreeMap<String, ReportValue>) -> Self {
        Self::Object(value)
    }
}

impl From<IsoDate> for ReportValue {
    fn from(value: IsoDate) -> Self {
        Self::Date(value.0)
    }
}

impl From<DecimalAmount> for ReportValue {
    fn from(value: DecimalAmount) -> Self {
        Self::Number(value.0)
    }
}
