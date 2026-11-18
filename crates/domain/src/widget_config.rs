use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type WidgetConfigSchema = BTreeMap<String, WidgetConfigField>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetConfigField {
    #[serde(rename = "type")]
    pub kind: FieldType,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub choices: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Number,
    Integer,
    Boolean,
    Select,
}

impl WidgetConfigField {
    pub fn accepts(&self, value: &Value) -> bool {
        match self.kind {
            FieldType::String => value.is_string(),
            FieldType::Boolean => value.is_boolean(),
            FieldType::Select => value
                .as_str()
                .is_some_and(|value| self.choices.iter().any(|choice| choice == value)),
            FieldType::Number | FieldType::Integer => value.as_f64().is_some_and(|number| {
                (self.kind == FieldType::Number || number.fract() == 0.0)
                    && self.min.is_none_or(|min| number >= min)
                    && self.max.is_none_or(|max| number <= max)
            }),
        }
    }
}
