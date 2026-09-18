---
title: Models & providers
description: Add models and correct built-in ones in models.json.
order: 1
---

# Models & providers

`~/.e/models.json` adds models and corrects the built-in ones.

```json
{
  "providers": {
    "local": {
      "base_url": "http://localhost:8080/v1",
      "api": "openai-completions",
      "responses_mount": "platform",
      "context_window": 64000,
      "supports_tools": true,
      "models": [
        "small-model",
        {
          "id": "big-model",
          "context_window": 1000000,
          "image_input": false,
          "pricing": {
            "input_per_million": 1.0,
            "output_per_million": 4.0,
            "cache_read_per_million": 0.1,
            "cache_write_5m_per_million": 1.25,
            "cache_write_1h_per_million": 2.0
          }
        }
      ]
    }
  }
}
```

An entry with a built-in model's provider and id replaces that model. The
file wins, as a theme file does.

Only models whose provider has credentials appear in `/models`. To cycle
through a shortlist, pick it with `/scoped-models`, then press ctrl+p to
cycle.

## Endpoint and dialect

These keys tell e where to send requests and how to shape them.

- `base_url` is the provider's endpoint. A new provider requires it. An entry
  for a built-in provider may omit it and inherit that provider's endpoint.
  e never guesses another provider's host.
- `api` is the wire dialect: `openai-completions`, `openai-responses`,
  `codex-responses`, `anthropic-messages`, or `google-generative-ai`. The
  default is `openai-completions`. e also accepts the short aliases
  `completions`, `responses`, `anthropic`, and `google`. Any other name is a
  load error.
- `responses_mount` selects the Responses path explicitly. It affects only a
  Responses dialect. e never infers it from whether the stored credential is
  a key or OAuth.

| `responses_mount` | Request path | Notes |
| --- | --- | --- |
| `platform` | `{base_url}/responses` | Default. |
| `codex` | `{base_url}/codex/responses` | Adds the ChatGPT account headers. |

## Model discovery

`catalog` controls only live model discovery. It is independent of `api`.
The separation matters for gateways that accept one inference dialect but
expose another provider's catalog shape.

| `catalog` | How e reads the model list |
| --- | --- |
| `openai` | Default. `GET /models`, reading `data[].id`. |
| `anthropic` | `GET /v1/models`, with `x-api-key`. |
| `google` | `models[].name`, with `x-goog-api-key`. |
| `chatgpt` | The ChatGPT backend's picker. e reads `models[].slug` and strips the `-wm` suffix. It keeps work-mode entries only and uses `max_tokens` as the context window. |
| `none` | No live discovery. |

## Limits

`context_window` sets the model's context size. It drives the statusline
percentage and auto-compaction, so set it truthfully. Put it on the provider
as a default for its models, or on a model object. The default is `200000`.

`max_output` caps the reply-token ceiling. Use it for models whose real limit
is below the dialect's own default, for example a small Anthropic model. Put
it on the provider or on a model object. Only the Anthropic dialect reads it
today. The default is the dialect's own constant.

## Reasoning effort

`effort` on a model object declares the model's reasoning levels, in cycle
order. Shift+tab walks exactly this list, for example `["low", "medium",
"high", "xhigh"]`.

A model entry without `effort` takes its levels from the first source that
has them:

1. its provider default
2. what models.dev states for the id
3. its built-in declaration

If none of these has levels, the model has no reasoning knob.

e sends each level as the exact string in `reasoning_effort`, or the
dialect's equivalent. The levels must match what the backend accepts. For
example, opencode-go's `glm-5.3-flash` takes `["low", "high", "max"]`, with no
`medium`. The gateway's own list does not advertise that set.

## Capabilities

`supports_tools` and `image_input` are capabilities. Set them at provider or
model level.

| Key | Default |
| --- | --- |
| `supports_tools` | `true` |
| `image_input` | `false` |

e sends no tool schemas to a model declared without tool support. That model
cannot execute a tool even if it emits one.

A live-discovered id takes what models.dev states for it, else the
provider-level defaults. It never takes an override declared on some other
model of the same provider. An explicit provider `image_input` setting wins
over feed facts for discovered ids too.

## Pricing

`pricing` declares USD rates per million uncached input and output tokens.
e shows a turn estimate and includes `cost_usd` in the `e rpc` response.

The cache-read, five-minute cache-write, and one-hour cache-write rates are
optional. They price prompt caching separately. An omitted cache rate falls
back to the ordinary input rate rather than dropping those tokens.

Pricing itself is optional, because it changes independently of the wire
protocol. Use the provider's current published rates.

## Credentials

`/login <provider>` stores an API key for any provider name, in
`~/.e/auth.json`.

A provider with no stored credential falls back to its conventional
environment variable. This is what CI and scripts want. `auth.json` wins when
both exist. The variables are:

- `ANTHROPIC_API_KEY`
- `OPENAI_API_KEY`
- `GEMINI_API_KEY`
- `XAI_API_KEY`
- `GROQ_API_KEY`
- `MISTRAL_API_KEY`
- `DEEPSEEK_API_KEY`
- `CEREBRAS_API_KEY`
- `OPENROUTER_API_KEY`
- `TOGETHER_API_KEY`
- `FIREWORKS_API_KEY`
- `OPENCODE_API_KEY`
- `OPENCODE_GO_API_KEY`
- `AI_GATEWAY_API_KEY`

Local backends need no credential at all. This covers Ollama on
`localhost:11434` and LM Studio on `localhost:1234`. They are always signed
in, and their models appear as soon as the local server answers `/models`.

## The catalog is live

e asks each signed-in provider for its model list with `GET {base}/models`.
It does this in the background at launch, after a sign-in, and when `/models`
opens. A model a gateway ships today appears today, with no e release
involved.

Most of those lists carry nothing but ids. The facts come from
[models.dev](https://models.dev), a community catalog. e fetches it in the same refresh, trims
it to e's providers, and caches it in `~/.e/models-dev.json`.

For every model models.dev knows, built-in seed or freshly discovered id, it
sets:

- the context window
- the effort levels
- whether reasoning is adaptive or a token budget, the Anthropic thinking
  shape
- image and tool support
- pricing

models.dev never adds ids. Which models a provider serves is the provider's
word. It does not set `max_output`.

### Precedence

From lowest to highest:

1. the built-in seed
2. the models.dev facts
3. a window the gateway itself reports
4. `models.json`

A seed is only the offline fallback. A wrong fact is fixed upstream, not
pinned in e.

An explicit `models.json` value is final. It survives every refresh and every
e update. A partial entry inherits the facts for what it leaves unsaid, even
when the model has no built-in seed.
