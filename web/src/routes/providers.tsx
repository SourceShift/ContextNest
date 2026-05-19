import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/cn";
import {
  Brain,
  Cloud,
  Cpu,
  Database,
  ExternalLink,
  Server,
  Sparkles,
} from "lucide-react";

interface ProviderEntry {
  name: string;
  kind: "llm" | "embedding";
  status: "built-in" | "compatible" | "byo";
  icon: React.ReactNode;
  description: string;
  setup: { env?: Record<string, string>; rust?: string };
  docs?: string;
}

const PROVIDERS: ProviderEntry[] = [
  // ── LLM providers ────────────────────────────────────────────────────
  {
    name: "Anthropic",
    kind: "llm",
    status: "built-in",
    icon: <Sparkles className="size-5" />,
    description:
      "Claude family. Default model: claude-3-5-haiku-20241022. Works with any Anthropic-protocol proxy (z.ai GLM) via base_url override.",
    setup: {
      env: {
        CONTEXTNEST_LLM_PROVIDER: "anthropic",
        ANTHROPIC_API_KEY: "sk-ant-...",
        CONTEXTNEST_LLM_MODEL: "(optional)",
      },
    },
    docs: "https://docs.anthropic.com",
  },
  {
    name: "OpenAI",
    kind: "llm",
    status: "built-in",
    icon: <Sparkles className="size-5" />,
    description:
      "OpenAI plus any OpenAI-compatible endpoint (Azure, LiteLLM, Ollama, vLLM, LM Studio, OpenRouter, Together, Groq, Mistral, Anyscale, Fireworks).",
    setup: {
      env: {
        CONTEXTNEST_LLM_PROVIDER: "openai",
        OPENAI_API_KEY: "sk-...",
        CONTEXTNEST_LLM_BASE_URL: "(optional override)",
        CONTEXTNEST_LLM_MODEL: "(optional, default gpt-4o-mini)",
      },
    },
    docs: "https://platform.openai.com/docs",
  },
  {
    name: "Google Gemini",
    kind: "llm",
    status: "built-in",
    icon: <Sparkles className="size-5" />,
    description: "Gemini family. Default model: gemini-2.0-flash.",
    setup: {
      env: {
        CONTEXTNEST_LLM_PROVIDER: "google",
        GOOGLE_API_KEY: "...",
      },
    },
    docs: "https://ai.google.dev",
  },
  {
    name: "Ollama (any model)",
    kind: "llm",
    status: "compatible",
    icon: <Server className="size-5" />,
    description:
      "Local Ollama via the OpenAI-compatible /v1 endpoint. No real auth — any non-empty key satisfies the bearer middleware. Works with llama3, mistral, qwen, deepseek, etc.",
    setup: {
      env: {
        CONTEXTNEST_LLM_PROVIDER: "openai",
        CONTEXTNEST_LLM_BASE_URL: "http://localhost:11434/v1",
        OPENAI_API_KEY: "ollama-placeholder",
        CONTEXTNEST_LLM_MODEL: "llama3",
      },
    },
    docs: "https://ollama.com",
  },
  {
    name: "OpenRouter",
    kind: "llm",
    status: "compatible",
    icon: <Cloud className="size-5" />,
    description:
      "Multi-provider aggregator. Point base_url at OpenRouter and use any of its 300+ models with one API key.",
    setup: {
      env: {
        CONTEXTNEST_LLM_PROVIDER: "openai",
        CONTEXTNEST_LLM_BASE_URL: "https://openrouter.ai/api/v1",
        OPENAI_API_KEY: "sk-or-v1-...",
        CONTEXTNEST_LLM_MODEL: "anthropic/claude-3.5-sonnet",
      },
    },
    docs: "https://openrouter.ai/docs",
  },
  {
    name: "Custom LLM (any)",
    kind: "llm",
    status: "byo",
    icon: <Cpu className="size-5" />,
    description:
      "Plug any LanguageModel impl via LlmServiceBuilder::with_custom_provider. Covers non-OpenAI-shaped APIs, internal services, gRPC frontends, test doubles.",
    setup: {
      rust: `use std::sync::Arc;\nuse contextnest::services::LlmServiceBuilder;\n\nlet service = LlmServiceBuilder::new()\n    .with_custom_provider("my-llm", Arc::new(my_model))\n    .build();`,
    },
    docs: "/docs/extensibility.md",
  },

  // ── Embedding providers ─────────────────────────────────────────────
  {
    name: "Ollama embeddings",
    kind: "embedding",
    status: "built-in",
    icon: <Server className="size-5" />,
    description:
      "Local Ollama with any pulled embedding model — nomic-embed-text (768-d), mxbai-embed-large (1024-d), all-minilm (384-d), snowflake-arctic-embed (1024-d).",
    setup: {
      rust: `use contextnest::services::OllamaEmbeddingProvider;\n\nlet p = OllamaEmbeddingProvider::new(\n    "",                            // empty → http://localhost:11434\n    "nomic-embed-text",            // any model Ollama has pulled\n    768,                           // model's output dimension\n);`,
    },
    docs: "/docs/extensibility.md",
  },
  {
    name: "Hugging Face Inference",
    kind: "embedding",
    status: "built-in",
    icon: <Brain className="size-5" />,
    description:
      "Any HF model with feature-extraction — sentence-transformers/*, BAAI/bge-*, intfloat/e5-*, Alibaba-NLP/gte-*. Public Inference API or dedicated Inference Endpoints.",
    setup: {
      rust: `use contextnest::services::HuggingFaceEmbeddingProvider;\n\nlet p = HuggingFaceEmbeddingProvider::new(\n    "BAAI/bge-large-en-v1.5",\n    "hf_your_token",\n    1024,\n    "",                            // empty → public Inference API\n);`,
    },
    docs: "/docs/extensibility.md",
  },
  {
    name: "Custom HTTP (Voyage / Cohere / Jina / Mistral / TEI)",
    kind: "embedding",
    status: "built-in",
    icon: <Database className="size-5" />,
    description:
      "Generic JSON-over-HTTP. Covers Voyage AI, Cohere v1/v3, Jina, Mistral, text-embeddings-inference, vLLM-with-embeddings, and any internal microservice — no Rust code needed, just a config struct.",
    setup: {
      rust: `use std::collections::HashMap;\nuse contextnest::services::{\n    CustomHttpEmbeddingConfig, CustomHttpEmbeddingProvider,\n};\n\nlet p = CustomHttpEmbeddingProvider::new(CustomHttpEmbeddingConfig {\n    name: "voyage".into(),\n    endpoint: "https://api.voyageai.com/v1/embeddings".into(),\n    api_key: Some("vy-...".into()),\n    input_field: "input".into(),\n    model: Some("voyage-3-large".into()),\n    extra_fields: HashMap::new(),\n    embedding_path: "data.0.embedding".into(),\n    dimension: 1024,\n    extra_headers: HashMap::new(),\n});`,
    },
    docs: "/docs/extensibility.md",
  },
  {
    name: "Custom embedding (any)",
    kind: "embedding",
    status: "byo",
    icon: <Cpu className="size-5" />,
    description:
      "Implement the EmbeddingProvider trait for anything else — gRPC, stateful auth, internal protocols. ~10 lines of Rust.",
    setup: {
      rust: `use async_trait::async_trait;\nuse contextnest::services::EmbeddingProvider;\n\n#[async_trait]\nimpl EmbeddingProvider for MyEmbedder {\n    fn name(&self) -> &str { "my-embedder" }\n    fn dimension(&self) -> usize { 1024 }\n    async fn embed(&self, text: &str) -> /*…*/ { todo!() }\n}`,
    },
    docs: "/docs/extensibility.md",
  },
];

export function ProvidersRoute() {
  const llms = PROVIDERS.filter((p) => p.kind === "llm");
  const embeddings = PROVIDERS.filter((p) => p.kind === "embedding");

  return (
    <div className="mx-auto max-w-5xl">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold text-[var(--color-text)]">
          Providers
        </h1>
        <p className="mt-2 text-sm text-[var(--color-text-muted)]">
          ContextNest is provider-agnostic. Plug any LLM and any embedding
          service — three are built-in for each, plus an "any" escape hatch
          via the trait surface.
        </p>
      </header>

      <Section title="LLM providers" entries={llms} />
      <Section title="Embedding providers" entries={embeddings} />
    </div>
  );
}

function Section({
  title,
  entries,
}: {
  title: string;
  entries: ProviderEntry[];
}) {
  return (
    <section className="mb-10">
      <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-[var(--color-text-muted)]">
        {title}
      </h2>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {entries.map((e) => (
          <ProviderCard key={e.name} entry={e} />
        ))}
      </div>
    </section>
  );
}

function ProviderCard({ entry }: { entry: ProviderEntry }) {
  return (
    <Card>
      <CardHeader>
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-2 text-[var(--color-accent)]">
            {entry.icon}
            <CardTitle>{entry.name}</CardTitle>
          </div>
          <StatusBadge status={entry.status} />
        </div>
        <CardDescription>{entry.description}</CardDescription>
      </CardHeader>
      <CardContent>
        {entry.setup.env ? <EnvBlock env={entry.setup.env} /> : null}
        {entry.setup.rust ? <RustBlock code={entry.setup.rust} /> : null}
        {entry.docs ? (
          <div className="mt-3">
            <a
              href={entry.docs}
              target={entry.docs.startsWith("http") ? "_blank" : undefined}
              rel="noopener noreferrer"
              className={cn(
                "inline-flex h-8 items-center gap-1 rounded-md px-3 text-sm",
                "text-[var(--color-text)] hover:bg-[var(--color-bg-subtle)]",
                "transition-colors duration-150",
              )}
            >
              Docs
              <ExternalLink className="size-3" />
            </a>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

function StatusBadge({ status }: { status: ProviderEntry["status"] }) {
  const label =
    status === "built-in"
      ? "Built-in"
      : status === "compatible"
        ? "OpenAI-compatible"
        : "Bring your own";
  const color =
    status === "built-in"
      ? "var(--color-success)"
      : status === "compatible"
        ? "var(--color-accent)"
        : "var(--color-warning)";
  return (
    <span
      className="inline-flex items-center rounded-full border px-2 py-0.5 text-[10px] font-medium uppercase tracking-wider"
      style={{ borderColor: color, color }}
    >
      {label}
    </span>
  );
}

function EnvBlock({ env }: { env: Record<string, string> }) {
  const text = Object.entries(env)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
  return <CodeBlock label="env" code={text} />;
}

function RustBlock({ code }: { code: string }) {
  return <CodeBlock label="rust" code={code} />;
}

function CodeBlock({ label, code }: { label: string; code: string }) {
  return (
    <div className="mt-2 overflow-hidden rounded-md border border-[var(--color-border)]">
      <div className="flex items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-bg-subtle)] px-3 py-1 text-[10px] uppercase tracking-wider text-[var(--color-text-subtle)]">
        <span>{label}</span>
        <button
          type="button"
          onClick={() => navigator.clipboard?.writeText(code)}
          className="hover:text-[var(--color-text)] transition-colors"
        >
          Copy
        </button>
      </div>
      <pre className="overflow-x-auto bg-[var(--color-bg)] p-3 text-xs leading-relaxed text-[var(--color-text)]">
        <code>{code}</code>
      </pre>
    </div>
  );
}
