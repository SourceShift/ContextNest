# Epic — Privacy filter + `.cn-ignore` opt-out

**Depends on:** MVP foundation PR.

**Estimate:** ~1 day.

## What

Two mechanisms for keeping sensitive data out of ContextNest:

### 1. Per-project opt-out via `.cn-ignore`

A file at `~/.claude/projects/<encoded-dir>/.cn-ignore` causes the
ingester to skip the entire project. No memories from any session in
that project are stored.

### 2. Pattern-based redactor

A configurable list of regex patterns that the ingester applies to
every memory's text before pushing it to the substrate. Matches are
replaced with `[REDACTED:<label>]`.

Built-in patterns (always on):
- Credit card numbers (Luhn-valid 13–19 digit sequences)
- SSNs
- AWS / GCP / GitHub / Anthropic / OpenAI tokens (well-known prefixes)
- Email addresses (`--redact-emails` flag opt-in only, off by default)

User-supplied patterns via `~/.contextnest/redact.toml` — use the
inline-table array form so docs CI's grep-based link checker doesn't
false-positive on TOML's array-of-tables syntax:

```toml
patterns = [
    { name = "internal_customer_id", regex = "CUST-[A-Z0-9]{8}", label = "customer_id" },
    { name = "api_key_format",       regex = "sk-[a-zA-Z0-9]{32,}",  label = "api_key" },
]
```

(The implementation accepts both inline-table and array-of-tables
syntax — TOML treats them identically.)

## Why

Transcripts contain whatever the user typed. Sometimes that's API
keys, customer data, internal IDs. Without a filter, the substrate
indexes all of it and any later `retrieve` exposes it. The opt-out
file is the nuclear option; the redactor is the surgical default.

## Files touched

| File | Change |
|---|---|
| `src/ingest/claude_code/redactor.rs` | New: load patterns, apply to text |
| `src/ingest/claude_code/mod.rs` | Check `.cn-ignore` before discovering sessions; pipe each extracted memory's text through the redactor before storing |
| `src/ingest/claude_code/sink.rs` | Skip stores where text became 100% redacted (no signal left) |
| `docs/ingest/privacy.md` | New: how to configure, sample `redact.toml`, threat model |
| `tests/redactor_test.rs` | Built-in pattern coverage + custom-pattern loading + 100%-redacted skip |

## Implementation sketch

```rust
pub struct Redactor {
    patterns: Vec<(Regex, String)>,  // (regex, label)
}

impl Redactor {
    pub fn from_config(path: &Path) -> Result<Self> { /* load TOML */ }
    pub fn with_defaults() -> Self { /* CC, SSN, well-known token prefixes */ }
    pub fn redact(&self, text: &str) -> RedactionResult {
        // returns { text, num_redactions, original_len, redacted_len }
    }
}
```

Apply via:
```rust
let result = redactor.redact(&memory.text);
if result.redacted_len < result.original_len / 4 {
    tracing::warn!("memory was >75% redacted, skipping store");
    return Ok(());
}
memory.text = result.text;
memory.metadata.insert("redaction_count", result.num_redactions.into());
sink.store(&memory).await?;
```

## Success criteria

- `.cn-ignore` causes the ingester to skip the entire project (logged
  with a clear message: "skipping project X — .cn-ignore present").
- Default patterns catch the common cases without false positives on
  normal English text (zero hits on the README files of major OSS
  projects when run as a regression test).
- User-supplied patterns load from `~/.contextnest/redact.toml`
  without restart of the substrate (re-read on every ingest run).
- Documented threat model: what the redactor catches, what it
  doesn't, and what users should still review by hand.

## What's NOT in scope

- LLM-based PII detection (e.g. running each memory through Claude to
  identify PII before storing). Too slow + expensive for an automatic
  default. Add as opt-in `--llm-redactor` flag in a v0.3+ epic if
  anyone asks.
- Encryption-at-rest of the substrate itself. The substrate is local;
  if your filesystem is compromised so is everything else. Encryption
  belongs at the disk / volume layer.
- Per-session redaction overrides. v0.3+ if anyone asks.
