# im-core SDK Architecture

## 1. Positioning

`crates/im-core` is the reusable Rust IM SDK for awiki. It owns product capabilities that used to be spread through the CLI: identity, auth/session, directory, messages, groups, attachments, secure, realtime, email, content/site, and local state.

The SDK is not a collection of wire helpers, RPC parameter builders, SQLite helpers, or crypto utilities. Public callers construct `ImCore`, bind an identity into `ImClient`, then call high-level services.

```text
CLI / Flutter / App / Agent
        |
        v
ImCore                    # environment-level entrypoint
        |
        v
ImClient                  # identity-bound product client
        |
        +-- auth()
        +-- identity()
        +-- directory()
        +-- messages()
        +-- groups()
        +-- attachments()
        +-- secure()
        +-- realtime()
        +-- email()
        +-- content() / site()
```

## 2. Crate Boundaries

```text
crates/im-core       # SDK product capability layer
crates/awiki-cli     # CLI thin shell
crates/im-core-dart  # Rust-Dart facade
packages/awiki_im_core
                    # Flutter/Dart package and platform loader
```

Dependency direction is fixed:

```text
awiki-cli      -> im-core
im-core-dart   -> im-core
awiki_im_core  -> im-core-dart native library
```

`im-core` must not depend on `awiki-cli`, CLI command parsing, CLI config resolution, CLI workspace discovery, OpenClaw/Hermes UX, or service manager types.

## 3. Host vs SDK Responsibilities

| Layer | Owns | Does not own |
| --- | --- | --- |
| `im-core` | Product flows, auth retry, target resolution, local owner binding, remote transport, local projection, secure/realtime orchestration | CLI flags, stdout/stderr, exit code, workspace discovery, service install/start/stop |
| `awiki-cli` | Command parsing, config/workspace/path resolution, permission checks, dry-run, output envelope, daemon/service UX, OpenClaw/Hermes setup | Business flows, raw wire payload construction, auth retry, secure/MLS internals |
| `im-core-dart` / `awiki_im_core` | Dart-friendly facade, FFI lifecycle, platform native library loading | App UI/cache DTOs, `awiki-me` gateway policy, Flutter Web runtime |

The CLI handler target shape is:

```text
parse flags -> build ImCore/ImClient -> call SDK -> render output
```

CLI may parse `--to`, `--group`, `--text-file`, `--file`, and `--secure`; it passes `MessageTarget`, `MessageBody`, `AttachmentInput`, and `MessageSecurityMode` to SDK services.

## 4. Identity Model

`ImCore` is environment-level and does not bind a current identity. `ImClient` binds one identity and automatically carries actor, auth runtime, local owner, and identity-scoped state.

```rust
let core = ImCore::new(config, paths)?;
let client = core.client(IdentitySelector::Default)?;
client.messages().send(request)?;
```

Rules:

- Do not use mutable global "current identity" inside SDK.
- `Default` is one `IdentitySelector`, not hidden process state.
- CLI credential names map to `IdentitySelector::LocalAlias`.
- auth/session, local state, direct secure state, and MLS state must be identity-scoped.
- Business queries inject owner internally; callers do not hand-write owner filters.

### 4.1 Local multi-device authorization projection

The V1 identity registry persists one AWiki-local `device_state` for every
multi-device identity. It contains the random `ProtocolDeviceId`, public
signing/E2EE key IDs, `active|revoked`, `member|admin`, server-confirmed
`management_ready`, `auth_generation`, and the current Document/Registry
checkpoint. These fields are local authorization state; the interoperable device
list remains the root-signed DID Document's embedded `deviceManifest`.

The only active Registry combinations exposed by V1 are:

```text
active + member + management_ready=false
active + admin  + management_ready=true
```

Join does not create an `AdminAwaitingRoot` or
`active + admin + management_ready=false` state. An admin projection is ready
only when the Registry reports the second state and the local active Root Vault
record can be opened. Revoked devices and local Vault/auth/checkpoint
inconsistencies fail closed.

The formal local identity shape is one `DeviceIdentity`:

```text
device id
device signing private ref
device E2EE private ref
Manifest/Registry authorization checkpoint
optional access token
root capability = absent | pending | active
```

The device signing key is mandatory. The DID root ref is optional: a member can
authenticate and communicate without it. `pending` root material is restricted
to the current root-import completion; ordinary DID management requires both an
`active` root ref and a ready-admin Registry projection. The signing, E2EE, and
root key roles remain distinct and cannot substitute for one another.

New registration continues through the existing `register` product method. Core
generates one root key, independent signing/E2EE device keys, and a random
protocol device ID, then builds a root-signed DID Document containing exactly
one bootstrap Manifest entry. The server atomically creates User, Handle, DID
checkpoint, Registry, and the first ready admin. The existing registration
result returns one access token; it does not return a device refresh token.
There is no production `device_genesis`, Genesis grant, or multi-device
registration rollout branch.

A local encrypted pending-registration record may preserve generated key
material and the exact operation across an ambiguous network result. It is only
a crash-recovery mechanism: it must not introduce a second remote registration
protocol or generate a second identity on retry.

Legacy identities keep `device_state` absent until an explicit one-time upgrade.
Only the original device that still has the usable Legacy `key-1` is supported:
Core treats that key as the existing DID root, creates new independent device
keys and ID, and submits the same-DID/same-Handle single-device Manifest through
the existing document-update path. The encrypted pending upgrade is reused
after ambiguous failure. V1 does not support concurrent upgrade from copied
Legacy roots, Join before upgrade, or recovery after the original root is lost.

Device authentication is access-only. Core explicitly selects the current
`device_signing_key_id` for a fresh DID-WBA signature. Any successful User
Service RPC handled with that signature can return a new access token in the
standard authentication response headers; `get_me` is only the recommended
no-side-effect bootstrap when there is no business RPC to execute. Bearer
requests do not renew tokens. Core validates the returned DID, user, device,
key, generation, scope, audience, purpose, and expiry before atomically
replacing the one persisted access token. V1 has no device-token issue or
refresh RPC and stores no device refresh token.

Device Join keeps unauthenticated new-device and authenticated ready-admin
transports separate. The new device may create, poll, respond, cancel, and
observe its own HTTP Join session. Existing ready admins discover work only
through the generic P3 System Notification path; they do not
poll Join lists or status in the background.

Core uses the target DID's unique `ANPMessageService.serviceDid` only as the Home Service domain
trust anchor. The P3 Business Origin must be a separately resolved
`did:wba:<home-domain>:agents:system-notification:e1_*` Agent DID with a valid E1-bound document
proof and Origin Proof; it must not equal or impersonate `serviceDid`. Core also verifies the
candidate Join Request before exposing it. The admin's first explicit action
submits claim and encrypted Challenge in one RPC/CAS. The two devices derive
their six-digit SAS locally. Only the short-lived display value may cross the
Core-to-host facade. Neither the SAS nor the ephemeral pairing shared secret
enters the network, Outbox, persistence, or logs; restart-safe protocol inputs
and the pairing-key reference remain in encrypted local Join/Vault state. While
the remote session remains `response_verified`, the candidate re-derives the
same SAS from that state on every poll. This closes the
response-submit/process-crash boundary without persisting the display value.
After host user-presence, approval atomically commits the DID Document, Registry
member row, and consumed Join session.

Remote `consumed` is not sufficient local authorization. The candidate resolves
the DID Document independently, verifies its exact Manifest entry and keys,
then performs a fresh device-signed User Service request and stores the returned
member access token with the rootless identity. Only after identity, checkpoint,
and token persistence commit is the Join reported authorized. A request to
continue as admin starts a separate root-transfer flow; it never changes the
Join result. Hosted device auth, live Join verification, restart-safe
activation, Root workflows, and P5 publication use one document-bound decoder
for canonical vNext `JsonWebKey2020` OKP Ed25519/X25519 methods; no consumer may
reinterpret those methods through a different generic verification-method
parser.

Public host DTOs expose only safe session, DID, device, role, status, readiness,
fingerprint, short-lived local SAS display, and UI-action facts. The explicit
Device Registry read snapshot additionally exposes `registry_version` and each
device's `auth_generation` as canonical decimal strings so an App can replace
its display-only account-state cache monotonically. Those values do not
authorize Join, revoke, or root transfer; all security actions still perform a
fresh Core Registry read. The current User Service Registry stores both values
as `u64`; Core converts them to decimal strings before the Dart/Flutter boundary
so no Dart or JavaScript numeric representation can narrow them. Join
session/progress DTOs continue to exclude them.
OTP/account grants, Join tokens, pairing private keys, Challenge plaintext, SAS
derivation material, root plaintext, Object Proof secrets, document
version/hash, and other internal checkpoints do not cross the host facade or
CLI output boundary.

Local identity deletion is an offline Core transaction, not a remote logout
operation. Core first persists a secret-free retirement marker keyed by the
immutable identity ID, then atomically removes the identity from the registry
and default pointer before deleting its owned directory and every Vault record
whose `identity_id` matches exactly. Startup recovery resumes incomplete phases.
Completed identity-ID tombstones repeat Vault cleanup on later opens so an
operation admitted before host teardown cannot resurrect credentials after
deletion returns. Directory deletion additionally verifies the persisted
identity ID and DID, preventing an old retirement record from deleting a path
that has since been reused by another identity.

The host must detach its active-session pointer before invoking deletion and
may stop realtime/dispose runtime only as best-effort cleanup. Network shutdown
latency is therefore never part of the local deletion result, and a generic UI
network timeout must not classify this operation.

Legacy upgrade is likewise one Core-owned, resumable lifecycle operation:
Vault migration, document update, fresh device authentication, Registry
verification, and local promotion share one pending record. The host awaits the
typed terminal/retry projection and must not wrap this future in a shorter
generic request timeout; such a timeout does not cancel the native operation
and can otherwise start a concurrent retry. Immediate and persisted failures
use the same safe allowlisted categories (`transport_unavailable`,
`service_error`, `permission_denied`, `auth_required`,
`local_state_unavailable`, or `legacy_upgrade_failed`) without exposing
transport bodies or secret state.

### 4.2 Stable account binding for message sync

Every local vNext client has one fail-closed sync identity projection:

```text
owner_identity_id     <- immutable local identity ID
account_id            <- identity index user_id
current_did           <- current local DID snapshot
protocol_device_id    <- current active vNext authorization
identity_generation   <- Handle binding_generation
device_auth_generation <- current device authorization generation
```

`ImClient::active_sync_account_binding()` is the only public boundary that
materializes these six values together. It does not use
`IdentitySummary.device_id`, a vault-context device id, a DID-derived account
key, or a constant generation. When an older identity index has no
`binding_generation`, Core performs an authoritative public WNS lookup,
verifies the exact full Handle and current DID, persists the returned
generation, and only then returns the binding. Transport failure remains a
typed transport error; malformed or mismatched authority data fails closed.
Legacy and hosted clients return unsupported rather than receiving a guessed
binding.

Both generations are canonical positive decimal strings and are never narrowed
to a machine integer. The local binding reducer is monotonic: neither
generation may move backwards, `current_did` cannot change at the same
`identity_generation`, and a DID rotation is accepted only with a newer
identity generation. The `(account_id, protocol_device_id)` pair cannot be
rebound to another local owner.

Management-device root transfer reuses the ordinary exact-device P5 v2
implementation. After eligibility and PreKey/session checks, one explicit user
confirmation authorizes one target and message ID; V1 does not add a system
PIN/biometric step to this transfer. An existing session sends a standard
Cipher; when no session exists, the first standard Init carries the same
RootKeyEnvelope as its first application plaintext. Core never sends an empty
Init and never asks for a second confirmation.

The sender persists ratchet state and byte-identical retry ciphertext before
network I/O. The receiver processes the control JSON before ordinary message
projection, revalidates the current Manifest/Registry and root fingerprint, and
atomically seals the root as a `pending` Vault capability together with the
consumed message and exact completion state. Root plaintext and control JSON
never reach History, conversation, notification preview, search, Dart, CLI, or
ordinary backup surfaces.

The receiver then submits one HTTPS `device_root_import_complete` request with
an outer importing-device Object Proof and an inner root-possession Object
Proof. User Service verifies the current Registry, both proofs, and the ordinary
P5 trusted route tuple, then atomically changes the member directly into a
ready admin and increments `auth_generation`. After reading that authoritative
state, Core promotes the pending root ref to active and obtains a management
access token through a fresh device-signed request.

There is no root-specific delivery class, private completion sidecar, encrypted
imported ACK, ACK-driven readiness, empty-Init phase, or root-transfer rollout
state machine in the target architecture. P5 Reply only converges the standard
session. Transfer or completion failure leaves the already joined device as a
member.

### 4.2 P5/P6 public message product paths

Ordinary multi-device messaging uses two independent, host-local rollout gates:
`multi_device_direct_e2ee_enabled` selects exact-device P5 v2 Direct only for a
local vNext identity, while `multi_device_group_e2ee_enabled` selects the
device-scoped P6 v2 Group path. Both default to `false`; neither is an ANP
capability, DID Document member, nor cross-domain request field. Turning either
gate off preserves its existing message route and does not disable the other.

P5 keeps one public logical message while the product runtime resolves the
target DID Document's embedded `deviceManifest`. It sends one standard
`direct.send` per exact recipient device and per eligible sibling device of the
sender; it never invents a cross-domain `deliveries[]` request. Sibling copies
use the encrypted own-sync application form and project as outgoing logical
messages. A local, secret-free delivery ledger aggregates accepted/failed
devices, preserves partial success, and makes a retry with the same logical
message/idempotency identity skip already accepted devices. Attachment bytes
are encrypted and uploaded once; only their Manifest is wrapped independently
for each exact-device Direct session.

P6 reads current business group state through the standard P4 boundary, then
encrypts one application into exactly one MLS ciphertext and submits that
ciphertext once, independent of the number of group Leaves. A group attachment
is likewise encrypted and uploaded once, with its Manifest carried inside the
single MLS Application message. Every device still owns independent MLS local
state; the one-ciphertext rule does not imply shared Leaf secrets.

P6 的本地 MLS OwnerScope 每次都从 identity index 中当前 `active` 的 vNext
device authorization 读取 `ProtocolDeviceId`。重启或重建 `ImClient` 后仍使用同一
权威设备标识；不得依赖进程内 `IdentitySummary.device_id`，也不得为 legacy、缺失授权
或已撤销设备合成 sibling/`default` fallback。

Inbound confidentiality filtering is gate-independent. Inbox/History, reliable
sync, realtime, and delegated projections recognize P5/P6 v2 candidates before
legacy rendering. Enabled paths may expose only an authenticated, decrypted
business projection (including an outgoing projection for own-sync); handshake,
notice/control, replay, malformed, failed, or gate-disabled candidates are
consumed or dropped. Raw v2 wire bodies, ciphertext, and control JSON never
cross the Rust/Dart/CLI/App public boundary and never fall back to a legacy
plaintext renderer.

Realtime own-sync keeps the current client identity as the local storage owner
and routes the projected outgoing message to the decrypted `target_did`.
It must not reinterpret that external target as the local owner or retain the
same-user wire sender as the Direct peer; the committed wire route, canonical
Persona conversation, sender/receiver snapshots, and UI hint all use the same
external peer.

For P6, blocking/async read and realtime share the same internal notice
consumer. A standard `group.e2ee.notice` is bound to the current owner
DID/device, resolved against the current P4 group-member DID documents, and
passed to the SDK's durable, idempotent MLS notice state machine. Controls are
never projected as messages or events; malformed, unknown-profile, wrong-device,
or wrong-group inputs fail closed.

## 5. Paths and Configuration

Hosts pass explicit `ImCoreConfig` and `ImCorePaths`.

Host responsibilities:

- workspace and `config.yaml` resolution.
- identity root/default/registry path selection.
- DID document, key, auth/session, SQLite, runtime, cache, and temp paths.
- directory creation, chmod, backup, cleanup, and migration timing.

SDK responsibilities:

- read/write only the explicit paths passed by the host.
- bind paths to the selected identity.
- initialize and migrate local state through `CoreBootstrap`.
- avoid CLI workspace auto-discovery and CLI config parsing.

## 6. Public/Internal Boundary

Public API expresses product intent. Internal implementation owns wire, store, crypto, and transport details.

| Module | Public API expresses | Internal only |
| --- | --- | --- |
| core | `ImCore`, `ImClient`, config, paths, bootstrap, errors | `ClientIdentityRuntime`, path expansion, store handles |
| identity | selectors, summaries, registration, Legacy upgrade, recovery, device Join/admin promotion, permanent device revoke result/outcome category, profile, DID replacement plan | private key material, DID writer, raw identity store rows, revoke checkpoints and pending intents |
| onboarding | Skill Token claim request/result and resumable claim operation | raw Token transport, pending key bundle, journal, DID generation, exchange and greeting orchestration |
| auth | login, ensure, device-signed access-token renewal, refresh, status | proof builder, JWT file format, bearer header handling |
| directory | peer resolve, handle lookup, contacts, relationships | user-service raw request/response, contact store rows |
| messages | send, inbox, history, mark-read, conversations, reliable sync | message RPC params, wire DTOs, raw notification frames, checkpoint load/store |
| groups | lifecycle, members, profile/policy, group reads | group wire helpers, raw group receipts |
| attachments | send/download, source/destination DTOs | upload slots, object commit, ticket params, encrypted manifest internals |
| secure | status, prepare, repair, outbox summary, secure send policy | ciphertext, prekeys, KeyPackage, MLS private state, provider IO |
| realtime | status, runner, event stream, normalized `ImEvent` | WebSocket frame, request id, ping/pong, dispatch queues |
| email | account, inbox, read, mark-read, send, attachment, notifications | mail RPC params, raw JSON payload, auth headers |
| content/site | page/site product operations | content/site RPC envelope and wire normalization |

## 7. Module Map

- `core`: environment entrypoint, identity-bound client, bootstrap, errors, common IDs and paging types.
- `identity`: local registry, default identity, Handle registration, one-time Legacy upgrade, recovery, device Join/admin promotion, permanent device revoke and Identity-only pending recovery, profile, contact binding, and DID replacement plan.
- `onboarding`: environment-level Skill Agent claim for an initialized, empty workspace. It verifies the scoped Token before key generation, persists a recoverable pending identity, exchanges it for a new DID identity, authenticates, and sends the deterministic Controller greeting before completion.
- `auth`: DID-WBA, access-only session persistence, device-signed token renewal, refresh, status, and retry support for business services.
- `local_state`: SQLite schema, owner isolation, messages, contacts, groups, email notification, secure outbox, realtime projection, and reliable sync checkpoints.
- `discovery`: endpoint and capability selection from config, DID documents, profile, and service metadata.
- `directory`: DID/Handle lookup, public profile, contact projection, relationship APIs.
- `messages`: direct/group send, inbox, history, conversations, mark-read, retry plan, local message projection.
- `groups`: group lifecycle, members, profile/policy, group message reads, group E2EE lifecycle hooks.
- `attachments`: upload, digest, manifest, message send, ticket download, local file or memory sinks.
- `secure`: direct E2EE, group E2EE, status/prepare/repair, secure outbox, secure message orchestration.
- `realtime`: embeddable WebSocket runner, reconnect, notification projection, host notification events.
- `email`: account, inbox/read/send/mark-read, attachment download, mail notifications.
- `content/site`: handle content pages and tenant bare-domain site pages.

## 8. Runtime and Features

`im-core` is blocking-first. Flutter/Dart and App hosts expose async APIs by running SDK work on their own worker thread or platform runtime. Any future async public API must be designed separately from the current blocking contract.

Transport is explicit through configuration and capability checks:

- `HttpOnly` keeps business operations on HTTP/RPC.
- realtime runner requires a non-HTTP-only transport policy and returns a capability error when unavailable.
- realtime session startup does not require a cached bearer token before spawning the runner. The auth layer first tries the cached token; when it is missing or receives `401`, it performs one fresh device-signed User Service request, stores the access token from the authentication response headers, and retries once. Bearer transport never renews itself, and no device refresh token is used.
- group E2EE, secure direct, SQLite-backed state, and advanced provider traits are feature-gated where appropriate.

## 9. Security Rules

- Remote messages are untrusted input.
- CLI/App output must not expose JWTs, private keys, raw secure state, ciphertext internals, MLS artifacts, provider stdout/stderr, or host secrets.
- Skill onboarding requests use a redacted, non-serializable Token type. Token HTTP requests reject redirects; journals contain only non-secret scope and recovery state. A non-empty or ambiguous workspace fails closed.
- Host notification payloads must contain approved event summaries, not raw message instructions.
- Diagnostics may expose lower-level details only behind explicit debug/diagnostic gates.
- Whole-roster Group security decisions use a bounded, version-bound
  `group.list_members` page collector. No MLS mutation may begin until every page has the same
  Group DID and canonical state version, cursor progress and totals are complete, and the
  authoritative `max_members` policy and implementation hard cap are satisfied.
- Permanent device revoke completes at the validated User Registry/DID Document result and local
  Identity convergence. It does not scan or wait for every MLS group. Message Service keeps the
  durable per-group send-pause gate; an owner device with local controller state converges a
  selected group only through explicit group repair.

## 9.2 Device Revoke And Group MLS Convergence

`PendingDeviceRevoke` is an Identity exact-retry record, not a second MLS work queue. The
destructive request persists its stable intent before submission. A validated remote result is
persisted before local DID Document/checkpoint convergence, after which the pending record is
deleted. Identity/session activation and a successful fresh Registry read may resume only that
local convergence. A record that already contains a validated remote result converges and deletes
without Registry or DID Document network access. Only a record without that result requires the
exact Registry, generation, checkpoint, DID Document hash and Manifest match. Recovery is bounded,
shares the revoke lock, never submits a new revoke request, and never touches MLS.

Message Service owns the per-group `device_revocation_pending` fact. Core group secure status reads
the Host-authoritative, low-sensitivity `group.get.e2ee_maintenance` projection before reporting
readiness. A gate plus active owner and local controller state becomes `NeedsRepair`; a non-owner
becomes `WaitingForMembershipUpdate`; a device without controller state becomes
`MissingLocalState`. A missing or malformed authoritative response fails closed and cannot be
reported as `Ready`. Status is read-only: it does not enumerate the roster, resolve Manifests,
write the MLS WAL, or build a Commit. The low-sensitivity maintenance object accepts exactly
`reason` and `send_paused`; target identifiers, counts, and other fields are rejected rather than
silently projected.

## 9.1 Key Material Boundary

The full current technical design is documented in
`docs/architecture/identity-secret-storage.md`. This section is the short
architecture summary.

Identity private material is an internal SDK concern. Business flows must not read `private_key_path`, `e2ee_agreement_private_key_path`, PEM files, or `auth.json` directly. DID-WBA auth, direct/group message signing, attachment signing, and secure direct static key loading go through the internal `KeyMaterialProvider` contract. That contract exposes separate device-request-signing and DID-Document-root accessors: daily authentication and messaging consume `device_request_signing_material` as an atomic `(verification method, private key)` pair, while only DID Document creation/re-sign/update may request the root accessor. A caller must not combine the current device private key with the first `authentication` entry from the shared DID Document because its ordering is account-wide and may name another device. Legacy `key-1` identities retain their dual-role behavior only through an explicit compatibility adapter. vNext vault refs require a device-signing key but make the root ref optional, so a member device can authenticate without possessing DID root control material and root-only operations fail closed.

The compatibility default remains file-backed when a host opens `ImCore` without
explicit vault options:

- DID documents are read from the identity directory.
- DID/default signing keys are read from `private.key` or `key-1-private.pem`.
- secure direct agreement keys are read from `e2ee-agreement-private.pem` or legacy `key-3-private.pem`.
- auth/session state remains compatible with `auth.json`.

Vault-backed identity storage is explicit and no-prompt by design:

- Hosts pass `ImCoreOpenOptions` with `IdentitySecretStoragePolicy::VaultPreferred` or `VaultRequired` plus `ImCoreSecretVaultOptions`.
- The vault root key is a host-provided no-prompt secret. It must not be written to `ImCoreConfig`, CLI workspace config, ordinary App JSON state, logs, diagnostics, JSON output, or `Debug` output. Explicit E2E runs may use a private file test provider that remains local and untracked.
- `SecretVault` stores per-record AEAD ciphertext and binds workspace, local vault-context device, identity, DID, kind, key id/version, schema, cipher, KDF, and no-prompt policy into authenticated metadata. The vault-context device id is a local storage scope and is a distinct Rust type from the random `ProtocolDeviceId`; it must never be published in a DID Manifest or copied into cross-domain messages.
- `VaultRequired` is fail-closed. Missing root key, missing vault context, wrong workspace/device metadata, corrupt metadata, or failed open/verify must not silently fall back to plaintext for new secret persistence.
- In `VaultRequired`, new registration, one-time Legacy upgrade, device Join/admin promotion, daemon subkey package persistence, and access-token replacement use vault-backed persistence and must not write private PEM/JWT material to the legacy identity files.
- Identity vault migration seals records, opens them back for verification, and only then writes `vault_migration` metadata. Existing PEM/auth.json compatibility files are retained until an explicit cleanup path is available; migration failure must not delete or quarantine them.
- Status, migration, and verification APIs expose backend/status/warnings summaries only. They must not expose the root key, private key, JWT, full `SecretRef`, or ciphertext internals.

Process boundaries matter. App, CLI, and daemon run as separate hosts and must each unlock or provide their own vault context for their own state root. Do not assume one OS keychain item is readable across all processes.

Current host integration status:

- Plain `ImCore::new` / `open` remains FileCompat for compatibility. Secure callers must pass explicit vault options.
- `awiki-cli` resolves `secret_storage.mode`, `vault_dir`, `workspace_id`, and `device_id` from workspace config. The root key is read from `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` when present, otherwise from `vault_dir/root-key.b64u`; normal live paths may create that local private root-key file, while status/dry-run surfaces only report a redacted plan. `id vault status`, `id vault migrate`, `id vault cleanup-plaintext`, and doctor output are redacted.
- `im-core-dart` / `packages/awiki_im_core` expose optional Dart open options plus identity vault status/migrate/verify facade methods. The Dart package does not generate or persist host root keys.
- `awiki-me` opens `im-core` with `VaultRequired`. Production and custom state-root runs use `SecureAppKeyValueStore` for the App-local root key; only explicit E2E state roots use a private file test provider.
- `awiki-deamon` stores daemon/runtime `agent_identity` private keys and `user_delegated_identity` private keys as SecretVault refs in `daemon.db`; the legacy PEM columns keep a sentinel for compatibility. Older plaintext rows are read only as a migration bridge and are re-sealed when a daemon vault root key is available.

Known residual risks after the App/CLI/daemon vault integration:

- CLI root keys supplied through `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` are visible to the process environment; CLI root keys stored in `vault_dir/root-key.b64u` rely on private local file permissions. A platform wrapping/root-key backend and rotation/backup story remain follow-up work.
- App root key rotation, backup, recovery UX, and secure deletion of old plaintext compatibility files are not implemented.
- `id vault cleanup-plaintext` is a migration-gated/preflight surface unless a CLI-safe live cleanup API is added. Do not document it as deleting legacy files in this build.
- Explicit delegated `key_ref` flows support `vault:` refs and should use them for new daemon-owned delegated keys. `file:` / `local:` / bare path refs remain compatibility inputs and can still read caller-provided delegated private key files.
- The daemon Message/im-core SDK main path uses hosted in-memory identity material and no longer writes `private.key`, `e2ee-agreement-private.pem`, or `auth.json` for that path. Legacy DID-auth compatibility helpers may still create those files for user-service inventory/auth paths and should be treated as compatibility-only.
- The App bootstrap path can still receive a daemon subkey private key plaintext DTO. This is a temporary compatibility exception and should be replaced by an encrypted bootstrap envelope in a separate change.
- Direct E2EE session/prekey local state is encrypted at rest through SecretVault envelopes. Group MLS private state is outside this hardening pass.
- `awiki-deamon` `agent_auth_state` bearer tokens are persisted as daemon SecretVault refs with a sentinel in the `jwt_token` column; do not log or expose them.
- External key-agent IPC, public signing APIs, and DID child-key scope/revocation semantics are outside this boundary.

## 10. API References

Stable API references live under `docs/api/`:

- `docs/api/im-core-public-api.md`
- `docs/api/im-core-interface/*`

These files describe the SDK public surface and interface-level contracts. They should only change when the API changes; architecture-only cleanup should update this document and related feature docs instead.

## 11. Durable Conversation Registry And Summary Projection

The SQLite local state keeps `messages` as the durable message projection truth, while target schema version 29 uses `conversation_registry` as the durable conversation-existence truth. This distinction allows a validated Direct or Group conversation to remain in the recent list before its first message. `conversation_summaries` remains a rebuildable user-visible-message aggregate and may legitimately have no row for an empty conversation. Protocol/control records, including group lifecycle events, stay in the durable message projection when required but do not create or replace a conversation summary; the registry preserves the conversation independently. The current conversation/read/send projection contract keeps:

- primary key: `(owner_identity_id, conversation_id)`;
- hot index: `idx_conversation_summaries_owner_last(owner_identity_id, last_message_at DESC, conversation_id)`;
- unread index: `idx_conversation_summaries_owner_unread_last(owner_identity_id, unread_count, last_message_at DESC)`.

`list_conversations_for_owner_identity()` reads active `conversation_registry` rows by owner, left-joins `conversation_summaries`, and joins only the stored `last_message_id` back to `messages`. The legacy `threads` SQLite view remains available for debugging and compatibility, but it is no longer the chat-list hot path. Incremental writes update touched summaries inside the same SQLite transaction as message/read-state projection; rebuild/repair paths remain available when a gap, migration, or debug check requires recomputing owner summaries from durable `messages` and `thread_read_state`.

Summary rows are derived state and may be rebuilt from `messages`, but hot writes are incremental after the performance work:

- schema open creates the table/indexes and backfills v17 stores when summaries are absent;
- ordinary message insert/update updates `conversation_summaries` by delta in the same SQLite write transaction;
- bounded mark-read, `mark_conversation_read`, and legacy `mark_thread_read` update unread / unread mention counters by delta where the previous state is known;
- fallback rebuild remains for message conversation moves, legacy DID-to-peer-scope direct merges, last-message ambiguity, missing/corrupt summary rows, first unread mention ambiguity, and explicit owner repair;
- committed invalidation is evaluated only after the local projection transaction commits; a runtime store increments its version and emits a patch only when the committed conversation/timeline projection materially changed. An unchanged projection is a no-op, not a synthetic `Reset`;
- verified Direct alias correctness is persisted in owner-scoped `conversation_aliases`; a SQLite TEMP memo may skip repeated work within one connection, but it is only a performance cache and is never identity evidence. Legacy DID rows are folded only when the DID belongs to the target `peer_persona_id` in verified identifier history. The alias insert is conflict-visible and the legacy registry row becomes `merged + resolved` with an explicit target.

`messages.ensure_conversation()` / Dart `client.messages.ensureConversation(...)` is the explicit user-open creation boundary. Direct creation fails closed unless the owner has a valid `direct_peer_routes` entry for the canonical `dm:peer-scope:v1:*` ID. Group creation fails closed unless the owner has an active local membership projection. Successful active Group create/join/get/add-member/refresh projection also idempotently ensures that same canonical Group DID registry row inside Core, so an empty Group conversation does not depend on an App navigation callback or first message to remain visible. The registry stores `activity_at` independently of `last_message_at`; list pagination uses the opaque v2 cursor ordered by `activity_at DESC, conversation_id DESC`. Migration only backfills conversations represented by verified routes, Group projections, summaries, or preserved legacy rows and never invents an identity from display data.

Every fresh Handle discovery path must receive an available authority status and a stable non-DID `user_id`/`subject_id` before it can build a Direct Persona. Both local directory lookup and public `/.well-known/handle/` discovery validate the same authority/subject/Handle contract, and public discovery additionally verifies that its `did:wba` provider domain matches the Handle authority; a missing or DID-shaped subject returns `identity_unresolved` instead of manufacturing a canonical Direct ID.

`ConversationIdentity.conversation_id` is the SDK-level routing key for message display. Conversation list rows, message metadata, timeline patches, read-state updates, conversation-scoped send, and local repair must carry or derive from this canonical identity. `ThreadRef::{Direct, Group, Thread}` remains a compatibility / adapter surface for CLI migration, legacy callers, and low-level diagnostics. New AWiki Me and Flutter SDK message-display paths must not reconstruct a route from DID, handle, or legacy direct aliases when a canonical `conversation_id` is available.

`direct_peer_routes` is a routing projection, not a message or conversation
truth. A successful directory lookup with stable `user_id + full_handle`
upserts `(owner_identity_id, conversation_id) -> current_did` after recomputing
the v1 peer-scope hash. This lets the first text, payload, attachment, read, or
sync operation resolve an otherwise non-reversible canonical ID before any
message exists. DID rotation updates `current_did` without changing the
conversation ID. Missing, cross-owner, or integrity-invalid routes fail closed;
message metadata/participants remain a compatibility fallback for conversations
that predate the route projection. App and CLI callers must never manufacture a
`dm:<DID>` alias to bypass this resolver.

Schema 28 makes the verified authority identity explicit. A canonical Direct is
created from `authority_namespace + authority_subject_id + full_handle`, where
the namespace is the IDNA-normalized authoritative Handle provider domain and
the subject comes from a usable Handle Authority response. The projection is
persisted in `peer_personas` and `peer_identifiers`; `direct_peer_routes`
references the Persona while keeping `current_did` as a replaceable delivery
route. A verified legacy DID reference is written to append-only
`conversation_aliases`. Alias insertion is conflict-visible (`INSERT OR IGNORE`
followed by target verification), never last-write-wins. Registry lifecycle and
canonical resolution are orthogonal, so `active + legacy_unresolved` cannot be
mistaken for a resolved canonical row. An alias may continue to target a
resolved canonical conversation after that target becomes `archived`, `left`,
or `deleted`; it must never target an unresolved or `merged` registry row.
Once a legacy registry row is `merged`, ordinary summary refresh or
`ensure` calls cannot reactivate it; only the canonical target remains eligible
for the active conversation list.

Schema 28 also separates immutable protocol facts from mutable local
conversation projection. Each message stores `wire_thread_kind`,
`wire_thread_ref`, and `wire_identity_resolution_state` alongside the canonical
`conversation_id`; the old `thread_id` column is deprecated compatibility data.
Canonical alias merge may update only `conversation_id`. It must not rewrite
wire thread facts, sender/receiver DID snapshots, group identifiers, or
`server_seq`. Replaying the same owner/message ID with different non-empty wire
facts fails with `message_wire_identity_conflict`; a replay may only fill wire
facts that were genuinely absent in a legacy row.

Verified Handle projection writes the Persona, current and historical
identifiers, route, Persona-keyed profile, and matching contact association as
one local transaction. It is the only directory path that writes a canonical
Direct route; the former parallel scope/DID route projection is intentionally
absent so a route cannot bypass Persona validation or be written twice.
UI/profile consumers must eventually read display data by `peer_persona_id`;
DID remains a credential snapshot or route address, not a profile identity key.

Inbound Direct sync performs the same Persona lookup inside
the checkpoint transaction. If the peer DID is not yet bound to a verified
Persona, the immutable message projection is serialized into the owner-scoped
`inbound_resolution_backlog`; only after that durable write may the global
checkpoint advance. A later verified Handle projection replays matching rows
into `messages` and removes them from the backlog without changing their
`wire_thread_kind`, `wire_thread_ref`, or sender/receiver DID snapshots.

Remote history, thread catch-up, and realtime incoming projection use that same
canonical ingress rule even though they do not advance the account checkpoint.
They must resolve Direct wire DID snapshots through the verified Persona route
before writing `messages`. An unresolved record is written transactionally to
`inbound_resolution_backlog` with a stable source/message key and must not first
materialize a `dm:<DID>` message or registry row. Local pending/outgoing
projection remains a separate write path because it is created from an already
validated conversation/send boundary.

For an online first inbound Direct, the realtime ingress performs an
authoritative Handle lookup by the wire peer DID, verifies that the lookup DID
matches that snapshot, and projects the verified Persona/route before committing
the message. This is not a DID-derived Persona fallback. If authority lookup is
unavailable, malformed, conflicting, or returns another DID, the normal
canonical-ingress rule still places the message in the resolution backlog; no
legacy Direct row or authoritative patch is emitted.

The redacted canonical invariant doctor is available through the local-state
compatibility diagnostics. It reports counts and invariant labels only: active
Direct/Group exact-one violations, unresolved resolved rows, alias chains or
missing targets, route/Persona/registry mismatches, orphan profiles, invalid
merged rows, and messages without a canonical registry owner. It never logs
message bodies, complete identifiers, credentials, or key material.

An existing schema 27 database is not modified by ordinary schema open. Core
returns `local_state_upgrade_required` until the release/0710 backup/shadow/
validation runner performs the explicit 27→current cutover. The runner uses a
cross-process file lock and SQLite Online Backup, performs canonical mapping
inside a disposable shadow transaction, verifies conservation and canonical
invariants, and records a resumable redacted journal before replacing the live
SQLite file set. The pre-open detector owns exactly schema 27; already canonical
schemas 28 through current are a no-op there and remain owned by the ordinary
atomic schema migration in Core open.
The source allowlist is pinned to the exact deployed release/0710 daemon
artifact, source ref, and schema fingerprint. Its checked-in fixture is built
by that binary in an isolated state root and contains synthetic rows only.
After a completed cutover, the pre-open restore API verifies the retained
backup, keeps the current target as a private safety copy, and restores the
whole schema 27 file set; partial table-level downgrade is unsupported.

Because summaries contain message preview fields, diagnostics and tests should treat them as local private state. Do not expose message content, payload JSON, or sender details in public logs; only log counts, durations, and redacted identifiers.

## 12. Conversation Snapshot And Runtime Store

Conversation snapshot and patch APIs are non-authoritative acceleration layers on top of committed local projection:

- `messages.load_conversation_snapshot()` / Dart `client.messages.loadConversationSnapshot()` reads a redb snapshot generated from `conversation_summaries`.
- Snapshot entries use `ConversationSnapshotItem`, a core-only DTO containing thread identity, the committed Group profile title when applicable, participants, last message projection, unread counts, message count, and last message time. Group title comes from Core's owner-scoped `groups` projection; it is not an App presentation overlay.
- `messages.watch_conversation_patches()` / Dart `client.messages.watchConversationPatches()` streams versioned `ConversationStorePatch` values from an in-memory runtime store. A bound watch subscribes before reading and emits exactly one initial `Reset` seeded from the canonical SQLite projection; the redb snapshot remains available only through the explicit legacy snapshot API and is never an authoritative watch seed.
- `messages.repair_conversation_store()` / Dart `client.messages.repairConversationStore()` returns a reset/repair patch and the current runtime store version after lag, overflow, stream close, or version gaps.
- `messages.watch_conversation_timeline_patches(conversation, limit)` / Dart `client.messages.watchConversationTimelinePatches(conversation, limit: ...)` streams versioned `ThreadMessageStorePatch` values for the currently opened canonical conversation timeline.
- `messages.repair_conversation_timeline_store(conversation, limit)` / Dart `client.messages.repairConversationTimelineStore(conversation, limit: ...)` returns a reset/repair patch for the conversation timeline runtime store.
- `messages.watch_thread_patches(thread, limit)` / Dart `client.messages.watchThreadPatches(thread, limit: ...)` and `messages.repair_thread_store(thread, limit)` / Dart `client.messages.repairThreadStore(thread, limit: ...)` remain compatibility adapters for CLI / legacy `ThreadRef` paths, not the AWiki Me display-chain owner.
- Dart bridge patch sessions retain cancellation ownership after the raw Core
  session moves into the background stream worker. `stopConversationPatchSession`
  and `stopThreadMessagePatchSession` signal that worker, wake an idle
  `next_patch().await`, and join it before returning. Conversation, canonical
  timeline, and compatibility thread streams share this lifecycle; dropping the
  Dart wrapper aborts any remaining worker as a final resource-safety boundary.
- Patch notifications are emitted only after the underlying local projection commit succeeds; `snapshot_required=true` or failed sync apply must not emit an authoritative patch.
- A committed invalidation whose projected items equal the runtime-store state must not increment the store version or emit `Reset`. `Upsert` / `Remove` are used for one-row material changes, `Reset` for material multi-row replacement, and explicit repair/lag paths remain allowed to emit repair/reset patches.
- Realtime incoming messages follow the same committed-projection rule: a WebSocket hint or decoded event is not authoritative by itself, but once its message projection is committed to SQLite, `im-core` emits conversation and conversation-timeline patches for active subscribers.
- A realtime incoming row without a thread-local `server_sequence` must not use the sender-provided `sent_at` as its local ordering timestamp. `im-core` records the recipient-side receive/commit timestamp until reliable sync or thread catch-up supplies the authoritative sequence and accepted timestamp. This prevents coarse or skewed sender clocks from reversing two distinct canonical messages during the realtime-to-sync convergence window.

The public APIs currently live under `messages()` / `client.messages` for compatibility with the existing SDK grouping. A future `conversations()` / `client.conversations` namespace may wrap the same core store, but both names must not expose divergent DTOs or ownership semantics.

`ConversationSnapshotItem` and `ConversationStorePatch` must remain SDK/core DTOs. Their optional Group `title` is a committed Group profile projection, not an App-local override. They must not include `awiki-me` App-only presentation fields such as `hidden`, `pinned`, `muted`, `customTitle`, `avatarSeed`, `peerLifecycleState`, `ConversationSummary`, or `ChatMessage`. AWiki Me composes those fields in its own application layer; see `awiki-me/docs/conversation-presentation-ownership.md`.

Because snapshots and patches contain message preview fields, diagnostics and tests should treat them as local private state. Do not expose message content, payload JSON, or sender details in public logs; only log counts, durations, and redacted identifiers.

## 13. Local-first Message History

`messages.history()` keeps its remote history + projection/reconcile semantics. AWiki Me first paint should use `messages.local_conversation_timeline()` / Dart `client.messages.localConversationTimeline(...)` with a `ConversationReadRef`. Hot compatibility paths that only need already-projected local messages can still use `messages.local_history()` / Dart `client.messages.localHistory(...)`.

Local conversation timeline:

- reads only the local SQLite `messages` projection through `owner_identity_id` and canonical `ConversationReadRef.conversation_id`;
- does not call `direct.get_history`, `group.list_messages`, `inbox.get`, directory lookup, or E2EE remote projection;
- returns newest-first `MessagePage` items and an opaque `local-history:v1:*` cursor for paging older local messages;
- supports direct, group, and raw thread-backed conversations through the same owner-scoped conversation-id normalization as conversation mark-read.

The API is for fast first paint. Apps should show local conversation timeline rows immediately, then run `sync_conversation_after()` or a documented repair path in the background when freshness is needed. Remote history/backfill results are not UI truth until they have been persisted to the local projection and reloaded or emitted through the conversation timeline store.

## 13.1 Conversation Send And Local Echo

Conversation-surface sends should use `messages.send_conversation_text()` / Dart `client.messages.sendConversationText(...)`, `messages.send_conversation_payload()` / Dart `client.messages.sendConversationPayload(...)`, or `attachments.send_conversation()` / Dart `client.attachments.sendConversation(...)` when the caller already has a `ConversationReadRef`. `im-core` resolves the canonical conversation through its owner-scoped route projection, writes a durable pending projection row under that same canonical ID before network send, updates the row to accepted/sent/failed as the network result arrives, and emits committed patches only after the SQLite transaction succeeds. The returned `SendMessageResult.message.metadata.conversation_identity` must expose that same canonical ID after direct-route normalization; it must not retain the transport target Handle or DID identity used before the network send. The first message in a peer-scope conversation does not require a pre-existing message row and must not be bootstrapped with a legacy DID conversation alias.

`MessageMetadata.send_state`, `MessageMetadata.retry_plan`, and `MessageMetadata.conversation_identity` are the SDK facts for pending/accepted/sent/failed presentation. AWiki Me may render those states, but it must not create a second durable optimistic message store or decide send correctness from memory-only pending rows. Attachment local file preview may exist only as transient UI state during upload; list/detail timeline truth, retry correctness, and final send state must come from the SDK durable projection. Secure/E2EE conversation-surface local echo remains fail-closed where unsupported by the secure route.

## 14. Reliable Message Sync

Reliable message sync is split between service-owned event logs and
`im-core`-owned local recovery state. The service API is documented in
`message-service/docs/api/ANP-client-server-api-sync.md`; this document records
the SDK architecture boundary.

Ordinary v2 Direct/Group sync is the default product path for every valid
account/device binding. Core does not accept an account allowlist, device
cohort, or percentage rollout input. A host may explicitly disable the global
v2 reader only for emergency rollback; the independent P5/P6 E2EE gates in
Section 4.2 remain default-off and do not control ordinary synchronization.

`im-core` Rust/SQLite owns the global reliable checkpoint:

- `messages.sync_now()` / Dart `client.messages.syncNow(...)` are the v2 ordinary
  Direct/Group main path. Rust derives account/device binding internally,
  bootstraps a tail-only cursor and Group baseline when required, exactly
  hydrates every required `message.created` through `message.get_batch`, and
  commits event receipts, canonical projection changes, and the next v2 cursor
  in one SQLite transaction. Required hydration, schema, identity, or route
  failure rolls back the whole page and does not write a receipt or advance the
  cursor. Although the wire method accepts at most 100 event IDs and the service
  enforces a 16 MiB hard response budget, Core uses ordered chunks of 8 to leave
  headroom for compact-JSON framing and escaping; any unavailable item in any
  chunk aborts the full delta page.
- Before the first bootstrap for a local owner, Core persists a cryptographically
  random opaque `client_instance_id` in the local sync database. Lost responses,
  process restarts, and retries reuse it; a new/cleared local database generates
  a different value. It is not derived from owner, account, or device identity
  and is never exposed through the SDK.
- `messages.sync_delta()` / Dart `client.messages.syncDelta(...)` are high-level
  v1 compatibility calls. Rust reads the current checkpoint from local `sync_state`, injects
  `since_event_seq` into the wire request, applies the returned page, and writes
  the new checkpoint only after the local apply transaction succeeds.
- Public Rust, Dart, Flutter, CLI, and App APIs must not expose
  account/device binding, `loadGlobalCheckpoint`, `storeGlobalCheckpoint`, raw
  v1/v2 cursors, raw `since_event_seq`, raw `next_event_seq`, or equivalent
  manual checkpoint advance. The v1 and v2 cursor stores remain isolated.
- `snapshot_required=true` is fail-closed until a documented repair API exists:
  no checkpoint advance and no local projection wipe.
- An identity-unresolved inbound message is not a failed apply and is never
  projected into a `dm:<DID>` conversation. The same transaction stores it in
  `inbound_resolution_backlog` before advancing the checkpoint; verified
  Persona projection later performs idempotent replay. Binding conflicts remain
  conflict-visible rather than being guessed or last-write-wins.
- An ordinary P3 Direct event may be owner-scoped to the sender and contain only
  message/thread metadata. Core checks the exact `message_id` and `server_seq`,
  resolves the peer through the authoritative Handle directory, groups missing
  targets by Direct peer, and hydrates `direct.get_history` from immediately
  before the earliest missing sequence for that peer. Core resolves the peer
  scope once for the authoritative history page and reuses the verified result
  across its messages; it must not issue one history request per metadata event
  or one directory lookup per message. A later local thread sequence does not
  prove that an exact message exists. Core advances the global checkpoint only
  after every required message in the page is committed; an empty, incomplete,
  or non-advancing history response fails closed. This is sender-side reliable
  sync for plain messages and must not create a P5 session or otherwise change
  the original message security level. Canonical conversation IDs are
  presentation/storage routing aliases, not wire identities: when an
  authoritative Direct history page is projected through a stable
  conversation ID, Core still persists the immutable wire identity as
  `direct + peer DID`. It must not reinterpret that presentation thread as a
  `thread` wire identity or relax conflict detection to make the merge pass.

`messages.sync_conversation_after()` / Dart `client.messages.syncConversationAfter(...)` is the conversationId-first catch-up API for AWiki Me and the Flutter SDK display chain. It resolves `ConversationReadRef.conversation_id` to the syncable storage thread/ref, uses `after_server_seq`, and does not read or advance the account-level checkpoint. `messages.sync_thread_after()` / Dart `client.messages.syncThreadAfter(...)` remains a legacy / debug adapter. Both blocking and async Core paths call the account-authorized, plain-only service `sync.thread_after` method with exactly `thread_key`, `after_server_seq`, and `limit` in the request body. Direct uses the durable owner-scoped `sync_thread_bindings` conversation reference and fails closed when no authoritative binding exists; it never substitutes a peer DID. Group uses the Group DID as its thread key. Implementations must not return a locally merged `history_async` page as a catch-up result and must strictly filter `server_seq > after_server_seq`.

Realtime notification parsing may expose a readonly `RealtimeSyncHint` from the
top-level WebSocket `sync` member. The hint is scheduling metadata for
duplicate/gap/dirty detection and for deciding when to call `sync_delta`.
Realtime projection is allowed to keep the UI fresh, but receiving a realtime
hint or applying a realtime projection does not advance the reliable checkpoint.
If a realtime incoming message cannot be projected or its local SQLite write
fails, it must not emit an authoritative conversation/timeline patch. Identity-
unresolved realtime messages are durably backlogged by the same canonical
ingress used for remote history and are replayed only after verified Persona
projection; the next reliable sync or repair path remains responsible for
convergence. When the Handle authority lookup succeeds, Persona projection and
the inbound message commit happen in that order in the same local-state actor
sequence, so a first inbound Direct becomes patch-visible under its canonical
Persona conversation without briefly materializing a DID conversation.

Schema version 20 adds `sync_state` with owner-scoped checkpoint rows:

- key: `(owner_identity_id, scope, checkpoint_kind)`;
- value: decimal string `event_seq`, plus `owner_did`, `updated_at`, and optional
  `metadata_json`;
- index: `idx_sync_state_owner_kind(owner_identity_id, checkpoint_kind,
  updated_at DESC)`.

`sync_state` is private local recovery state. Diagnostics should report counts,
durations, redacted owner/thread identifiers, and checkpoint age rather than raw
message payloads or sensitive E2EE material.

Schema 32 freezes the next reliable-sync persistence boundary without changing
the current `sync_delta()` wire behavior:

- `identity_account_bindings` stores the exact six-part vNext binding and
  monotonic generation fences.
- `message_sync_state` is a separate v2 cursor row bound to the exact account,
  protocol device, and device authorization generation. No row or cursor is
  invented before an explicit bootstrap.
- `sync_applied_events` stores idempotency receipts with bounded pruning that
  retains at least 10,000 recent receipts per owner and protects the active
  recovery window.
- `sync_recovery_state` stores restart-safe recovery metadata and hashes only;
  raw recovery tokens are forbidden.
- `local_mutation_outbox` initially admits only
  `read_state_mark_read`. Message edit, recall, delete, tombstone, and generic
  message-send mutations are not part of this phase.

Schema 33 adds the per-owner `sync_installation_state` row used to persist the
opaque bootstrap `client_instance_id`. A true schema-32 database upgrades
atomically to 33 before bootstrap; keeping this as an explicit version boundary
also makes older schema-32 binaries reject the newer database instead of
silently treating it as downgrade-compatible.

Schema 34 completes the ordinary read/recovery boundary. Owner-scoped
`sync_thread_bindings` maps the service's opaque Direct `conversation_ref` or
Group thread key to exactly one canonical local conversation; Core never guesses
that mapping from a DID. `sync_remote_read_states` is a durable unresolved
Direct read-state backlog. Snapshot/delta may advance after transactionally
storing a current read state whose recent message was outside the 48-hour/500
message window; a later ordinary message binding replays and removes that
backlog in the same transaction. `thread_read_state.remote_state_version`
provides monotonic stale/conflict rejection.

`syncNow` closes compact recovery inside one call:
delta (or existing-device bootstrap recovery) → process-local opaque token →
snapshot validation/atomic merge → post-anchor delta. Snapshot application
merges current read/Group state and recent ordinary messages without deleting
older local messages, commits receipts/projections/cursor/recovery completion in
one SQLite transaction, and returns only the existing high-level `changed` or
`idle` terminal outcome after the post-anchor delta succeeds. A raw token,
cursor, cutoff, policy limit, or returned snapshot count never crosses the Rust
public, Dart, Flutter, CLI, or App boundary and is never persisted. Startup
changes an interrupted recovery to `retryable` while retaining the original
cursor, so the next `syncNow` obtains a fresh process-local token.

Core serializes `syncNow` per `owner_identity_id`. Snapshot commit additionally
uses a SQLite compare-and-swap fence over the exact previous epoch/cursor,
recovery-id hash, authorized anchor, and `applying` phase; a stale or concurrent
workflow cannot replace a newer cursor. Snapshot parsing is closed-schema and
rejects unknown top-level/policy/exclusion/read/Group fields, duplicate event
IDs or sequences, messages before the server cutoff, and malformed state
timestamps. Core does not calculate or widen the 48-hour/500-message policy.

The existing `sync_state` table remains the active checkpoint for the v1
`sync.delta` compatibility implementation; v2 `syncNow` uses
`message_sync_state`. Schemas 28, 29, 30, and 31 migrate atomically through 32
to 34; a true schema-32 database migrates through the dedicated 32-to-33 and
33-to-34 steps. Schema 27 remains owned exclusively by the explicit pre-open
canonical upgrade gate. Startup recovery changes interrupted recovery/apply and
in-flight read mutations to retryable state without advancing a cursor.

`sync.delta` is an authenticated exact-device projection over an owner-global
sequence. The service may omit sibling-targeted or expired rows while advancing
`next_event_seq`; Core therefore accepts strictly increasing visible sequences
with gaps and empty advancing pages, counts only visible applied events, and
commits the returned scan checkpoint atomically with those projections.

`messages.sync_diagnostics()` is the product-safe observability boundary for
this runtime. It exposes only the last successful sync time, a typed
`uninitialized|idle|recovering|retryable|blocked` mode, pending read-mutation
count, typed dirty domains, and typed retry state/next retry time. It does not
expose the account/device binding, stream epoch, raw scan cursor, recovery
anchor/token/hash, event/message payload, or message content. Developer tooling
that needs deeper inspection must remain a separately controlled internal
surface and may use only redacted account hashes and aggregate lag/counts.

Successful delta and snapshot commits schedule bounded best-effort local
cleanup. Cleanup failure never reverses a successful sync result. Applied-event
receipts are removed only before the safe cursor, outside the active recovery
window, while retaining at least the newest 10,000 receipts per owner.
`local_mutation_outbox` and terminal recovery rows have a seven-day audited
retention window and a maximum 256-row cleanup batch: pending, in-flight, and
retryable mutations are never deleted, and a recovery row is deleted only when
it is terminal, expired, and its anchor is covered by the current cursor.
Cleanup never writes the cursor, epoch, retention floor, or committed
projection.

Conversation and timeline patch watchers subscribe to the broadcast channel
before reading their initial committed seed. Initial seed versions fence queued
patches: a commit during seed construction is either represented by the newer
seed or delivered once afterward, but is never lost or replayed as an older
duplicate. The public committed patch envelope and patch variants are
unchanged.

## 15. System Notification Projection

Exact-device System Notification ingress is separated before ordinary Direct chat projection.
Core accepts only delivery rows/hints carrying the trusted server-side
`system_notification`/`system.notification` marker; a payload type alone never grants the system
route. Exact-device routing is Message Service storage/delivery metadata and authenticated Inbox
scope; it is not a P3 field and must not add `device_id`, `recipient_device_id`, or another
device-targeting extension to P3 `meta`. P3 keeps the standard agent-DID target only. Full
deliveries are verified against the target user's freshly resolved, root-bound DID
Document and its unique compatible `ANPMessageService.serviceDid`. That service DID anchors only
the trusted Home Service domain. `meta.sender_did` must instead use the reserved independent
`did:wba:<home-domain>:agents:system-notification:e1_*` Business Origin Agent path; Core resolves
that exact DID, verifies its E1-bound DID Document proof, and verifies its RFC 9421 Origin Proof.
Join Request self-proof and the closed type-specific payload are verified separately.

Schema version 29 stores an event receipt and one current reducer projection per
`(owner_identity_id, owner_did, did, join_session_id)`. The reducer uses
`none/revision=0`, ignores older revisions, treats identical same-revision content as a no-op,
rejects same-revision conflicts, and never reopens a terminal state. Terminal tombstones carry a
minimum 30-day retention boundary. The durable verified business payload is private Core state for
the Join orchestrator; the public snapshot and change stream remain secret-free.

`system.notification` sync events advance only the reliable account checkpoint and schedule
exact-device Inbox hydration. Neither sync hints nor full notifications produce message,
conversation, history, search, unread/read-watermark, attachment, or chat realtime projection.

## 16. Conversation Read State

Conversation-level read state is separate from reliable sync checkpoints:

- `messages.mark_conversation_read()` / Dart `client.messages.markConversationRead(...)` accepts `ConversationReadRef` and an optional `ReadWatermark`; this is the AWiki Me / Flutter SDK display-chain read ack path.
- `messages.mark_thread_read()` / Dart `client.messages.markThreadRead(...)` remains a compatibility adapter for CLI / legacy `ThreadRef` callers.
- If no watermark is provided, `im-core` computes the highest visible committed thread-local sequence from local projection / thread store.
- Direct read watermarks use direct thread-local `server_seq`.
- Group read watermarks use the group thread view `server_seq`; the service may map it from group host `group_event_seq`, but public SDK/API callers do not submit `read_up_to_group_event_seq`.
- Local truth lives in `thread_read_state`; `conversation_summaries` caches unread/read display projection but is not the only source of truth.
- `MarkThreadReadResult.effective_watermark` reports the locally committed watermark. Callers may treat `pending_remote_ack=true` as local-first success only when that effective watermark covers their target; remote acknowledgement is an independent convergence state.
- Remote ack uses `message-service` `read_state.mark_read` with profile `anp.read_state.local.v1`.
  The wire thread is resolved by `im-core` to direct / group; raw canonical storage
  `conversation_id` values are never serialized as `kind: "thread"`. Legacy direct
  `inbox.mark_read(message_ids)` remains only as fallback for unsupported services.
- A v2 read outbox operation is claimed before transport send. Its operation ID
  and payload remain immutable while in flight; a higher watermark creates a
  blocked successor. Core clears `pending_remote_ack` only after a closed-schema
  response echoes the exact DID/thread, reports a non-partial final remote ack,
  and returns a server watermark at least as high as the sent watermark. Every
  transport, decode, validation, or local-commit failure returns the claim to
  `retryable`.
- `message.read_state_updated` is a required known v2 event. `thread_kind` is
  mandatory and is never inferred from a thread key. A read-only delta or
  snapshot emits a committed conversation/thread invalidation after the read
  projection transaction succeeds.
