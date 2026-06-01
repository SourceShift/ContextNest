# SDK fixture-parity recordings — v0.3 LLM proxy slice 1.4

Real request and response bodies as the three target SDKs emit and parse them.
Recorded from public documentation + by inspecting SDK source.

## Why JSON files (vs inline literals or live-spawned SDKs)

| Option | Pros | Cons |
|---|---|---|
| (a) Inline string literals in test file | Self-contained, single file | Fixtures get lost in `assert!()` noise; hard to diff against real SDK behaviour |
| **(b) JSON files (this directory)** | Reviewable in isolation; `cat` shows exactly what SDKs emit; drift caught by re-running the SDK + diffing | Adds files |
| (c) Spawn actual SDKs at test time | Most authoritative | Adds Python+Node toolchains to CI; flaky; defeats hermetic-test invariant |

Picked (b). The CI cost of (c) makes (b) the only sensible default; (a) is
strictly worse than (b) on every readability axis.

## What's recorded

### Chat completions — requests

| File | SDK | Notes |
|---|---|---|
| `openai_python_chat_simple_request.json` | `openai-python` 1.x | `client.chat.completions.create()` with string content |
| `openai_python_chat_multimodal_request.json` | `openai-python` 1.x | Array `content` with text + image_url part |
| `openai_python_chat_tool_use_request.json` | `openai-python` 1.x | `tools` + `tool_choice` |
| `anthropic_sdk_compat_chat_request.json` | `@anthropic-ai/sdk` via OpenAI-compat baseURL | What it emits when pointed at an OpenAI-shaped endpoint |
| `google_genai_openai_shim_request.json` | `@google/generative-ai` via `client = genai.Client(http_options=...)` OpenAI shim | The unified shape Google's library emits to OpenAI-compatible base URLs |

### Chat completions — responses

| File | SDK | Notes |
|---|---|---|
| `openai_python_chat_response.json` | `openai-python` 1.x | What the SDK parses cleanly into `ChatCompletion` |
| `openai_python_chat_tool_use_response.json` | `openai-python` 1.x | `tool_calls` present, `content: null` |

### Embeddings

| File | SDK | Notes |
|---|---|---|
| `openai_python_embeddings_string_request.json` | `openai-python` 1.x | `input: "..."` string form |
| `openai_python_embeddings_array_request.json` | `openai-python` 1.x | `input: [...]` array form |
| `openai_python_embeddings_response.json` | `openai-python` 1.x | What the SDK parses cleanly into `CreateEmbeddingResponse` |

## Re-recording protocol

When an SDK ships a new wire-format version that breaks one of these
fixtures:

1. Identify the request shape in the SDK's source (e.g. `openai/_streaming.py`
   for streaming, `openai/resources/chat/completions.py` for chat).
2. Copy the literal body into the matching `.json` file.
3. Run `cargo test --test llm_proxy_sdk_parity` — failures point at the
   shape that needs updating in `src/api/llm_proxy/openai_shapes.rs`.
4. Update the shape, re-run, commit fixture + shape change together so
   the regression signal is preserved in `git log`.

The fixtures are NOT auto-regenerated. They are checkpoint snapshots —
the diff cost of an SDK-version bump IS the cost of the bump, surfaced in
the PR that updates them.
