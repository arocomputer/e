//! The model catalog: which models exist, what they speak, and resolving a
//! pick to a Model. Built-ins come from the provider registry (data), the
//! models.dev facts (`modelsdev.rs`) bring their windows, effort levels,
//! and pricing up to date, live remote sync (`remote.rs`) adds the ids each
//! provider actually serves, and explicit `~/.e/models.json` values win over
//! all of it. The active model comes from `~/.e/settings.json`
//! `{"model": "provider/id"}` or a `/model` switch at runtime.

use serde::Deserialize;

use crate::config::home;

mod modelsdev;
mod remote;
use remote::remote_overlay;
pub use remote::{refresh_remote, refresh_remote_within, REMOTE_REFRESH_MS};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Api {
    /// OpenAI chat-completions dialect (`/chat/completions`, SSE deltas).
    Completions,
    /// The responses dialect behind the ChatGPT backend (OAuth + account id).
    Responses,
    /// The Anthropic Messages dialect (`/v1/messages`, x-api-key).
    Anthropic,
    /// The Gemini dialect (`:streamGenerateContent?alt=sse`, x-goog-api-key).
    Google,
}

impl Api {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completions => "openai-completions",
            Self::Responses => "openai-responses",
            Self::Anthropic => "anthropic-messages",
            Self::Google => "google-generative-ai",
        }
    }

    /// Parse a dialect name from provider JSON / models.json. Unknown strings
    /// are `None` — callers decide whether to panic (built-ins) or fall back
    /// (user file).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "openai-completions" | "completions" => Some(Self::Completions),
            "codex-responses" | "openai-responses" | "responses" => Some(Self::Responses),
            "anthropic-messages" | "anthropic" => Some(Self::Anthropic),
            "google-generative-ai" | "google" => Some(Self::Google),
            _ => None,
        }
    }
}

/// How the model takes its reasoning knob. Adaptive models (Claude 4.7+)
/// reject the legacy manual-thinking shape with a 400, so this is declared
/// per model in provider data and rides the request through to the
/// Anthropic dialect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Thinking {
    /// `thinking: {"type": "adaptive"}` plus `output_config.effort`.
    Adaptive,
    /// Legacy `thinking: {"type": "enabled", "budget_tokens": N}`.
    Manual,
}

impl Thinking {
    fn parse(value: &str) -> Option<Thinking> {
        match value {
            "adaptive" => Some(Thinking::Adaptive),
            "manual" => Some(Thinking::Manual),
            _ => None,
        }
    }

    fn from_decl(value: Option<&str>) -> Thinking {
        match value {
            Some("adaptive") => Thinking::Adaptive,
            // Undeclared keeps today's wire shape; only data opts into
            // adaptive, so user-declared models never change behavior.
            _ => Thinking::Manual,
        }
    }
}

/// Optional USD rates per million tokens. Pricing changes independently of
/// protocol support, so built-ins may omit it; users and gateways can declare
/// authoritative rates in models.json without waiting for an e release.
#[derive(Clone, Debug, Deserialize, serde::Serialize, PartialEq)]
pub struct Pricing {
    pub input_per_million: f64,
    pub output_per_million: f64,
    #[serde(default)]
    pub cache_read_per_million: Option<f64>,
    #[serde(default)]
    pub cache_write_5m_per_million: Option<f64>,
    #[serde(default)]
    pub cache_write_1h_per_million: Option<f64>,
}

impl Pricing {
    /// Price disjoint provider counters; undeclared cache rates conservatively
    /// fall back to ordinary input rather than dropping billed tokens.
    pub fn estimate(&self, usage: super::Usage) -> f64 {
        let cache_read = self
            .cache_read_per_million
            .unwrap_or(self.input_per_million);
        let cache_write_5m = self
            .cache_write_5m_per_million
            .unwrap_or(self.input_per_million);
        let cache_write_1h = self
            .cache_write_1h_per_million
            .unwrap_or(self.input_per_million);
        (usage.input as f64 * self.input_per_million
            + usage.output as f64 * self.output_per_million
            + usage.cache_read as f64 * cache_read
            + usage.cache_write_5m as f64 * cache_write_5m
            + usage.cache_write_1h as f64 * cache_write_1h)
            / 1_000_000.0
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub provider: String,
    pub id: String,
    pub base_url: String,
    pub api: Api,
    pub catalog: crate::providers::registry::CatalogStrategy,
    pub responses_mount: crate::providers::registry::ResponsesMount,
    /// Provider-wide defaults retained separately from per-model overrides,
    /// so a remotely discovered id never inherits an arbitrary sibling's
    /// narrower compatibility declaration.
    pub provider_supports_tools: bool,
    pub provider_image_input: bool,
    /// Effort values the backend accepts for its reasoning knob, if any.
    pub effort: Vec<String>,
    /// Which thinking wire shape the backend accepts for this model.
    pub thinking: Thinking,
    /// Context window in tokens. A provider report replaces the built-in
    /// seed, while an explicit models.json value replaces both.
    pub context_window: u64,
    /// Output ceiling in tokens, when the model's own limit is below the
    /// dialect's default. `None` leaves the dialect's own constant in force.
    pub max_output: Option<u64>,
    /// Whether tool schemas/calls are supported for this model deployment.
    pub supports_tools: bool,
    /// Whether this model accepts image input (the frontend also needs an
    /// image-capable message path before it advertises an attachment).
    pub image_input: bool,
    pub pricing: Option<Pricing>,
}

pub fn slug(model: &Model) -> String {
    format!("{}/{}", model.provider, model.id)
}

pub fn builtin_catalog() -> Vec<Model> {
    // Providers are data (providers/data/*.json); this just projects the
    // registry into models.
    crate::providers::registry::all()
        .iter()
        .flat_map(|provider| {
            provider.models.iter().map(|decl| Model {
                provider: provider.name.clone(),
                id: decl.id.clone(),
                base_url: provider.base_url.clone(),
                api: provider.api(),
                catalog: provider.catalog,
                responses_mount: provider.responses_mount,
                provider_supports_tools: provider.supports_tools,
                provider_image_input: provider.image_input,
                effort: decl.effort.clone(),
                thinking: Thinking::from_decl(decl.thinking.as_deref()),
                context_window: decl.context_window,
                max_output: decl.max_output,
                supports_tools: decl.supports_tools && provider.supports_tools,
                image_input: decl.image_input || provider.image_input,
                pricing: decl.pricing.clone(),
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct ModelsFile {
    providers: std::collections::BTreeMap<String, ProviderEntry>,
}

#[derive(Deserialize)]
struct ProviderEntry {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    catalog: Option<crate::providers::registry::CatalogStrategy>,
    #[serde(default)]
    responses_mount: Option<crate::providers::registry::ResponsesMount>,
    #[serde(default)]
    effort: Option<Vec<String>>,
    /// Default window for this provider's models; each model may override.
    #[serde(default)]
    context_window: Option<u64>,
    #[serde(default)]
    thinking: Option<String>,
    /// Default output ceiling for this provider's models; each model may
    /// override.
    #[serde(default)]
    max_output: Option<u64>,
    #[serde(default)]
    supports_tools: Option<bool>,
    #[serde(default)]
    image_input: Option<bool>,
    #[serde(default)]
    pricing: Option<Pricing>,
    #[serde(default)]
    models: Vec<ModelEntry>,
}

/// A model in models.json: a bare id string, or an object when the model
/// needs its own context window or effort levels.
#[derive(Deserialize)]
#[serde(untagged)]
enum ModelEntry {
    Id(String),
    Detailed(UserModel),
}

/// One model's own declarations in models.json; anything unset falls back
/// to its provider entry, then to what the model already had.
#[derive(Deserialize, Default)]
struct UserModel {
    id: String,
    #[serde(default)]
    context_window: Option<u64>,
    #[serde(default)]
    effort: Vec<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    max_output: Option<u64>,
    #[serde(default)]
    supports_tools: Option<bool>,
    #[serde(default)]
    image_input: Option<bool>,
    #[serde(default)]
    pricing: Option<Pricing>,
}

impl ModelEntry {
    /// A bare id declares nothing but itself.
    fn into_user_model(self) -> UserModel {
        match self {
            ModelEntry::Id(id) => UserModel {
                id,
                ..UserModel::default()
            },
            ModelEntry::Detailed(decl) => decl,
        }
    }
}

/// Configuration problems that caused user-declared providers to be omitted.
/// Callers surface these in their own UI; the catalog itself stays data-only.
pub fn config_warnings() -> Vec<String> {
    let path = home::home().join("models.json");
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => return vec![format!("models.json: cannot read configuration: {error}")],
    };
    let file = match serde_json::from_str::<ModelsFile>(&json) {
        Ok(file) => file,
        Err(error) => return vec![format!("models.json: invalid configuration: {error}")],
    };
    let mut warnings = Vec::new();
    for (provider, entry) in file.providers {
        if entry.base_url.is_none() && crate::providers::registry::find(&provider).is_none() {
            warnings.push(format!(
                "models.json: provider {provider} requires an explicit base_url"
            ));
        }
        if let Some(api) = entry.api.as_deref().filter(|api| Api::parse(api).is_none()) {
            warnings.push(format!(
                "models.json: provider {provider}: unknown api dialect `{api}`"
            ));
        }
        if let Some(thinking) = entry
            .thinking
            .as_deref()
            .filter(|thinking| Thinking::parse(thinking).is_none())
        {
            warnings.push(format!(
                "models.json: provider {provider}: unknown thinking mode `{thinking}`"
            ));
        }
        for model in entry.models {
            if let ModelEntry::Detailed(UserModel {
                id,
                thinking: Some(thinking),
                ..
            }) = model
            {
                if Thinking::parse(&thinking).is_none() {
                    warnings.push(format!("models.json: provider {provider}, model {id}: unknown thinking mode `{thinking}`"));
                }
            }
        }
    }
    warnings
}

/// Built-ins plus `~/.e/models.json` — and the file wins on a name clash,
/// the same rule as themes: never override what the user declared.
pub fn catalog() -> Vec<Model> {
    let mut models = builtin_catalog();
    // Seeds are a snapshot of the feed; the cached feed is newer.
    let facts = modelsdev::facts();
    for model in &mut models {
        if let Some(facts) = facts.get(&(model.provider.clone(), model.id.clone())) {
            modelsdev::apply(model, facts);
        }
    }
    let mut overrides = Overrides::default();
    if let Some(file) = models_file() {
        for (provider, entry) in file.providers {
            apply_provider(&mut models, &mut overrides, &facts, provider, entry);
        }
    }
    // The overlay runs last so it can attach to user-declared providers too.
    // It adds unclaimed ids (with their feed facts) and replaces seed windows
    // with live reports, but never replaces a context window the user
    // explicitly declared.
    remote_overlay(&mut models, &overrides.context, &overrides.image, &facts);
    models
}

/// `~/.e/models.json`, when it exists and parses; `config_warnings` reports
/// why it doesn't.
fn models_file() -> Option<ModelsFile> {
    let json = std::fs::read_to_string(home::home().join("models.json")).ok()?;
    serde_json::from_str(&json).ok()
}

/// What models.json explicitly declared, for the remote overlay to respect.
#[derive(Default)]
struct Overrides {
    /// Keep the source of a resolved window long enough for the remote
    /// overlay to distinguish a built-in fallback from the user's final value.
    context: std::collections::HashSet<(String, String)>,
    /// Discovery must distinguish an explicit image setting from a default,
    /// keyed by provider.
    image: std::collections::HashMap<String, bool>,
}

/// A provider's transport and provider-wide capabilities, shared by every
/// model it serves: resolved from a models.json entry, copied from an
/// assembled model, or read from the registry.
struct Deployment {
    base_url: String,
    api: Api,
    catalog: crate::providers::registry::CatalogStrategy,
    responses_mount: crate::providers::registry::ResponsesMount,
    supports_tools: bool,
    image_input: bool,
}

impl Deployment {
    /// Resolve the entry against the built-in provider of the same name.
    /// `None` skips the entry: an unknown dialect, or no base URL at all.
    fn from_entry(provider: &str, entry: &ProviderEntry) -> Option<Self> {
        // A partial entry — "correct this one field" — must inherit
        // the built-in provider's transport and defaults rather than
        // silently swapping dialect and endpoint. Otherwise tweaking
        // a context window on an Anthropic model would send that
        // model's requests (and its credential) to an unrelated
        // gateway's Chat Completions endpoint.
        let builtin = crate::providers::registry::find(provider);
        let api = match entry.api.as_deref() {
            // An unknown dialect skips the entry and keeps the built-in
            // provider intact; an invalid user override is a configuration
            // warning, never a process-wide panic.
            Some(name) => Api::parse(name)?,
            None => builtin.map(|p| p.api()).unwrap_or(Api::Completions),
        };
        let catalog = entry
            .catalog
            .or_else(|| builtin.map(|provider| provider.catalog))
            .unwrap_or_default();
        let responses_mount = entry
            .responses_mount
            .or_else(|| builtin.map(|provider| provider.responses_mount))
            .unwrap_or_default();
        let supports_tools = entry
            .supports_tools
            .or_else(|| builtin.map(|provider| provider.supports_tools))
            .unwrap_or(true);
        let image_input = entry
            .image_input
            .or_else(|| builtin.map(|provider| provider.image_input))
            .unwrap_or(false);
        let base_url = entry
            .base_url
            .clone()
            .or_else(|| builtin.map(|p| p.base_url.clone()))?;
        Some(Self {
            base_url,
            api,
            catalog,
            responses_mount,
            supports_tools,
            image_input,
        })
    }

    /// The deployment an assembled model already carries (models.json
    /// overrides included).
    fn from_model(model: &Model) -> Self {
        Self {
            base_url: model.base_url.clone(),
            api: model.api,
            catalog: model.catalog,
            responses_mount: model.responses_mount,
            supports_tools: model.provider_supports_tools,
            image_input: model.provider_image_input,
        }
    }

    /// A registry provider's own deployment.
    fn from_builtin(provider: &crate::providers::registry::Provider) -> Self {
        Self {
            base_url: provider.base_url.clone(),
            api: provider.api(),
            catalog: provider.catalog,
            responses_mount: provider.responses_mount,
            supports_tools: provider.supports_tools,
            image_input: provider.image_input,
        }
    }

    /// Point a model at this deployment.
    fn apply(&self, model: &mut Model) {
        model.base_url = self.base_url.clone();
        model.api = self.api;
        model.catalog = self.catalog;
        model.responses_mount = self.responses_mount;
        model.provider_supports_tools = self.supports_tools;
        model.provider_image_input = self.image_input;
    }

    /// A model this deployment serves that no seed declares: provider
    /// defaults, then its feed facts.
    fn new_model(&self, provider: &str, id: &str, facts: &modelsdev::FactsMap) -> Model {
        let mut model = Model {
            provider: provider.to_string(),
            id: id.to_string(),
            base_url: self.base_url.clone(),
            api: self.api,
            catalog: self.catalog,
            responses_mount: self.responses_mount,
            provider_supports_tools: self.supports_tools,
            provider_image_input: self.image_input,
            effort: Vec::new(),
            thinking: Thinking::Manual,
            context_window: 200_000,
            max_output: None,
            supports_tools: self.supports_tools,
            image_input: self.image_input,
            pricing: None,
        };
        if let Some(facts) = facts.get(&(provider.to_string(), id.to_string())) {
            modelsdev::apply(&mut model, facts);
        }
        model
    }
}

/// Fold one models.json provider entry into the catalog: its deployment and
/// defaults onto the seeds, then each model it declares.
fn apply_provider(
    models: &mut Vec<Model>,
    overrides: &mut Overrides,
    facts: &modelsdev::FactsMap,
    provider: String,
    mut entry: ProviderEntry,
) {
    let Some(deployment) = Deployment::from_entry(&provider, &entry) else {
        return;
    };
    if let Some(image_input) = entry.image_input {
        overrides.image.insert(provider.clone(), image_input);
    }
    // Provider fields describe one deployment and apply to its
    // built-in seed models too — transport, capabilities, and
    // the defaults (window, output ceiling, effort, thinking,
    // pricing) alike. Per-model declarations below can still
    // narrow capabilities without changing what newly
    // discovered sibling ids inherit.
    for existing in models.iter_mut().filter(|m| m.provider == provider) {
        deployment.apply(existing);
        apply_provider_defaults(existing, &entry);
        if entry.context_window.is_some() {
            overrides
                .context
                .insert((provider.clone(), existing.id.clone()));
        }
    }
    for declared in std::mem::take(&mut entry.models)
        .into_iter()
        .map(ModelEntry::into_user_model)
    {
        // Inherit the assembled seed, or start a non-seed id from
        // its feed facts. Explicit values below win in either case.
        let existing = models
            .iter()
            .find(|m| m.provider == provider && m.id == declared.id)
            .cloned()
            .unwrap_or_else(|| deployment.new_model(&provider, &declared.id, facts));
        let has_context_override =
            declared.context_window.is_some() || entry.context_window.is_some();
        let Some(resolved) = declared_model(&provider, &entry, &deployment, declared, existing)
        else {
            continue;
        };
        models.retain(|m| !(m.provider == resolved.provider && m.id == resolved.id));
        if has_context_override {
            overrides
                .context
                .insert((resolved.provider.clone(), resolved.id.clone()));
        }
        models.push(resolved);
    }
}

/// A provider entry's explicit defaults, applied over a seed model.
fn apply_provider_defaults(model: &mut Model, entry: &ProviderEntry) {
    if let Some(supports_tools) = entry.supports_tools {
        model.supports_tools = supports_tools;
    }
    if let Some(image_input) = entry.image_input {
        model.image_input = image_input;
    }
    if let Some(window) = entry.context_window {
        model.context_window = window;
    }
    if let Some(max_output) = entry.max_output {
        model.max_output = Some(max_output);
    }
    if let Some(effort) = entry.effort.as_ref().filter(|e| !e.is_empty()) {
        model.effort = effort.clone();
    }
    if let Some(thinking) = entry.thinking.as_deref().and_then(Thinking::parse) {
        model.thinking = thinking;
    }
    if let Some(pricing) = &entry.pricing {
        model.pricing = Some(pricing.clone());
    }
}

/// A declared model: its own values, then the provider entry's, then what
/// `existing` already had. `None` when the thinking mode it would use is
/// malformed.
fn declared_model(
    provider: &str,
    entry: &ProviderEntry,
    deployment: &Deployment,
    declared: UserModel,
    existing: Model,
) -> Option<Model> {
    let effort = if !declared.effort.is_empty() {
        // Per-model declaration wins…
        declared.effort
    } else {
        match &entry.effort {
            // …then the per-provider default from the file…
            Some(e) if !e.is_empty() => e.clone(),
            // …then the model's own effort.
            _ => existing.effort,
        }
    };
    let thinking = match declared.thinking.as_ref().or(entry.thinking.as_ref()) {
        // A malformed per-model declaration should
        // not make `doctor` or startup unusable.
        Some(t) => Thinking::parse(t)?,
        // …then the model's own declaration.
        None => existing.thinking,
    };
    Some(Model {
        provider: provider.to_string(),
        id: declared.id,
        base_url: deployment.base_url.clone(),
        api: deployment.api,
        catalog: deployment.catalog,
        responses_mount: deployment.responses_mount,
        provider_supports_tools: deployment.supports_tools,
        provider_image_input: deployment.image_input,
        effort,
        thinking,
        context_window: declared
            .context_window
            .or(entry.context_window)
            .unwrap_or(existing.context_window),
        max_output: declared
            .max_output
            .or(entry.max_output)
            .or(existing.max_output),
        supports_tools: declared
            .supports_tools
            .or(entry.supports_tools)
            .unwrap_or(existing.supports_tools),
        image_input: declared
            .image_input
            .or(entry.image_input)
            .unwrap_or(existing.image_input),
        pricing: declared
            .pricing
            .or_else(|| entry.pricing.clone())
            .or(existing.pricing),
    })
}

#[derive(Deserialize, Default)]
struct Settings {
    #[serde(default)]
    model: Option<String>,
}

pub const DEFAULT_MODEL: &str = "opencode-go/deepseek-v4-flash";

/// The catalog cut to providers with credentials — the models e can
/// actually serve. Everything user-facing (the picker, resolution, the
/// default) works on this set; the full catalog is data, not a menu.
pub fn available() -> Vec<Model> {
    let auth = crate::auth::load();
    catalog()
        .into_iter()
        .filter(|m| crate::auth::signed_in(&auth, &m.provider))
        .collect()
}

/// The configured model if its provider is signed in; otherwise the first
/// available model; with no credentials at all, the catalog default (the
/// startup warning covers that state).
pub fn default_model() -> Model {
    let wanted = std::fs::read_to_string(home::settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .and_then(|s| s.model);
    if let Some(wanted) = &wanted {
        if let Some(m) = resolve(wanted) {
            return m;
        }
    }
    // The catalog embeds DEFAULT_MODEL, so this resolves for any shipped
    // data; if the embedded data were malformed, that is a build bug CI
    // catches, not a runtime state. Scoped allow, proof: compile-time data.
    #[allow(clippy::expect_used)]
    available()
        .into_iter()
        .next()
        .or_else(|| resolve_in(&catalog(), DEFAULT_MODEL))
        .expect("builtin default")
}

/// Resolve `provider/id`, a bare id, or a unique substring — among the
/// available models only, so a pick is always usable.
pub fn resolve(query: &str) -> Option<Model> {
    resolve_in(&available(), query)
}

fn resolve_in(models: &[Model], query: &str) -> Option<Model> {
    if let Some(m) = models.iter().find(|m| slug(m) == query || m.id == query) {
        return Some(m.clone());
    }
    let matches: Vec<&Model> = models.iter().filter(|m| slug(m).contains(query)).collect();
    if matches.len() == 1 {
        return Some(matches[0].clone());
    }
    None
}

/// The scoped-model ids ("provider/id"), or None when no scope is set.
pub fn scope() -> Option<Vec<String>> {
    crate::config::settings::get_strings("scoped_models")
}

/// Back to no scope at all: ctrl+p cycles everything again.
pub fn clear_scope() -> std::io::Result<()> {
    crate::config::settings::remove("scoped_models")
}

/// Sort models for a picker: grouped by provider in registry order (unknown
/// providers after, alphabetically), original order within a provider.
pub fn provider_grouped(mut models: Vec<Model>) -> Vec<Model> {
    let registry_pos = |provider: &str| {
        crate::providers::registry::all()
            .iter()
            .position(|p| p.name == provider)
            .unwrap_or(usize::MAX)
    };
    models.sort_by(|a, b| {
        registry_pos(&a.provider)
            .cmp(&registry_pos(&b.provider))
            .then_with(|| a.provider.cmp(&b.provider))
    });
    models
}

pub fn set_scope(ids: &[String]) -> std::io::Result<()> {
    // Empty is reset, not a scope of nothing — `Some([])` would leave the
    // picker showing no marks and ctrl+p cycling a dead pool.
    if ids.is_empty() {
        return clear_scope();
    }
    crate::config::settings::set_strings("scoped_models", ids)
}

/// The models ctrl+p cycles: the scope filtered to what is signed in, or —
/// with no scope — everything available (the reference behavior).
pub fn cycle_pool() -> Vec<Model> {
    let available = available();
    match scope() {
        Some(ids) if !ids.is_empty() => available
            .into_iter()
            .filter(|m| ids.iter().any(|id| *id == slug(m)))
            .collect(),
        _ => available,
    }
}

/// Human name for a provider, for panels: capitalized, no dashes.
pub fn display_name(provider: &str) -> String {
    crate::providers::registry::find(provider)
        .map(|p| p.display.clone())
        .unwrap_or_else(|| provider.to_string())
}

#[cfg(test)]
mod pricing_tests {
    use super::Pricing;

    #[test]
    fn cached_input_uses_its_own_rate_without_double_charging() {
        let pricing = Pricing {
            input_per_million: 2.0,
            output_per_million: 8.0,
            cache_read_per_million: Some(0.5),
            cache_write_5m_per_million: Some(2.5),
            cache_write_1h_per_million: Some(4.0),
        };
        let cost = pricing.estimate(crate::providers::Usage {
            input: 750_000,
            output: 100_000,
            cache_read: 250_000,
            cache_write_5m: 100_000,
            cache_write_1h: 50_000,
        });
        assert!((cost - 2.875).abs() < f64::EPSILON);
    }
}
