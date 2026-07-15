# README and Documentation Release Readiness Checklist

[English](release-readiness-checklist.md) | [简体中文](release-readiness-checklist.zh-CN.md)

## P0: required before release

- [ ] The opening paragraph explains user value without requiring knowledge of ANP, DID, or internal modules.
- [ ] The project status is `MVP`, `Developer Preview`, `Beta`, or `Stable`, and the owner has confirmed it.
- [ ] Every download, installation, and upgrade URL has been verified without authentication.
- [ ] No `{{...}}`, `TODO(...)`, internal domains, temporary branches, or personal paths remain.
- [ ] Every command in a code block has run in a clean environment.
- [ ] The first-success path does not depend on an undocumented sibling checkout, account pool, or secret configuration.
- [ ] Platform support matches the actual SDK, packaging, and CI state.
- [ ] E2EE descriptions distinguish Direct, Group, client, server, and dependency conditions.
- [ ] A self-hosted server states its no-E2EE, single-node, and production identity-provider boundaries near the top.
- [ ] `SECURITY.md` provides a working private vulnerability reporting channel.
- [ ] Screenshots have replaced placeholders and contain no real sensitive data.
- [ ] The English and Chinese READMEs state the same facts.
- [ ] The final version is on the default branch.

## Link checks

- [ ] Language-switch links are correct.
- [ ] Every relative path in the repository exists.
- [ ] Cross-repository links use the organization's canonical repositories.
- [ ] Document anchors work after GitHub rendering.
- [ ] Releases, Roadmap, Issues, Security, and License links are accessible.
- [ ] Image path capitalization matches the filesystem.

## Command checks

- [ ] The `awiki-me` source build succeeds on a supported platform.
- [ ] `awiki-cli version`, `status`, `doctor`, and the first-message flow run successfully.
- [ ] The public CLI installation command contains no release template variables.
- [ ] `awiki-open-server` can create a venv, install, start, and pass `healthz` from an empty directory.
- [ ] Open Server passes `smoke-local` or an equivalent first-success check.
- [ ] Public deployment documentation does not enable `AWIKI_ALLOW_UNSIGNED_PEER_DEV` or contact-verification compatibility.

## Compatibility checks

- [ ] Record the verification date and each component version or commit.
- [ ] Verify AWiki Me to hosted-service direct, group, attachment, and contact behavior.
- [ ] Verify awiki-cli to Open Server identity, direct message, group participation, attachment, people, and site behavior.
- [ ] Verify the AWiki Me to Open Server custom-tenant workflow or mark it explicitly as unverified.
- [ ] State Agent/Daemon behavior on self-hosted domains outside the allowlist.
- [ ] Do not describe Web as an available product platform.

## GitHub repository home page

- [ ] The About description matches the README opening.
- [ ] Topics are precise and limited to four to eight.
- [ ] A Social Preview is configured.
- [ ] Badges are necessary and accurate.
- [ ] The default branch contains the latest README.
- [ ] If the repository name differs from the product identity, the README opening removes the ambiguity.

## Unfamiliar-user test

Invite at least three to five people who do not know the current architecture and ask them to use only the repository home page:

- [ ] After 10 seconds, they can describe the project in one sentence.
- [ ] After 30 seconds, they understand its relationship to the other two AWiki repositories.
- [ ] After 60 seconds, they can decide whether they are a target user.
- [ ] Within five minutes, they complete a meaningful first success.
- [ ] They can accurately state the maturity, platform status, and security boundaries.
