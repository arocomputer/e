//! Frontend-neutral execution preferences and validation for a model-backed run.

use crate::providers::catalog;

/// The process-wide tool ceiling, which a nested request may only restrict.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolMode {
    #[default]
    All,
    None,
}

impl ToolMode {
    /// Whether this run permits tool execution.
    pub fn allows(self) -> bool {
        matches!(self, Self::All)
    }

    /// Apply a request preference without restoring disabled capabilities.
    pub fn restrict(self, requested: Self) -> Self {
        match (self, requested) {
            (Self::None, _) | (_, Self::None) => Self::None,
            (Self::All, Self::All) => Self::All,
        }
    }
}

/// Execution preferences shared by terminal, print, and RPC frontends, without argv state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub images: Vec<String>,
    pub no_save: bool,
    pub tool_mode: ToolMode,
}

/// Resolve the requested model and validate explicit effort before starting a run.
pub fn resolve_model(options: &Options) -> Result<catalog::Model, String> {
    let selected = match options.model.as_deref() {
        Some(query) => catalog::resolve(query).ok_or_else(|| {
            format!("model `{query}` is unavailable; sign in to its provider or choose a model from /model")
        })?,
        None => catalog::default_model(),
    };
    if let Some(effort) = options.effort.as_deref() {
        if !selected.effort.iter().any(|level| level == effort) {
            let supported = if selected.effort.is_empty() {
                "none".to_string()
            } else {
                selected.effort.join(", ")
            };
            return Err(format!(
                "model `{}` does not support effort `{effort}` (supported: {supported})",
                catalog::slug(&selected)
            ));
        }
    }
    Ok(selected)
}

/// Translate execution preferences into the agent's lifecycle options.
pub fn agent_options(options: &Options) -> crate::agent::AgentOptions {
    crate::agent::AgentOptions {
        save_session: !options.no_save,
        tool_mode: options.tool_mode,
        effort_override: options.effort.clone(),
        allowed_tools: None,
        ..crate::agent::AgentOptions::default()
    }
}

/// Validate image capability and load local attachments for a run.
pub fn load_images(
    options: &Options,
    model: &catalog::Model,
) -> Result<Vec<crate::providers::ImageInput>, String> {
    if !options.images.is_empty() && !model.image_input {
        return Err(format!(
            "model `{}` is not declared image-capable",
            catalog::slug(model)
        ));
    }
    crate::providers::ImageInput::from_paths(&options.images)
}

#[cfg(test)]
mod tests {
    use super::ToolMode;

    #[test]
    fn nested_tool_modes_can_only_become_more_restrictive() {
        assert_eq!(ToolMode::None.restrict(ToolMode::All), ToolMode::None);
        assert_eq!(ToolMode::All.restrict(ToolMode::None), ToolMode::None);
        assert_eq!(ToolMode::All.restrict(ToolMode::All), ToolMode::All);
    }
}
