# awiki-cli-rs2 Compatibility and Maturity

[English](compatibility.md) | [简体中文](compatibility.zh-CN.md)

Last reviewed: 2026-07-23. Add real release versions, commits, and verification dates before public release.

## 1. Capability status

| Area | Current status | Public wording |
| --- | --- | --- |
| Identity | Implemented | Usable; registration and recovery depend on the service and identity provider. |
| Messaging | Partially implemented | Do not claim every messaging capability is production-ready. |
| Group | Implemented | Individual servers may still limit administration methods. |
| Runtime | Partially implemented | Validate listener and Host Notification scope per platform. |
| Page | Implemented | Handle-page capabilities. |
| Site Pages | Implemented | Tenant bare-domain pages. |
| Discovery | Partially implemented | Do not describe the workflow as complete automatic discovery. |
| People | Partially implemented | Relationships/local contacts work; search is incomplete. |
| Debug helpers | Partially implemented | Troubleshooting only, not a primary product path. |

## 2. CLI platform targets

Current release configuration lists `darwin-arm64`, `darwin-amd64`, `linux-amd64` with a static musl strategy, and `windows-amd64`. On Windows 11 ARM64, the installer identifies the ARM64 host separately and selects the real `windows-amd64` artifact through Windows x64 app emulation when the manifest does not declare a native `windows-arm64` entry. A declared but invalid ARM64 entry fails closed. The manifest and installer logs continue to report the selected artifact as `windows-amd64`; native Windows ARM64 is not a current release target. Listing a target does not prove every release passed complete system tests; the manifest must record what was actually produced.

## 3. SDK platforms

`awiki_im_core` natively targets Android, iOS, macOS, and Linux. Flutter Web is currently only a stub that throws `UnsupportedError`.

## 4. Server compatibility

| Service | Identity | Direct | Group | Attachment | People/Site | Secure |
| --- | --- | --- | --- | --- | --- | --- |
| AWiki hosted services | Primary path | Primary path | Primary path | Primary path | By service capability | Verify per release |
| `awiki-open-server` | Local registration/compatibility routes | Plaintext send/inbox/history | Participant join/send/messages | Local object capabilities | Local smoke path available | No E2EE |
| Other AWiki-compatible service | Verify individually | Verify individually | Verify individually | Verify individually | Verify individually | Cannot be inferred |
| Generic ANP remote | DID/service discovery scope | Depends on public method | Limited methods | Limited object methods | Not equivalent to AWiki product APIs | Cannot be inferred |

## 5. Open Server impact

With `awiki-open-server`, the CLI can validate local DID registration, Direct/Inbox/History, open-group join/send/messages, People relationships and Site Pages compatibility, and local attachment slots/objects/tickets. It cannot require Direct/Group E2EE, assume full group create/add/remove/update, or depend on production SMS/email/Aliyun providers. Two CLI workspaces that both point to `awiki.info` are not evidence of Open Server interoperability.

## 6. Secure messages

Canonical entry points include:

```bash
awiki-cli msg send --to <handle> --text "..." --secure required
awiki-cli msg secure status
awiki-cli msg secure repair
awiki-cli group secure status
awiki-cli group secure repair
```

The CLI provides high-level secure-message intent and delegates protocol and local key state to the shared IM Core. Success depends on identity, peer, server, and secure profile; Open Server currently has no E2EE. Never expose MLS private state, KeyPackages, prekeys, ciphertext, or low-level debug commands to ordinary users.

## 7. Version consistency

Before release, run:

```bash
cargo run -p xtask -- check-version
cargo run -p awiki-cli -- version
```

Align `crates/awiki-cli/Cargo.toml`, `scripts/release/cli/release-config.json`, npm wrappers/packages, manifests, Skill metadata, embedded binary commits, ANP commits, and Daemon/Flutter native-artifact provenance.

## 8. Verification record

```text
Date: YYYY-MM-DD
CLI channel/version:
CLI commit:
ANP commit:
Server/domain/version:
Platform/arch:

Passed:
- install + version
- init + doctor
- register/recover
- direct send/inbox/history/read
- group lifecycle/messages
- attachment send/download
- runtime listener
- host notification
- secure direct/group (when applicable)
- people/pages/site

Failed or not verified:
- ...
```
