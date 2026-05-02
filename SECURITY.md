# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

ContextNest is a pre-1.0 substrate. Security guarantees are limited: the API surface,
wire format, and internal data structures are still stabilising. Do not deploy
v0.1.x in a production environment that processes sensitive personal data without
independently auditing the code first.

## Scope

The security boundary covers:

- The `PathValidator` input-canonicalisation and symlink-check logic in `src/security/`.
- HTTP request deserialisation and input validation in `src/api/tools.rs`.
- Credential handling in `LlmService` (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
  `GOOGLE_API_KEY`) — these are read from environment variables and never
  written to logs or serialised responses.

Out of scope for v0.1.x: multi-tenant isolation, encrypted-at-rest attractor state,
and network-level access control (deploy behind a reverse proxy for those).

## Reporting a vulnerability

Please use **GitHub Security Advisories** to report vulnerabilities privately:

1. Go to <https://github.com/SourceShift/ContextNest/security/advisories>
2. Click **New draft security advisory**
3. Describe the vulnerability, affected versions, and steps to reproduce

If you prefer email, contact the maintainer at the address listed in `Cargo.toml`
(`authors` field). Expect an initial response within 5 business days.

Please do not open a public GitHub issue for security-sensitive reports.

## Disclosure policy

We follow responsible disclosure. Once a fix is ready and released we will publish
a GitHub Security Advisory with full details. Credit is given to the reporter unless
they request otherwise.
