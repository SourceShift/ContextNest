# Privacy in Claude Code ingest

Transcripts contain whatever the user typed. Sometimes that's an API
key, a customer ID, a tax-ID. Without a filter, the substrate
indexes all of it and any later `retrieve` exposes it. ContextNest
ships two mechanisms for keeping sensitive data out.

## 1. Per-project opt-out (`.cn-ignore`)

Drop a file named `.cn-ignore` into a project's Claude Code session
directory:

```
~/.claude/projects/-Users-me-secret-project/.cn-ignore
```

The ingester logs a `warn!` line ("skipping project — .cn-ignore
present") and stores **zero** memories from any session under that
project. Both the CLI batch ingest and the real-time hook receiver
respect this opt-out.

This is the **nuclear option** — useful when a project handles
genuinely sensitive material (regulated data, customer secrets) and
you don't want any heuristic redactor to be the last line of
defense.

## 2. Pattern-based redactor (default-on)

Every memory's `text` field is run through a regex-based redactor
before storage. Each match is replaced with `[REDACTED:<label>]`.

### Built-in patterns

| Name | Detects | Confidence |
|---|---|---|
| `aws_access_key` | `AKIA[A-Z0-9]{16}` | high |
| `github_pat_classic` | `ghp_<36 chars>` | high |
| `github_pat_fine_grained` | `github_pat_<82 chars>` | high |
| `anthropic_api_key` | `sk-ant-<32+ chars>` | high |
| `openai_api_key` | `sk-` (incl. `sk-proj-`) followed by 32+ chars | high |
| `gcp_service_account` | `*@*.iam.gserviceaccount.com` | high |
| `ssn` | `\d{3}-\d{2}-\d{4}` | medium |
| `credit_card_candidate` | 13-19 digit run, **Luhn-checked** | medium |

The Luhn check on `credit_card_candidate` keeps generic numeric runs
(UUIDs, build IDs, IMEIs, version codes) from false-positive
triggering. Without it, the pattern would catch every git short-SHA
that happened to be all-digits.

### User-supplied patterns

Drop a `redact.toml` at `~/.contextnest/redact.toml`:

```toml
patterns = [
    { name = "internal_customer_id", regex = "CUST-[A-Z0-9]{8}", label = "customer_id" },
    { name = "api_key_format",       regex = "sk-[a-zA-Z0-9]{32,}",  label = "api_key" },
]
```

User patterns are **appended** to the built-in defaults — the
built-ins always run. An invalid regex in the config is logged at
`warn!` and skipped; absence of the file falls back to defaults only.

### What happens to memories that are mostly secrets

If more than 75% of a memory's bytes were matched by the redactor,
the record is **dropped, not stored** — a memory consisting almost
entirely of `[REDACTED:…]` labels has no semantic signal left, so
storing it just pollutes retrieval.

The CLI summary reports the drop count:

```
INGEST COMPLETE
  Memories: 138 success / 0 fail
  Privacy filter: 3 records dropped (>75% redacted)
```

Live cc-hook ingest emits the same `warn!` log line per drop.

## Threat model

**The redactor catches:**

- Common cloud / SaaS API key formats with strong prefix patterns
- US SSNs in the standard `xxx-xx-xxxx` format
- Luhn-valid credit-card numbers as part of natural text

**The redactor does NOT catch:**

- Tokens with unfamiliar prefixes (custom enterprise SSO, legacy
  systems) — add a user-pattern rule
- PII that looks like normal text (names, addresses, free-form notes
  about people)
- Secrets that already passed through paraphrase / reformat by
  Claude before being committed to transcript
- Encrypted-but-unencrypted-stored material (an SSH private key
  pasted as base64 doesn't trigger anything unless you add a rule)

**The redactor is the surgical default. The `.cn-ignore` opt-out is
the nuclear option.** For genuinely regulated material, use both —
the redactor catches accidental paste-ins inside non-sensitive
projects, the opt-out keeps an entire sensitive project off the
substrate.

## Configuration recovery

If `~/.contextnest/redact.toml` is malformed (bad TOML, invalid
regex), the ingester logs a `warn!` and **continues with the
built-in defaults only**. The absence of valid user config never
silently disables the built-in protections.
