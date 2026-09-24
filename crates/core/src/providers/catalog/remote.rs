//! The live half of the catalog: every signed-in provider's own
//! `GET /models` is fetched in the background, cached in
//! `~/.ulo/models-store.json`, and merged with the declared tables. New ids
//! appear with no ulo release, and provider-reported context windows replace
//! built-in seeds but not explicit user overrides. Failures stay silent so an
//! offline launch does not care.

use super::modelsdev::{self, FactsMap};
use super::{catalog, Deployment, Model};
use crate::providers::registry::CatalogStrategy;

/// How long a provider's fetched model list stays fresh (the reference's
/// refresh interval).
pub const REMOTE_REFRESH_MS: u64 = 4 * 60 * 60 * 1000;

fn store_path() -> std::path::PathBuf {
    crate::config::home::home().join("models-store.json")
}

/// Model ids each provider reported, from the cache. A new model a gateway
/// ships appears here on the next refresh — no ulo release involved. A
/// discovered id takes its facts (window, effort, thinking, pricing) from
/// models.dev; a window the gateway itself reports wins over that. Explicit
/// provider image settings remain final for discovered ids too.
pub(super) fn remote_overlay(
    models: &mut Vec<Model>,
    context_overrides: &std::collections::HashSet<(String, String)>,
    image_overrides: &std::collections::HashMap<String, bool>,
    facts: &FactsMap,
) {
    let object = crate::config::store::read_object(&store_path()).unwrap_or_default();
    for (provider, entry) in object {
        let Some(deployment) = known_deployment(models, &provider) else {
            continue; // only providers ulo knows how to speak to
        };
        if deployment.catalog == CatalogStrategy::None {
            // The provider's live discovery is off. A cache entry can
            // outlive that setting (written before it changed, or left
            // over from a prior config) — never resurrect it into the
            // catalog, or `catalog: "none"` would not actually disable
            // discovered models.
            continue;
        }
        for item in cached_models(&entry) {
            let Some(id) = item["id"].as_str() else {
                continue;
            };
            let window = item["context_window"].as_u64();
            match models
                .iter_mut()
                .find(|m| m.provider == provider && m.id == id)
            {
                Some(existing) => {
                    // A gateway report corrects a built-in seed. An explicit
                    // user value remains final because it may describe a
                    // deployment limit the provider's generic catalog cannot
                    // express.
                    if !context_overrides.contains(&(provider.clone(), id.to_string())) {
                        if let Some(w) = window {
                            existing.context_window = w;
                        }
                    }
                }
                None => {
                    // The endpoint reports only id/window, so start from
                    // the deployment-wide defaults retained independently
                    // from any declared sibling model's override; the
                    // feed's facts refine them.
                    let mut model = deployment.new_model(&provider, id, facts);
                    if let Some(image_input) = image_overrides.get(&provider) {
                        model.image_input = *image_input;
                    }
                    if let Some(window) = window {
                        model.context_window = window;
                    }
                    models.push(model);
                }
            }
        }
    }
}

/// Transport from an existing model of the provider (models.json overrides
/// included), else from the registry — a keyless local's whole catalog is
/// this overlay, so it has no model to copy from.
fn known_deployment(models: &[Model], provider: &str) -> Option<Deployment> {
    models
        .iter()
        .find(|m| m.provider == provider)
        .map(Deployment::from_model)
        .or_else(|| crate::providers::registry::find(provider).map(Deployment::from_builtin))
}

/// One provider's cached listing: `models` entries, or the legacy bare `ids`.
fn cached_models(entry: &serde_json::Value) -> Vec<serde_json::Value> {
    let listed = entry.get("models").and_then(|v| v.as_array()).cloned();
    let legacy = entry.get("ids").and_then(|v| v.as_array()).map(|ids| {
        ids.iter()
            .filter_map(|v| v.as_str())
            .map(|id| serde_json::json!({ "id": id }))
            .collect::<Vec<_>>()
    });
    listed.or(legacy).unwrap_or_default()
}

/// Refresh the cached model lists from every signed-in provider that serves
/// the standard `GET {base}/models`, and the models.dev facts beside them.
/// Silent on failure — an offline launch must not care. Skips providers
/// refreshed within the freshness window.
pub async fn refresh_remote() {
    refresh_remote_within(REMOTE_REFRESH_MS).await
}

/// Refresh providers whose cache is older than `max_age_ms` — the /models
/// picker calls this with a short window so a gateway's brand-new model
/// appears the moment someone looks for it.
pub async fn refresh_remote_within(max_age_ms: u64) {
    // Serialize refreshes in-process: launch, sign-in, and picker-open can
    // race, and interleaved read-merge-writes could drop a provider's entry.
    static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = REFRESH_LOCK.lock().await;
    let auth = crate::auth::load();
    let now = crate::auth::now_ms();
    let stored = crate::config::store::read_object(&store_path()).unwrap_or_default();
    // One representative model per signed-in provider gives base + auth
    // kind; catalog entries first so models.json base_url overrides win.
    // Registry providers follow so a keyless local with an empty seed list
    // (its models come only from this refresh) still gets polled.
    let mut providers: Vec<(String, Deployment)> = Vec::new();
    for m in catalog() {
        if crate::auth::signed_in(&auth, &m.provider)
            && !providers.iter().any(|(p, _)| *p == m.provider)
        {
            providers.push((m.provider.clone(), Deployment::from_model(&m)));
        }
    }
    for p in crate::providers::registry::all() {
        if crate::auth::signed_in(&auth, &p.name)
            && !providers.iter().any(|(name, _)| *name == p.name)
        {
            providers.push((p.name.clone(), Deployment::from_builtin(p)));
        }
    }
    // The facts feed is worth fetching only when there is a provider to
    // apply it to; with nothing signed in, nothing is listed either.
    if !providers.is_empty() {
        modelsdev::refresh(max_age_ms).await;
    }
    for (provider, deployment) in providers {
        if deployment.catalog == CatalogStrategy::None {
            continue;
        }
        let fresh = stored
            .get(&provider)
            .and_then(|ulo| ulo.get("checked_at"))
            .and_then(|v| v.as_u64())
            .map(|at| now.saturating_sub(at) < max_age_ms)
            .unwrap_or(false);
        if fresh {
            continue;
        }
        if let Some(models) = fetch_models(&provider, &deployment).await {
            let listed: Vec<serde_json::Value> = models
                .iter()
                .map(|(id, window)| match window {
                    Some(w) => serde_json::json!({ "id": id, "context_window": w }),
                    None => serde_json::json!({ "id": id }),
                })
                .collect();
            let entry = serde_json::json!({ "checked_at": now, "models": listed });
            let _ = crate::config::store::update(&store_path(), 0o644, |obj| {
                obj.insert(provider.clone(), entry);
            });
        }
    }
}

/// `type` values a gateway uses for models that are not chat models.
const NON_CHAT_TYPES: &[&str] = &[
    "embedding",
    "image",
    "video",
    "audio",
    "speech",
    "tts",
    "transcription",
    "moderation",
    "rerank",
];

/// Ids that are plainly not chat models — keep the picker for models a
/// coding agent can actually talk to.
fn looks_like_chat_model(id: &str) -> bool {
    const NOISE: &[&str] = &[
        "embed",
        "whisper",
        "tts",
        "audio",
        "image",
        "dall-e",
        "moderation",
        "rerank",
    ];
    let lower = id.to_lowercase();
    !NOISE.iter().any(|n| lower.contains(n))
}

/// "model-20251001" / "model-2024-05-13" is a dated alias; drop it when the
/// undated base is also in the list.
fn dated_alias_of(id: &str) -> Option<&str> {
    let (base, suffix) = id.rsplit_once('-')?;
    if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
        return Some(base);
    }
    // -YYYY-MM-DD
    if suffix.len() == 2 {
        if let Some((b2, mid)) = base.rsplit_once('-') {
            if mid.len() == 2 {
                if let Some((b3, year)) = b2.rsplit_once('-') {
                    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
                        return Some(b3);
                    }
                }
            }
        }
    }
    None
}

/// `GET {base}/models` with the provider's credential — (id, window?) per
/// listed model; None on any failure. Google lists at the same path but with
/// its own auth header and payload shape (`models[].name`, not `data[].id`).
async fn fetch_models(
    provider: &str,
    deployment: &Deployment,
) -> Option<Vec<(String, Option<u64>)>> {
    let request = models_request(provider, deployment).await?;
    let body: serde_json::Value = request.send().await.ok()?.json().await.ok()?;
    chat_models(deployment.catalog, &body)
}

/// The authorized list request for the provider's catalog strategy.
async fn models_request(
    provider: &str,
    deployment: &Deployment,
) -> Option<reqwest::RequestBuilder> {
    let strategy = deployment.catalog;
    let authorization = crate::providers::runtime::authorize_provider(
        provider,
        deployment.api,
        deployment.responses_mount,
    )
    .await
    .ok()?;
    let base = &deployment.base_url;
    // Anthropic declares the bare host as its base (the dialect appends
    // /v1 for /v1/messages); the list endpoint lives under /v1 too, so
    // fetching `{base}/models` would 404 silently on every refresh. Its
    // default page is 20 entries and pagination is not followed, so ask
    // for the whole list at once.
    let url = if strategy == CatalogStrategy::Anthropic {
        format!("{base}/v1/models?limit=1000")
    } else {
        format!("{base}/models")
    };
    let request = crate::providers::http()
        .ok()?
        .get(url)
        .timeout(std::time::Duration::from_secs(15));
    Some(match (authorization.credentialed, strategy) {
        (false, _) => request,
        (true, CatalogStrategy::Anthropic) => request
            .header("x-api-key", &authorization.bearer)
            .header("anthropic-version", "2023-06-01"),
        (true, CatalogStrategy::Google) => request.header("x-goog-api-key", &authorization.bearer),
        (true, _) => {
            let request = request.bearer_auth(&authorization.bearer);
            let request = match authorization.account_id {
                Some(account) => request.header("chatgpt-account-id", account),
                None => request,
            };
            // The ChatGPT backend answers the picker endpoints only for
            // requests that name a client; the codex mount carries the same
            // pair on every inference call.
            if strategy == CatalogStrategy::Chatgpt {
                request
                    .header("originator", "ulo")
                    .header("OpenAI-Beta", "responses=experimental")
            } else {
                request
            }
        }
    })
}

/// The chat models in a list response, dated aliases of listed bases
/// dropped; None when the payload lists none.
fn chat_models(
    strategy: CatalogStrategy,
    body: &serde_json::Value,
) -> Option<Vec<(String, Option<u64>)>> {
    let entries = match strategy {
        CatalogStrategy::Google | CatalogStrategy::Chatgpt => body["models"].as_array(),
        _ => body["data"].as_array(),
    }?;
    let all_ids: Vec<String> = entries
        .iter()
        .filter_map(|entry| wire_id(strategy, entry))
        .collect();
    let mut out = Vec::new();
    for entry in entries {
        let Some(id) = wire_id(strategy, entry) else {
            continue;
        };
        if !serves_chat(strategy, entry) || !looks_like_chat_model(&id) {
            continue;
        }
        if let Some(base_id) = dated_alias_of(&id) {
            if all_ids.iter().any(|a| a == base_id) {
                continue;
            }
        }
        out.push((id, reported_window(strategy, entry)));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The wire id: Gemini reports `models/gemini-…` and wants the bare id
/// back; ChatGPT marks codex-usable entries with a `-wm` slug suffix
/// that is the picker's own marker, not part of the model name.
fn wire_id(strategy: CatalogStrategy, entry: &serde_json::Value) -> Option<String> {
    match strategy {
        CatalogStrategy::Google => entry["name"]
            .as_str()
            .map(|n| n.strip_prefix("models/").unwrap_or(n).to_string()),
        CatalogStrategy::Chatgpt => entry["slug"]
            .as_str()
            .map(|s| s.strip_suffix("-wm").unwrap_or(s).to_string()),
        _ => entry["id"].as_str().map(String::from),
    }
}

/// Providers that report a type or capability list embeddings, images,
/// video, and speech beside chat models. Keep the picker for language
/// models: Gemini says so via supportedGenerationMethods, ChatGPT
/// via the work-mode flag (the codex lane), OpenAI-style gateways via
/// a `type` field, falling back to the id heuristic when the provider
/// doesn't say. `type` is a deny-list of known non-chat kinds, not an
/// allow-list: Anthropic tags every entry `model`, Together tags
/// instruct models `chat` and base models `language`.
fn serves_chat(strategy: CatalogStrategy, entry: &serde_json::Value) -> bool {
    match strategy {
        CatalogStrategy::Chatgpt => entry["is_work_mode_model"].as_bool().unwrap_or(false),
        CatalogStrategy::Google => entry["supportedGenerationMethods"]
            .as_array()
            .is_some_and(|ms| ms.iter().any(|m| m.as_str() == Some("generateContent"))),
        _ => entry["type"]
            .as_str()
            .is_none_or(|kind| !NON_CHAT_TYPES.contains(&kind)),
    }
}

/// Some gateways report the window; keep it when they do. Gemini's
/// inputTokenLimit is its context window as far as the picker cares,
/// and ChatGPT's max_tokens is the codex lane's own window — kept
/// strategy-scoped because an OpenAI-shaped gateway may report
/// max_tokens as an output limit, not a context window.
fn reported_window(strategy: CatalogStrategy, entry: &serde_json::Value) -> Option<u64> {
    if strategy == CatalogStrategy::Chatgpt {
        return entry["max_tokens"].as_u64();
    }
    entry["context_length"]
        .as_u64()
        .or(entry["context_window"].as_u64())
        .or(entry["max_context_length"].as_u64())
        .or(entry["inputTokenLimit"].as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::catalog::{Api, Model, Thinking};
    use crate::providers::registry::{CatalogStrategy, ResponsesMount};

    // ULO_HOME is process-global; serialize tests that set it.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn seeded_model(provider: &str, catalog: CatalogStrategy) -> Model {
        Model {
            provider: provider.into(),
            id: "seed".into(),
            base_url: "https://example.invalid".into(),
            api: Api::Completions,
            catalog,
            responses_mount: ResponsesMount::Platform,
            provider_supports_tools: true,
            provider_image_input: false,
            effort: Vec::new(),
            thinking: Thinking::Manual,
            context_window: 200_000,
            max_output: None,
            supports_tools: true,
            image_input: false,
            pricing: None,
        }
    }

    fn with_temp_home(name: &str, body: impl FnOnce(&std::path::Path)) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|ulo| ulo.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "ulo-remote-overlay-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ULO_HOME", &dir);
        body(&dir);
        std::env::remove_var("ULO_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_none_ignores_a_cached_model_the_overlay_would_otherwise_merge() {
        with_temp_home("none", |dir| {
            std::fs::write(
                dir.join("models-store.json"),
                r#"{"acme":{"models":[{"id":"stale-model","context_window":128000}]}}"#,
            )
            .unwrap();

            let mut models = vec![seeded_model("acme", CatalogStrategy::None)];
            remote_overlay(
                &mut models,
                &Default::default(),
                &Default::default(),
                &Default::default(),
            );
            assert_eq!(
                models.len(),
                1,
                "catalog: none must not resurrect a cached model"
            );
        });
    }

    #[test]
    fn a_normal_catalog_strategy_still_merges_cached_models() {
        with_temp_home("openai", |dir| {
            std::fs::write(
                dir.join("models-store.json"),
                r#"{"acme":{"models":[{"id":"discovered-model","context_window":128000}]}}"#,
            )
            .unwrap();

            let mut models = vec![seeded_model("acme", CatalogStrategy::Openai)];
            remote_overlay(
                &mut models,
                &Default::default(),
                &Default::default(),
                &Default::default(),
            );
            assert!(
                models.iter().any(|m| m.id == "discovered-model"),
                "a provider without catalog: none should still pick up cached models"
            );
        });
    }
}
