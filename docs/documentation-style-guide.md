# AWiki Open Source Documentation Style and Information Architecture

[English](documentation-style-guide.md) | [简体中文](documentation-style-guide.zh-CN.md)

## 1. Purpose of a README

A README is a project's home page, not its complete design document, API reference, or release runbook.

It must answer these questions in order:

1. **What**: What is this?
2. **Who**: Who should use it?
3. **Why**: What problem does it solve, and what makes it different?
4. **Status**: What is its maturity level, and what are its key boundaries?
5. **Try**: How can a user achieve a first success within five minutes?
6. **Next**: Where are the detailed documentation, contribution guide, and support channels?

## 2. Standard README structure

```text
Project name and one-sentence value proposition
Status / license / core technology badges
Screenshot or short demo
Target users and core value
Quick start / first success
Main capabilities
Limitations and compatibility
Ecosystem position / architecture overview
Security summary
Documentation links
Contributing / support / license
```

Move implementation details, complete configuration tables, test matrices, signature troubleshooting, full command trees, and API method tables into `docs/`.

## 3. Writing requirements

### 3.1 Lead with user outcomes

Recommended:

> Run an AWiki-compatible community on your own domain, with identity, messaging, attachments, and ANP interoperability.

Not recommended:

> A single-process service based on FastAPI, SQLite, JCS, Ed25519, and JSON-RPC.

Describe the technology stack in the next paragraph or in the development documentation.

### 3.2 Give each sentence one main purpose

Do not fill the opening paragraph with a long list of modules, protocol names, and internal boundaries. Prioritize five to seven user-visible capabilities in feature lists.

### 3.3 Use verifiable maturity labels

| Status | Meaning |
| --- | --- |
| `MVP` | Validates the core end-to-end flow and has clear capability and operational boundaries; it is not production-ready. |
| `Developer Preview` | Intended for developers and early testers; interfaces or user experience may change. |
| `Beta` | Main workflows are usable, but compatibility and reliability still require validation. |
| `Stable` | Has an explicit version policy, upgrade contract, support scope, and continuous verification. |
| `Experimental` | Must not be presented as the default production path. |
| `Unsupported` | Explicitly unavailable; do not soften this to "partially supported." |

A version number alone does not indicate maturity.

## 4. Command examples

- Every command must use a real entry point on the current branch.
- Public READMEs must not contain unresolved template variables.
- The first example should be the shortest practical copy-and-run path.
- Show `--dry-run` first for CLI operations with side effects.
- Explain the success criteria after a command instead of showing only the command.
- Use clearly safe placeholders such as `<recipient-handle>` and `example.com`.
- Never show real phone numbers, email addresses, DIDs, tokens, private keys, Team IDs, server paths, or test accounts.

## 5. Links and filenames

- Use `README.md` for the default English README.
- Use `README.zh-CN.md` for the Simplified Chinese README.
- Use English ASCII filenames and directory names.
- Use this language switch consistently: `[English](README.md) | [简体中文](README.zh-CN.md)`.
- Prefer relative links within a repository.
- Use stable organization-level canonical URLs across repositories.
- Do not link temporary branches, personal forks, or expiring CI artifacts.

## 6. Screenshots and demos

- GUI projects must show a product screenshot or a 20-40 second GIF near the top.
- CLI projects should include a real terminal demo.
- Server projects may use terminal demos, architecture diagrams, and interoperability sequence diagrams.
- Images must have descriptive alt text.
- Images must not expose real identities, tokens, phone numbers, email addresses, absolute paths, or internal domains.
- Ensure sufficient contrast and an explicit background for dark and light images.
- Store assets under `docs/assets/readme/`.

## 7. Describing compatibility

Do not write only "supports AWiki clients." State at least:

- the client or server version;
- verified capabilities;
- unverified capabilities;
- E2EE status;
- platform status;
- verification date; and
- whether an allowlist, specific domain, or test environment is required.

## 8. Security documentation

The README opening should show only security boundaries that affect adoption, including:

- whether E2EE is supported;
- whether public deployment is supported;
- whether the system is single-node;
- whether production identity providers are included; and
- which development switches must never be enabled publicly.

Document detailed key lifecycles, SecretVault behavior, signatures, Keychain access, permissions, and vulnerability reporting in `SECURITY.md` and focused design documents.

## 9. Maintenance workflow

Before every release:

1. Update the status and version compatibility matrix.
2. Run every README command in a clean environment.
3. Check every relative link.
4. Confirm that screenshots match the current UI.
5. Confirm that the English and Chinese READMEs state the same facts.
6. Search for template variables, TODOs, and obsolete domains.
7. Ask someone unfamiliar with the project to perform the 10-second, 60-second, and five-minute tests.
