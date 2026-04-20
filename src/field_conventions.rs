use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldControl {
    Text,
    Textarea,
    Select,
    Checkbox,
    Date,
    Currency,
    Number,
    StructuredList,
}

impl FieldControl {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Textarea => "textarea",
            Self::Select => "select",
            Self::Checkbox => "checkbox",
            Self::Date => "date",
            Self::Currency => "currency",
            Self::Number => "number",
            Self::StructuredList => "structured_list",
        }
    }

    pub const fn is_structured(self) -> bool {
        matches!(self, Self::StructuredList)
    }

    pub const fn uses_decimal_input_mode(self) -> bool {
        matches!(self, Self::Currency | Self::Number)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEntryMode {
    ModelPrefillReview,
    ComputedReadonly,
    UserConfirmedInput,
}

impl FieldEntryMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelPrefillReview => "model_prefill_review",
            Self::ComputedReadonly => "computed_readonly",
            Self::UserConfirmedInput => "user_confirmed_input",
        }
    }

    pub const fn is_readonly(self) -> bool {
        matches!(self, Self::ComputedReadonly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_controls_round_trip_as_expected_strings() {
        let cases = [
            (FieldControl::Text, "text"),
            (FieldControl::Textarea, "textarea"),
            (FieldControl::Select, "select"),
            (FieldControl::Checkbox, "checkbox"),
            (FieldControl::Date, "date"),
            (FieldControl::Currency, "currency"),
            (FieldControl::Number, "number"),
            (FieldControl::StructuredList, "structured_list"),
        ];
        for (control, expected) in cases {
            assert_eq!(control.as_str(), expected);
            let parsed: FieldControl =
                serde_json::from_str(&format!("\"{expected}\"")).expect("control should parse");
            assert_eq!(parsed, control);
        }
    }

    #[test]
    fn field_entry_modes_round_trip_as_expected_strings() {
        let cases = [
            (FieldEntryMode::ModelPrefillReview, "model_prefill_review"),
            (FieldEntryMode::ComputedReadonly, "computed_readonly"),
            (FieldEntryMode::UserConfirmedInput, "user_confirmed_input"),
        ];
        for (mode, expected) in cases {
            assert_eq!(mode.as_str(), expected);
            let parsed: FieldEntryMode =
                serde_json::from_str(&format!("\"{expected}\"")).expect("entry mode should parse");
            assert_eq!(parsed, mode);
        }
    }
}
