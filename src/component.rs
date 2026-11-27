//! Rust descriptions used by the Inspector and by project validation.
//! A field with an empty key edits the component's entire JSON value.
use serde_json::{Map, Value};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Number {
        default: f64,
        min: f64,
        max: f64,
    },
    Count(u64),
    Toggle(bool),
    Choice {
        default: String,
        options: Vec<String>,
    },
    Scene,
    Prefab,
}

impl Kind {
    pub fn default_value(&self) -> Value {
        match self {
            Self::Number { default, .. } => serde_json::json!(default),
            Self::Count(n) => (*n).into(),
            Self::Toggle(b) => (*b).into(),
            Self::Choice { default, .. } => serde_json::json!(default),
            Self::Scene | Self::Prefab => "".into(),
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub kind: Kind,
    pub when: Option<(String, String)>,
}

impl Field {
    pub fn new(key: &str, label: &str, kind: Kind) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            kind,
            when: None,
        }
    }

    pub fn visible(&self, data: &Value) -> bool {
        self.when.as_ref().is_none_or(|(key, value)| {
            data.get(key).and_then(Value::as_str) == Some(value.as_str())
        })
    }

    pub fn value<'a>(&self, data: &'a Value) -> Option<&'a Value> {
        if self.key.is_empty() {
            Some(data)
        } else {
            data.get(&self.key)
        }
    }

    pub fn set(&self, data: &mut Value, value: Value) {
        if self.key.is_empty() {
            *data = value;
        } else {
            data[&self.key] = value;
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Schema {
    pub name: String,
    pub fields: Vec<Field>,
    pub trigger: bool,
}

impl Schema {
    pub fn validate_definition(&self) -> Result<(), String> {
        let mut keys = std::collections::HashSet::new();
        if self.name.trim().is_empty()
            || self.fields.iter().any(|field| {
                !keys.insert(&field.key)
                    || (field.key.is_empty() && self.fields.len() != 1)
                    || match &field.kind {
                        Kind::Number { default, min, max } => {
                            !min.is_finite() || !max.is_finite() || !(*min..=*max).contains(default)
                        }
                        Kind::Choice { default, options } => !options.contains(default),
                        _ => false,
                    }
            })
        {
            return Err(format!("Invalid field definition for '{}'.", self.name));
        }
        Ok(())
    }

    pub fn default_value(&self) -> Value {
        let mut value = Value::Object(Map::new());
        for f in &self.fields {
            f.set(&mut value, f.kind.default_value());
        }
        value
    }

    pub fn validate(
        &self,
        data: &Value,
        scene: impl Fn(&str) -> bool,
        prefab: impl Fn(&str) -> bool,
    ) -> Result<(), String> {
        if self.fields.iter().any(|f| !f.key.is_empty()) {
            let map = data
                .as_object()
                .ok_or_else(|| format!("{} expects an object", self.name))?;
            if let Some(key) = map
                .keys()
                .find(|k| !self.fields.iter().any(|f| f.key == k.as_str()))
            {
                return Err(format!("{}: unknown field {key}", self.name));
            }
        }
        for f in &self.fields {
            let active = f.visible(data);
            if !active && f.value(data).is_none() {
                continue;
            }
            let v = f.value(data).unwrap_or(&Value::Null);
            let valid = match &f.kind {
                Kind::Number { min, max, .. } => {
                    v.as_f64().is_some_and(|n| (*min..=*max).contains(&n))
                }
                Kind::Count(_) => v.as_u64().is_some(),
                Kind::Toggle(_) => v.is_boolean(),
                Kind::Choice { options, .. } => v
                    .as_str()
                    .is_some_and(|s| options.iter().any(|choice| choice == s)),
                Kind::Scene => v.as_str().is_some_and(|id| !active || scene(id)),
                Kind::Prefab => v.as_str().is_some_and(|id| !active || prefab(id)),
            };
            if !valid {
                return Err(format!("{} / {}: invalid value {v}", self.name, f.label));
            }
        }
        Ok(())
    }
}
