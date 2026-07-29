# AWiki Notify Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a lazily loaded Notify reference that guides a Coding Agent to send one authorized terminal-state message to an AWiki Me user through `awiki-cli msg send`.

**Architecture:** Preserve the repository's single-entry, two-layer Skill architecture. Add `skills/references/12-notify.md`, route to it from `skills/SKILL.md`, expose it through the CLI `skills` documentation topic, and keep product behavior in the existing plain direct-message command rather than adding a new CLI or Daemon path.

**Tech Stack:** Markdown Skill contracts, Rust integration tests, `awiki-cli` CLI documentation catalog.

## Global Constraints

- Notify supports exactly `completed`, `blocked`, `action_required`, and `failed`; ordinary progress does not trigger a message.
- The receiver must be an exact user-provided AWiki Me Handle or DID authorized for the current task.
- Sending uses plain `awiki-cli msg send`, starts with `--dry-run`, and does not use Daemon, E2EE, or `runtime host-notify`.
- Sending pins the current identity and passes untrusted message text as a direct argv element, never through shell interpolation.
- Target validation uses `id resolve`; `msg send --dry-run` is documented and tested as syntactic planning only.
- The CLI JSON envelope is the machine success oracle; service acceptance is not proof that AWiki Me displayed the message or banner.
- Do not modify the network implementation, Daemon protocol, AWiki Me client, or unrelated dirty worktrees.
- Keep changes uncommitted until the user requests a commit or PR.

---

### Task 1: Add the Notify Skill contract

**Files:**
- Create: `crates/awiki-cli/tests/skill_notify_contract.rs`
- Create: `skills/references/12-notify.md`
- Modify: `skills/SKILL.md`
- Modify: `crates/awiki-cli/src/cli_docs/mod.rs`
- Modify: `docs/architecture/awiki-skill-architecture.md`
- Modify: `docs/agent-integration.md`
- Modify: `docs/agent-integration.zh-CN.md`

**Interfaces:**
- Consumes: existing `awiki-cli msg send --to <target> --text <message>` command and the `cli_docs::lookup("skills")` topic.
- Produces: `references/12-notify.md` as the canonical Notify workflow, one `Notify` route in the entry Skill, and one discoverable CLI docs reference.

- [x] **Step 1: Write the failing contract test**

```rust
use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn notify_skill_is_routed_and_discoverable() {
    let root = repository_root();
    let entry = fs::read_to_string(root.join("skills/SKILL.md")).expect("entry Skill");
    let notify =
        fs::read_to_string(root.join("skills/references/12-notify.md")).expect("Notify reference");
    let topic = awiki_cli::cli_docs::lookup("skills").expect("skills docs topic");

    assert!(entry.contains("| Notify |"));
    assert!(entry.contains("`references/12-notify.md`"));
    assert!(topic.references.contains(&"skills/references/12-notify.md"));

    for status in ["completed", "blocked", "action_required", "failed"] {
        assert!(notify.contains(&format!("`{status}`")));
    }
    assert!(notify.contains("awiki-cli msg send"));
    assert!(notify.contains("--dry-run"));
    assert!(notify.contains("--format json"));
}

#[test]
fn notify_skill_keeps_authorization_and_product_boundaries() {
    let notify = fs::read_to_string(
        repository_root().join("skills/references/12-notify.md"),
    )
    .expect("Notify reference");

    assert!(notify.contains("current task"));
    assert!(notify.contains("Do not guess"));
    assert!(notify.contains("ordinary progress"));
    assert!(notify.contains("does not prove AWiki Me displayed"));
    assert!(notify.contains("Do not use `runtime host-notify`"));
    assert!(notify.contains("Do not use E2EE"));
    assert!(notify.contains("Do not require the Daemon"));
    assert!(notify.contains("Pass arguments directly as an argv array"));
    assert!(notify.contains("awiki-cli id current"));
    assert!(notify.contains("awiki-cli id resolve"));
    assert!(notify.contains("Do not switch identities"));
    assert!(notify.contains("Dry-run is syntactic planning only"));
    assert!(notify.contains("data.plan.identity"));
    assert!(notify.contains("data.plan.target.did"));
    assert!(notify.contains("plain text may expose"));
    assert!(notify.contains("No crash-safe idempotency"));
}
```

- [x] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test -p awiki-cli --test skill_notify_contract
```

Expected: FAIL because `skills/references/12-notify.md` does not exist and the entry/docs topic has no Notify route.

- [x] **Step 3: Add the minimal Notify reference**

Create `skills/references/12-notify.md` with:

- its purpose and best-effort limitation;
- the four terminal-state definitions;
- current-task authorization and exact-target rules;
- the fixed three-line message format;
- sender-identity inspection and direct argv invocation;
- target resolution through `id resolve` and the non-resolving dry-run boundary;
- dry-run then send commands with JSON output;
- JSON success checks and the distinction between service acceptance and App display;
- one-send-per-terminal-state behavior and ambiguous-result no-blind-retry behavior;
- explicit exclusions for ordinary progress, Daemon, E2EE, and `runtime host-notify`.

- [x] **Step 4: Route and expose the reference**

Update `skills/SKILL.md` to:

- add a `Notify` row using keywords `notify`, `completed`, `blocked`, `action-required`, `failed`, and `terminal`;
- add the narrow current-task notification authorization exception after the onboarding exception;
- add `notify: partially implemented` to capability status because Skill invocation is best-effort, not lifecycle guaranteed.

Update `crates/awiki-cli/src/cli_docs/mod.rs` so the `skills` topic references `skills/references/12-notify.md`.

Update `docs/architecture/awiki-skill-architecture.md` so the file tree, reference count, routing description, migration map, and final architecture summary include `12-notify.md`.

Update both Agent integration guides so their minimal-loading tables route Coding Agent terminal notifications to `12-notify.md`.

- [x] **Step 5: Run the focused test and verify GREEN**

Run:

```bash
cargo test -p awiki-cli --test skill_notify_contract
```

Expected: all Notify contract tests PASS.

- [x] **Step 6: Run regression and formatting checks**

Run:

```bash
cargo test -p awiki-cli --lib
rustfmt --edition 2021 --check crates/awiki-cli/tests/skill_notify_contract.rs
git diff --check
```

Expected: all commands exit with code 0. The repository-wide `cargo fmt --all -- --check`
currently reports unrelated baseline formatting drift, so this task checks the new Rust test file
directly and does not rewrite unrelated files.

- [x] **Step 7: Review the final diff**

Run:

```bash
git status --short
git diff -- skills/SKILL.md skills/references/12-notify.md crates/awiki-cli/src/cli_docs/mod.rs crates/awiki-cli/tests/skill_notify_contract.rs docs/architecture/awiki-skill-architecture.md docs/agent-integration.md docs/agent-integration.zh-CN.md docs/superpowers
```

Expected: only the approved Notify Skill, its tests, architecture documentation, design, and plan are changed.
