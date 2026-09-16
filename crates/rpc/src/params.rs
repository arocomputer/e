//! Decode method parameters before applying defaults or changing session state.
//! Unknown fields remain accepted for forward-compatible clients.

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

pub(super) fn decode<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| format!("invalid params: {error}"))
}

#[derive(Deserialize)]
pub(super) struct Hello {
    pub ask: Option<bool>,
}
#[derive(Deserialize)]
pub(super) struct Create {
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub tools: Option<Vec<String>>,
    pub tool_mode: Option<String>,
    pub save: Option<bool>,
    pub resume: Option<String>,
    pub name: Option<String>,
}
#[derive(Deserialize)]
pub(super) struct Prompt {
    pub prompt: String,
    pub images: Option<Vec<String>>,
}
#[derive(Deserialize)]
pub(super) struct Steer {
    pub text: String,
}
#[derive(Deserialize)]
pub(super) struct Compact {
    pub focus: Option<String>,
}
#[derive(Deserialize)]
pub(super) struct Set {
    pub model: Option<String>,
    pub effort: Option<String>,
}
#[derive(Deserialize)]
pub(super) struct Export {
    pub path: Option<String>,
}
#[derive(Deserialize)]
pub(super) struct List {
    pub all: Option<bool>,
    pub cwd: Option<String>,
}
