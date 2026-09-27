---
title: Models & providers
description: Add models and correct built-in ones in models.json.
order: 1
---

# Models & providers

This guide covers signing in to a provider, choosing models, and
`~/.e/models.json`, which adds providers and models and corrects the built-in
ones. Read it to use a local server or gateway, fix a model's context window
or effort levels, or declare pricing.

## Choose a model

`/models` lists the models of every provider you are signed in to, and
remembers your pick as `model` in [settings](settings.md#agent). To cycle a
shortlist, choose it with `/scoped-models`, then press Ctrl+P (Ctrl+Shift+P
goes backward). Shift+Tab cycles the model's reasoning effort.

## Credentials

`/login` signs in with an account or an API key. `/login <provider>` goes
straight to one provider, and accepts any provider name, including one you
declared in `models.json`. Keys are stored in `~/.e/auth.json`.

A built-in provider with no stored credential falls back to its environment
variable, which is what CI and scripts want. `auth.json` wins when both
exist.

| Provider | Sign-in | Environment variable |
| --- | --- | --- |
| `anthropic` | key | `ANTHROPIC_API_KEY` |
| `openai` | key | `OPENAI_API_KEY` |
| `openai-codex` | ChatGPT account (browser) | none |
| `google` | key | `GEMINI_API_KEY` |
| `xai` | SuperGrok or X Premium account (device code), or key | `XAI_API_KEY` |
| `groq` | key | `GROQ_API_KEY` |
| `mistral` | key | `MISTRAL_API_KEY` |
| `deepseek` | key | `DEEPSEEK_API_KEY` |
| `cerebras` | key | `CEREBRAS_API_KEY` |
| `openrouter` | key | `OPENROUTER_API_KEY` |
| `together` | key | `TOGETHER_API_KEY` |
| `fireworks` | key | `FIREWORKS_API_KEY` |
| `opencode-zen` | key | `OPENCODE_API_KEY` |
| `opencode-go` | key | `OPENCODE_GO_API_KEY` |
| `vercel` | key | `AI_GATEWAY_API_KEY` |
| `ollama` | none, `localhost:11434` | none |
| `lmstudio` | none, `localhost:1234` | none |

Local backends need no credential. They count as signed in, and their models
appear as soon as the local server answers `/models`.

## Add a provider or model

Declare providers under `providers`, keyed by the provider name. A model is
a bare id or an object:

```json
{
  "providers": {
    "local": {
      "base_url": "http://localhost:8080/v1",
      "api": "openai-completions",
      "context_window": 64000,
      "models": [
        "small-model",
        {
          "id": "big-model",
          "context_window": 1000000,
          "image_input": true,
          "pricing": {
            "input_per_million": 1.0,
            "output_per_million": 4.0
          }
        }
      ]
    }
  }
}
```

Then run `/login local` to store its key. A provider you declare appears in
`/models` only once it has a stored credential. e rereads `models.json`
whenever it lists models, so edits need no restart.

## Correct a built-in model

An entry with a built-in provider's name inherits that provider's endpoint,
dialect, and defaults, so name only what you want to change:

```json
{
  "providers": {
    "anthropic": {
      "models": [{ "id": "claude-haiku-4-5", "max_output": 8192 }]
    }
  }
}
```

A model entry with a built-in model's id replaces that model. Fields you
leave out keep the values it already had. Fields set on the provider apply
to all of its models, built-in ones included.

## Fields

A provider entry takes every field in this table except `id`. A model object
takes `id` and the fields marked as model fields; a model field on the
provider is the default for its models.

| Field | Level | Default | Meaning |
| --- | --- | --- | --- |
| `base_url` | provider | the built-in provider's | The endpoint. Required for a provider that is not built in; e never guesses a host. |
| `api` | provider | the built-in provider's, else `openai-completions` | The wire dialect. See [endpoint and dialect](#endpoint-and-dialect). |
| `responses_mount` | provider | `platform` | The Responses request path. |
| `catalog` | provider | the built-in provider's, else `openai` | How e discovers the provider's models. See [model discovery](#model-discovery). |
| `models` | provider | `[]` | Model ids or objects. |
| `id` | model | required | The model id the provider expects. |
| `context_window` | model | `200000` for an unknown model | Context size in tokens. Drives the context percentage and automatic compaction, so set it truthfully. |
| `max_output` | model | the dialect's own ceiling | Caps reply tokens, for models whose real limit is lower. Only the Anthropic dialect reads it. |
| `effort` | model | see [reasoning effort](#reasoning-effort) | The reasoning levels, in cycle order. |
| `thinking` | model | `manual` for a new model | `adaptive` or `manual`: how the Anthropic dialect asks for reasoning. |
| `supports_tools` | model | `true` | `false` sends no tool schemas, so the model cannot run tools. |
| `image_input` | model | `false` | Whether the model accepts images. |
| `pricing` | model | none | USD rates. See [pricing](#pricing). |

An entry with an unknown `api` or `thinking` value, or a new provider with no
`base_url`, is skipped, and a built-in provider keeps its own settings. e
reports the reason at launch, in `e doctor`, and in `e -p` and `e rpc`
output.

## Endpoint and dialect

`api` is one of `openai-completions`, `openai-responses`, `codex-responses`,
`anthropic-messages`, or `google-generative-ai`. The short aliases
`completions`, `responses`, `anthropic`, and `google` work too.

`responses_mount` applies only to the Responses dialect. e sets it from this
field alone, never from whether the credential is a key or an account.

| `responses_mount` | Request path | Notes |
| --- | --- | --- |
| `platform` | `{base_url}/responses` | Default. |
| `codex` | `{base_url}/codex/responses` | Adds the ChatGPT account headers. |

## Model discovery

`catalog` controls only how e lists a provider's models, independent of
`api`. That matters for gateways that accept one dialect but list models in
another provider's shape.

| `catalog` | How e reads the model list |
| --- | --- |
| `openai` | `GET {base_url}/models`, reading `data[].id`. |
| `anthropic` | `GET {base_url}/v1/models`, with `x-api-key`. |
| `google` | `GET {base_url}/models`, reading `models[].name`, with `x-goog-api-key`. |
| `chatgpt` | The ChatGPT backend's picker list. e reads `models[].slug`, keeps work-mode entries, strips their `-wm` suffix, and uses `max_tokens` as the context window. |
| `none` | No discovery; only declared models appear. |

## Reasoning effort

`effort` lists the model's reasoning levels in the order Shift+Tab cycles
them, for example `["low", "medium", "high", "xhigh"]`. A model with no
levels has no effort control.

A model entry without `effort` takes its levels from the first source that
has them:

1. the provider entry's `effort`
2. what models.dev states for the id
3. the built-in model's declaration

e sends the level as the exact string in `reasoning_effort` or the dialect's
equivalent, so the levels must be ones the backend accepts. For example,
opencode-go's `glm-5.3-flash` takes `["low", "high", "max"]`, with no
`medium`, and the gateway's own list does not say so.

## Capabilities

A model declared with `supports_tools: false` gets no tool schemas and cannot
run a tool even if it emits a call. A provider-level `supports_tools: false`
wins over what models.dev says.

A model e discovers live takes what models.dev states for it, else the
provider-level values. It never inherits a value declared on some other
model of the same provider. An explicit provider-level `image_input` wins
over models.dev for discovered models too.

## Pricing

`pricing` declares USD rates per million tokens. With it, e shows a cost
estimate per turn and reports `cost_usd` from `e rpc`.

| Field | Required | Meaning |
| --- | --- | --- |
| `input_per_million` | yes | Uncached input tokens. |
| `output_per_million` | yes | Output tokens. |
| `cache_read_per_million` | no | Cache reads. |
| `cache_write_5m_per_million` | no | Five-minute cache writes. |
| `cache_write_1h_per_million` | no | One-hour cache writes. |

An omitted cache rate falls back to the input rate, so cached tokens are
never priced at zero. Use the provider's current published rates.

## The live catalog

e asks each signed-in provider for its model list in the background: at
launch, after a sign-in, and when `/models` opens. A model a gateway ships
today appears today, without a new e build. Lists are cached in
`~/.e/models-store.json`.

Most lists carry only ids. In the same refresh e fetches
[models.dev](https://models.dev), a community catalog, keeps the entries for
its providers, and caches them in `~/.e/models-dev.json`. For every model
models.dev knows, built-in or discovered, it supplies the context window,
effort levels, whether an Anthropic model takes adaptive or budget thinking,
image and tool support, and pricing. It never adds ids and never sets
`max_output`.

Each value comes from the highest source that states it:

1. `models.json`
2. a context window the provider itself reports
3. models.dev
4. the built-in model, the offline fallback

An explicit `models.json` value survives every refresh and every e update.
