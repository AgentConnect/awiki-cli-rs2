# awiki-cli Group E2EE operations

## Status

- Supported Group E2EE product actions are exposed through high-level `im-core` APIs and canonical CLI flags.
- Low-level `group e2ee *` orchestration commands remain hidden/internal or stable unsupported; they are not a product contract.
- Harness map: [Group E2EE cross-repo feature map](../../../awiki-harness/features/group-e2ee.md).
- Protocol/SDK: [ANP SDK / anp-mls Group E2EE](../../../anp/anp/docs/e2e/group-e2ee-p6-anp-mls.md).
- Service API: [message-service Group API](../../../message-service/docs/api/ANP-client-server-api-group.md).
- Public discovery and service capability gates still decide whether secure operations are available for a concrete identity/workspace/service.

The device-scoped P6 v2 product path also has a host-local, default-off
`ImCoreOpenOptions.multi_device_group_e2ee_enabled` rollout gate. This setting
is not an ANP capability and is never serialized into DID or cross-domain wire
objects. Enabling it selects the v2 implementation for supported public group
lifecycle operations and the existing redacted group status/repair facade; it
does not expose low-level MLS commands.

The CLI maps the deployment-local environment variable
`AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED` to that Core option. An unset variable
enables the AWiki CLI/Daemon product path; `0` is reserved for emergency
rollback, `1` explicitly enables it, and every other value fails closed. The
reusable SDK option remains default-off. This gate is independent from Join,
Direct, root transfer, revoke and Handle Recovery and is never serialized into
ANP discovery or message payloads.

### P6 v2 lifecycle routing

When the host-local rollout gate is enabled:

- `groups().publish_key_package()` binds the operation to the current
  authenticated protocol device and its P2 `deviceManifest` entry. Omitting
  `device_id` means the current device; selecting a sibling device or the
  legacy `default` device fails closed.
- Secure `groups().create()` first creates the P4 business group. Core then
  requires the exact P4 `group_state_ref`, publishes a current-device P6 v2
  creator KeyPackage, and submits the typed P6 create operation. The typed Host
  result must match before the SDK finalizes local MLS state.
- An uncertain P6 Host result is returned as an error while the SDK keeps its
  durable operation in `prepared` for an exact later recheck. A missing local
  group remains `MissingLocalState`; status or repair must not synthesize MLS
  state or claim success. An explicit non-accepting Host validation/state
  conflict aborts the local prepare; transport failures, temporary errors and
  idempotency conflicts remain uncertain and never do.
- Public `groups().add_member()`, `groups().remove_member()`, and `groups().leave()`
  first issue a fresh `group.get` that explicitly requests policy. The
  authoritative policy, not the caller's security hint or a best-effort local
  cache, selects Base-only versus P6 behavior. Missing, malformed, or
  conflicting security classification fails closed; an E2EE group still uses
  P6 when the caller leaves the security hint at its default.
- For an E2EE Add/Remove, an active owner device with the accepted local MLS
  endpoint may run the combined P4 + P6 operation. A P4 admin is not declared
  unauthorized, but Core currently fails before P4 with a controller-required
  local-state error because no durable owner handoff exists for this ordinary
  membership path. This avoids reporting P4 success as full E2EE convergence.
  Gate-on E2EE Leave likewise fails before P4 and never enters the legacy
  lifecycle until the subsequent owner-controlled device Removes can be durably
  orchestrated.
- The P4 mutation is sent as a Base operation without legacy E2EE extension
  fields. P6 accepts the exact response `member_did` as its subject (including
  a Handle binding change during the request), verifies any embedded
  `group_state_ref.group_did`, and consumes that exact state reference.
- On initial P4 member activation, Core resolves and validates the target's
  current P2 DID Document/embedded Manifest through a fresh authoritative
  resolve with no local cache fallback, selects every currently P6-eligible
  device that is absent from the accepted local tree, obtains a fresh
  KeyPackage for each exact DID/device pair from the target DID Document's
  `ANPMessageService.serviceDid` (including the federated case), and performs
  one ordered Add/Commit per device. Each Welcome remains a Host-routed notice
  for only that target device; the public result still represents one
  DID-level business member.
- On P4 member removal, Core derives the exact target device list from the
  authenticated accepted MLS tree and performs one ordered Remove/Commit per
  leaf. It deliberately does not require a removed device to remain in the
  current Manifest, because loss of P2 eligibility is itself a Remove trigger;
  sibling DID leaves are not selected.
- V1 convergence is desired-state reconciliation rather than a second Core
  membership-operation database. The authoritative P4 active-member roster,
  each member's freshly resolved P2 Manifest, and the SDK accepted endpoint
  inventory are compared on every step. Missing endpoints are added, extra
  endpoints are removed, and the accepted tree plus fresh P4 state reference
  are reloaded after each successful Commit. The SDK WAL is the sole durable
  per-Commit crash/response-loss state.
- Repeating public `group add` for an existing P4 member is idempotent at the
  business layer and enters device reconciliation instead of creating another
  P4 member. Public `group secure repair` performs the same owner-side
  reconciliation for the selected group and returns only high-level
  added/removed/remaining device counts. This is the supported path for a new
  device of a DID that is already a group member.
- Device revocation uses an exact `(DID, device_id)` Remove primitive. It never
  removes the P4 business member or selects sibling Leaves. Full member removal
  remains a separate P4 transition followed by removal of every accepted Leaf
  for that DID. After a Step 06 Identity revoke, Core enumerates groups where
  the current DID is the active P4 owner and invokes this exact primitive when
  the current device also has the active local MLS controller Leaf. Historical
  groups not joined by this device and other Group Hosts retain their durable
  removal trigger until an owner device with local MLS state runs the same
  selected-group repair.

When the rollout gate is disabled, the existing legacy lifecycle and provider
path remains unchanged. Core must not reinterpret a legacy result as P6 v2 or
mix legacy and v2 local state in one operation.

This lifecycle slice covers owner-driven initial member Add, member Remove,
same-DID device convergence, and selected-group repair. A partially accepted
multi-device sequence remains fail-closed and is resumed from the SDK WAL; it
must not be reported as fully converged until every remaining exact-device step
has completed.

## CLI responsibility

`awiki-cli` owns argument parsing, workspace/config selection, dry-run rendering, errors, schema/help/completion, and user-facing output. Supported E2EE behavior must execute through `im-core` public services; the CLI must not orchestrate MLS or expose raw crypto artifacts.

Key boundaries:

- Group lifecycle uses `client.groups().create/add_member/remove_member/leave` with `GroupSecurityRequirement::Required`.
- Group secure state uses `client.secure().group(group).status()/repair()`.
- Group secure send uses `client.messages().send(... MessageSecurityMode::E2eeRequired ...)`.
- Sensitive plaintext and MLS private material must not be placed in argv, shell history, service requests, or service logs.
- CLI output must not include raw KeyPackage, prekey payloads, MLS notice bodies, provider stdout/stderr, session counters, ratchet counters, or raw secure outbox rows.

## Runtime and local state

The default supported path is `im-core` native group E2EE runtime/storage. Historical CLI exec-provider controls such as `AWIKI_ANP_MLS_BINARY` are not part of the default supported product path.

The CLI business store may cache group/message indexes and high-level secure summaries. Active SQLite rows use `owner_identity_id` keys. Private MLS state and secure outbox internals are owned by `im-core`; provider state/path selection remains scoped by `owner_identity_id + device_id`.

Diagnostics and user-facing output may report high-level readiness, problem codes, repair summaries, and counts. They must not report raw MLS artifacts, provider stdout/stderr, provider binary paths, provider state paths, raw SQLite rows, backup contents, or secure outbox plaintext.

## User-facing command surface

Group E2EE is reached through normal group/message commands and high-level secure status/repair commands:

```bash
awiki-cli group create --name "Secure Group" --secure required
awiki-cli group add --group GROUP_DID --member DID --secure required
awiki-cli group remove --group GROUP_DID --member DID --secure required
awiki-cli group leave --group GROUP_DID --secure required
awiki-cli msg send --group GROUP_DID --secure required --text "..."
awiki-cli group secure status --group GROUP_DID
awiki-cli group secure repair --group GROUP_DID
```

Deprecated compatibility aliases:

- `group create/add/remove/leave --e2ee` maps to `--secure required`.
- `group create --message-security-profile group-e2ee` maps to `--secure required`.
- `group e2ee status/repair` maps to `group secure status/repair`.

Blocked/internal commands:

- `group secure diagnostics` and `group secure repair --explain` are stable unsupported in this version.
- `group e2ee publish-key-package/pending/process-leave-request/recover-member/update-key/rejoin` are hidden/internal or stable unsupported and must not appear in the default surface.
- Internal device-scoped invocations obtain the unique current `protocol_device_id` from
  `id device list` and pass it explicitly as `--device`; the hidden CLI does not synthesize
  or accept the legacy `default` device authority.

## Discovery posture

Group E2EE public discovery remains disabled. Default DID/service discovery must
not advertise `anp.group.e2ee.v1` or `group-e2ee` unless a separate security
review approves an explicit enablement plan.

## Orchestration flows

### Create

1. CLI passes `GroupSecurityRequirement::Required` through the im-core group create request.
2. `im-core` submits the high-level group create request, initializes local group secure state, and submits the service secure head/bootstrap request.
3. CLI renders only the high-level group result and warnings.

### Add and welcome

1. CLI passes group, member, role, reason, and `GroupSecurityRequirement::Required`.
2. `im-core` performs directory lookup, member mutation, KeyPackage lease, commit/welcome generation, service secure mutation, notice handling, and local projection.
3. CLI renders only high-level group/member results and warnings.

### Send and receive

1. CLI sends `MessageSecurityMode::E2eeRequired` through `client.messages().send`.
2. When the P6 v2 rollout gate is enabled, Core binds `group.list_messages` history requests to
   the unique authenticated current protocol device; a caller-supplied `meta.sender_device_id`
   is only an equality assertion. This lets the Host return the
   exact-device opaque `group.incoming` envelope needed for local validation/decryption after a
   missed realtime notification; callers cannot select a sibling or legacy `default` device.
3. `im-core` handles group snapshot/state lookup, encryption, transport, incoming decrypt, MLS notice processing, and local projection.
4. CLI does not build MLS payloads or wire RPC params.

### Repair and lifecycle

- `group secure status` returns high-level secure state and redacted problems.
- `group secure repair` first reconciles the selected group's P4 member/P2
  device desired state with the accepted SDK tree, then reconciles pending SDK
  WAL work and reports secret-free readiness plus device counts.
- `group remove --secure required` and `group leave --secure required` use secure-aware lifecycle APIs; low-level process-leave/update/rejoin commands are not the supported interface.
- a self-scoped P4 `member-removed` / `member-left` event immediately records
  `terminal_pending_remove` and disables new encryption without pretending that the final MLS
  Remove Commit has been applied. A later fresh Welcome can safely replace that state when the
  accepted newer tree proves the old exact LeafNode is absent.

### Handle-backed DID recovery

1. Handle Provider recovery 完成后，`im-core` 从可靠 group projection 为该本地 identity 建立逐群 durable recovery job。
2. 新 DID 对每个 Handle-backed 群提交 P4 `group.rebind_member`；请求携带完整 Handle、previous/new DID 和严格递增的 binding generation。旧 DID 单独签名不能证明 continuity。
3. 普通群在 P4 接受后完成。E2EE 群保持发送暂停，owner 使用同一个 P4 `group_state_ref` 先执行现有 `group.e2ee.add(new DID)`，再执行现有 `group.e2ee.remove(old DID)`。
4. Add 已接受而 Remove 未完成时必须保持 durable pending，重启后只重试当前阶段；不得回滚 P4、恢复旧 DID 或调用 `group.e2ee.recover_member` 代替 DID 变更。
5. 只有匹配的 Remove accepted notice 和 MLS roster 已无旧 DID 时才解除发送暂停。新 DID 只获得当前及未来 epoch，历史明文恢复不属于该流程。

该流程没有 `group.e2ee.rebind_member` wire method。CLI/App 只消费 high-level recovery status 和 repair API，不直接编排 raw Add/Remove payload。

## Release and packaging

- `awiki-cli` must enable the `im-core/group-e2ee` feature in the default Linux/macOS build.
- The release artifact script verifies the Linux/macOS feature graph includes both `im-core` feature `group-e2ee` and `anp` feature `mls`.
- Release notes should state that low-level E2EE diagnostics are blocked/internal and that Windows E2EE package validation is not a blocker for this stage.
- Windows artifacts may still be produced by the generic release matrix, but Windows E2EE package/release validation is explicitly deferred and must not block Linux/macOS rollout.

## Validation

Focused CLI checks:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo check -p awiki-cli --locked
cargo test -p im-core --features group-e2ee --locked lifecycle_
cargo test -p awiki-cli --locked msg_secure
cargo test -p awiki-cli --locked group_secure
cargo test -p awiki-cli --locked e2ee
```

Cross-service validation is owned by [awiki-system-test Group E2EE system tests](../../../awiki-system-test/docs/group-e2ee-system-tests.md). Local CLI work does not require connecting to real domains during unit/contract validation.
