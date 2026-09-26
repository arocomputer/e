//! Model facts from the models.dev community catalog. A provider's `/models` list is
//! the truth of *which* ids it serves, but most report nothing beyond the
//! id; models.dev carries the rest — context window, effort levels, image
//! and tool support, pricing — for every provider e speaks to. Fetched in
//! the same background refresh as the providers' lists, trimmed to the
//! providers in the registry, and cached in `~/.e/models-dev.json`, so a
//! model released today is usable today with no e release. Seeds are the
//! offline fallback; explicit `models.json` values win over both.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

use super::{Api, Model, Pricing, Thinking};

const URL: &str = "https://models.dev/api.json";

fn store_path() -> std::path::PathBuf {
    crate::config::home::home().join("models-dev.json")
}

/// What models.dev knows about one model, in e's own vocabulary. Every
/// field is optional so the overlay only touches what the feed states.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Facts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effort: Vec<String>,
    /// The model reasons through a token budget rather than effort levels
    /// (the Anthropic manual-thinking shape).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub budget_thinking: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<Pricing>,
}

/// Facts keyed by `(provider, model id)`, from the cache. Empty when the
/// feed was never fetched — an offline launch runs on seeds alone.
pub type FactsMap = HashMap<(String, String), Facts>;

pub(super) fn facts() -> FactsMap {
    let object = crate::config::store::read_object(&store_path()).unwrap_or_default();
    let mut out = HashMap::new();
    let Some(providers) = object.get("providers").and_then(|v| v.as_object()) else {
        return out;
    };
    for (provider, models) in providers {
        let Some(models) = models.as_object() else {
            continue;
        };
        for (id, entry) in models {
            if let Ok(facts) = serde_json::from_value::<Facts>(entry.clone()) {
                out.insert((provider.clone(), id.clone()), facts);
            }
        }
    }
    out
}

/// Lay the feed's facts over a model. A stated value replaces the seed's
/// (seeds are a snapshot of the same feed, only older); an unstated one
/// leaves the model alone. Whether a reasoning knob is effort levels or a
/// token budget decides the Anthropic thinking wire shape: every model
/// that takes effort is an adaptive-thinking model.
pub(super) fn apply(model: &mut Model, facts: &Facts) {
    if let Some(window) = facts.context_window {
        model.context_window = window;
    }
    if !facts.effort.is_empty() {
        model.effort = facts.effort.clone();
    }
    if model.api == Api::Anthropic {
        if !facts.effort.is_empty() {
            model.thinking = Thinking::Adaptive;
        } else if facts.budget_thinking {
            model.thinking = Thinking::Manual;
        }
    }
    if let Some(image_input) = facts.image_input {
        model.image_input = image_input;
    }
    if let Some(supports_tools) = facts.supports_tools {
        // A provider declared without tool support stays that way: the
        // deployment, not the model, is what cannot carry schemas.
        model.supports_tools = supports_tools && model.provider_supports_tools;
    }
    if facts.pricing.is_some() {
        model.pricing = facts.pricing.clone();
    }
}

/// Cut the feed down to the registry's providers, keyed by e's provider
/// names, in `Facts` shape. models.dev describes every provider it knows
/// (hundreds); e caches only what it can serve.
pub(super) fn trim(feed: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    for provider in crate::providers::registry::all() {
        let Some(dev_id) = provider.models_dev_id() else {
            continue;
        };
        let Some(models) = feed[dev_id]["models"].as_object() else {
            continue;
        };
        let mut entries = Map::new();
        for (id, entry) in models {
            let facts = facts_of(entry);
            if facts == Facts::default() {
                continue;
            }
            if let Ok(value) = serde_json::to_value(facts) {
                entries.insert(id.clone(), value);
            }
        }
        if !entries.is_empty() {
            out.insert(provider.name.clone(), Value::Object(entries));
        }
    }
    out
}

/// One models.dev model record into e's facts. Rates on models.dev are USD
/// per million tokens, the same unit as `Pricing`; its single cache-write
/// rate is the five-minute one.
fn facts_of(entry: &Value) -> Facts {
    let mut facts = Facts {
        context_window: entry["limit"]["context"].as_u64(),
        ..Facts::default()
    };
    if let Some(options) = entry["reasoning_options"].as_array() {
        for option in options {
            match option["type"].as_str() {
                Some("effort") => {
                    facts.effort = option["values"]
                        .as_array()
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|v| v.as_str())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default();
                }
                Some("budget_tokens") => facts.budget_thinking = true,
                _ => {}
            }
        }
    }
    if let Some(inputs) = entry["modalities"]["input"].as_array() {
        facts.image_input = Some(inputs.iter().any(|m| m.as_str() == Some("image")));
    }
    facts.supports_tools = entry["tool_call"].as_bool();
    let cost = &entry["cost"];
    if let (Some(input), Some(output)) = (cost["input"].as_f64(), cost["output"].as_f64()) {
        facts.pricing = Some(Pricing {
            input_per_million: input,
            output_per_million: output,
            cache_read_per_million: cost["cache_read"].as_f64(),
            cache_write_5m_per_million: cost["cache_write"].as_f64(),
            cache_write_1h_per_million: None,
        });
    }
    facts
}

/// Refresh the cached facts when they are older than `max_age_ms`. The
/// feed is one static file with an ETag, so a check that finds nothing new
/// costs a 304 and no body. Silent on any failure — the cache on disk, or
/// the seeds, carry on.
pub(super) async fn refresh(max_age_ms: u64) {
    let now = crate::auth::now_ms();
    let path = store_path();
    let stored = crate::config::store::read_object(&path).unwrap_or_default();
    let fresh = stored
        .get("checked_at")
        .and_then(|v| v.as_u64())
        .map(|at| now.saturating_sub(at) < max_age_ms)
        .unwrap_or(false);
    if fresh {
        return;
    }
    let Ok(client) = crate::providers::http() else {
        return;
    };
    let mut request = client.get(URL).timeout(std::time::Duration::from_secs(15));
    if let Some(etag) = stored.get("etag").and_then(|v| v.as_str()) {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let Ok(response) = request.send().await else {
        return;
    };
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        let _ = crate::config::store::update(&path, 0o644, |obj| {
            obj.insert("checked_at".into(), now.into());
        });
        return;
    }
    if !response.status().is_success() {
        return;
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let Ok(feed) = response.json::<Value>().await else {
        return;
    };
    let providers = trim(&feed);
    if providers.is_empty() {
        return; // not the feed we know; keep what we have
    }
    let _ = crate::config::store::update(&path, 0o644, |obj| {
        obj.insert("checked_at".into(), now.into());
        match etag {
            Some(etag) => obj.insert("etag".into(), etag.into()),
            None => obj.remove("etag"),
        };
        obj.insert("providers".into(), Value::Object(providers));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_keeps_registry_providers_under_their_e_names_in_facts_shape() {
        let feed = serde_json::json!({
            "anthropic": { "models": {
                "claude-new": {
                    "limit": { "context": 1000000, "output": 128000 },
                    "reasoning_options": [{ "type": "effort", "values": ["low", "high"] }],
                    "modalities": { "input": ["text", "image"] },
                    "tool_call": true,
                    "cost": { "input": 10, "output": 50, "cache_read": 1, "cache_write": 12.5 }
                },
                "claude-budget": {
                    "reasoning_options": [{ "type": "budget_tokens", "min": 1024 }]
                }
            }},
            // Together is `togetherai` on models.dev; the cache uses e's name.
            "togetherai": { "models": { "vendor/model": { "limit": { "context": 65536 } } } },
            // The codex deployment opts out of the feed entirely.
            "openai": { "models": { "gpt-x": { "limit": { "context": 1050000 } } } },
            "someone-else": { "models": { "m": { "limit": { "context": 1 } } } }
        });
        let trimmed = trim(&feed);
        let claude: Facts =
            serde_json::from_value(trimmed["anthropic"]["claude-new"].clone()).unwrap();
        assert_eq!(claude.context_window, Some(1_000_000));
        assert_eq!(claude.effort, vec!["low".to_string(), "high".to_string()]);
        assert!(!claude.budget_thinking);
        assert_eq!(claude.image_input, Some(true));
        assert_eq!(claude.supports_tools, Some(true));
        let pricing = claude.pricing.unwrap();
        assert_eq!(pricing.input_per_million, 10.0);
        assert_eq!(pricing.cache_write_5m_per_million, Some(12.5));
        assert_eq!(pricing.cache_write_1h_per_million, None);
        let budget: Facts =
            serde_json::from_value(trimmed["anthropic"]["claude-budget"].clone()).unwrap();
        assert!(budget.budget_thinking && budget.effort.is_empty());
        assert_eq!(
            trimmed["together"]["vendor/model"]["context_window"].as_u64(),
            Some(65_536)
        );
        assert!(
            trimmed.get("openai").is_some(),
            "the platform API maps by its own name"
        );
        assert!(trimmed.get("openai-codex").is_none(), "codex opts out");
        assert!(trimmed.get("someone-else").is_none());
    }

    #[test]
    fn apply_states_only_what_the_feed_states_and_picks_the_thinking_shape() {
        let seed = |api: Api| Model {
            provider: "p".into(),
            id: "m".into(),
            base_url: "https://example.invalid".into(),
            api,
            catalog: Default::default(),
            responses_mount: Default::default(),
            provider_supports_tools: true,
            provider_image_input: false,
            effort: vec!["low".into()],
            thinking: Thinking::Manual,
            context_window: 200_000,
            max_output: Some(8192),
            supports_tools: true,
            image_input: false,
            pricing: None,
        };
        let mut model = seed(Api::Anthropic);
        apply(
            &mut model,
            &Facts {
                context_window: Some(1_000_000),
                effort: vec!["low".into(), "max".into()],
                ..Facts::default()
            },
        );
        assert_eq!(model.context_window, 1_000_000);
        assert_eq!(model.effort, vec!["low".to_string(), "max".to_string()]);
        assert_eq!(
            model.thinking,
            Thinking::Adaptive,
            "effort levels mean adaptive"
        );
        assert_eq!(
            model.max_output,
            Some(8192),
            "the feed never touches the output ceiling"
        );

        let mut model = seed(Api::Anthropic);
        apply(
            &mut model,
            &Facts {
                budget_thinking: true,
                ..Facts::default()
            },
        );
        assert_eq!(
            model.effort,
            vec!["low".to_string()],
            "no stated effort keeps the seed's"
        );
        assert_eq!(model.thinking, Thinking::Manual);

        let mut model = seed(Api::Completions);
        model.provider_supports_tools = false;
        model.supports_tools = false;
        apply(
            &mut model,
            &Facts {
                effort: vec!["high".into()],
                supports_tools: Some(true),
                ..Facts::default()
            },
        );
        assert_eq!(
            model.thinking,
            Thinking::Manual,
            "thinking is an Anthropic-dialect fact"
        );
        assert!(
            !model.supports_tools,
            "a tool-less deployment stays tool-less"
        );
    }
}
