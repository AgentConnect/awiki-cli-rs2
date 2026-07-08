# awiki-cli Direct E2EE Operations

## Status

Direct E2EE is a supported product capability through high-level `im-core` services and CLI flags. Public `ANPMessageService` discovery remains intentionally narrower than implementation and does not advertise `anp.direct.e2ee.v1` / `direct-e2ee` until separately approved.

References:

- SDK boundary: `docs/architecture/im-core-sdk-architecture.md`
- Service API: `../message-service/docs/api/ANP-client-server-api-direct.md`
- Discovery posture: `docs/architecture/anp-service-discovery.md`

## Responsibility Boundary

`im-core` owns secure direct business orchestration:

- prekey bundle lookup and validation.
- direct init/cipher send.
- inbox/history/listener decrypt when local state is available.
- identity-scoped session, signed prekey, OPK and pending/outbox state.
- secure status, prepare, repair and retry domain results.

Current direct E2EE runtime state is owned by `im-core` local state and keyed by
`owner_identity_id`. `owner_did` is only the current DID snapshot and must not be
used as a runtime owner fallback.

`awiki-cli` owns product shell behavior:

- parsing `--secure required` / secure command flags.
- building `ImCore` / `ImClient`.
- rendering status, warnings, dry-run plans and errors.
- protecting stdout/stderr from raw cryptographic material.
- running listener/service infrastructure.

The CLI must not independently implement ratchet/session algorithms or expose raw secure artifacts.

Historical Go SDK reference stores may exist under the identity directory, but active runtime state is stored through `im-core` local state rather than using those paths as owner identity.

The CLI business SQLite additionally stores message indexes, local plaintext views,
and E2EE outbox metadata. Active rows use `owner_identity_id` keys, including
`e2ee_outbox(owner_identity_id, outbox_id)` and direct E2EE tables. It must not
expose or log root keys, chain keys, skipped message keys, nonces, private
ratchet keys, OPK private material, plaintext outbox payloads, raw SQLite rows,
backup contents, or JWTs.

## Local State

Direct E2EE state is identity-scoped. Private session/prekey material and pending secure delivery state are owned by `im-core` internals and must not be printed, logged, or serialized into CLI output.

The CLI business SQLite may store high-level message indexes, local plaintext views, delivery summaries and outbox summaries. It must not expose or log:

- root keys, chain keys, skipped message keys.
- nonces or private ratchet keys.
- OPK private material.
- plaintext outbox payloads, raw SQLite rows or backup contents.
- JWTs or private identity keys.
- raw secure wire payloads beyond explicit diagnostic gates.

## Main Flow

### First secure message

1. Sender resolves the peer and fetches a prekey bundle.
2. SDK verifies the recipient bundle and optional OPK sidecar.
3. SDK sends `direct.send` with secure init content.
4. Sender local session enters pending-confirmation state.

### Recipient decrypt and first reply

1. Recipient inbox/history/listener sees secure init content.
2. SDK processes init, persists session state and returns a safe plaintext view.
3. Recipient reply with secure required sends a secure cipher message.
4. Sender processes the first valid reply and marks the session established.

### Follow-up messages

- Established sessions send direct cipher messages.
- History and listener decrypt ciphers into local plaintext views.
- Replay, tamper and skip-window behavior remains SDK-owned.

## Command Surface

```bash
awiki-cli msg send --to DID --text "..." --secure required
awiki-cli msg inbox --scope direct --with DID
awiki-cli msg history --with DID

awiki-cli msg secure status --with DID
awiki-cli msg secure init --with DID
awiki-cli msg secure repair --with DID
awiki-cli msg secure failed
awiki-cli msg secure retry OUTBOX_ID
awiki-cli msg secure drop OUTBOX_ID
```

User-facing output should help repair sessions while redacting private cryptographic material.

## Discovery Posture

Generated DID documents keep public profile lists conservative. Direct E2EE may be implemented locally without advertising it as a public interop capability. Public discovery enablement must be approved separately and reflected in `docs/architecture/anp-service-discovery.md`.

## Validation

Focused local checks should cover:

```bash
cargo test -p im-core --locked secure
cargo test -p awiki-cli --locked msg_secure
cargo test -p awiki-cli --locked direct
```

System-level validation belongs in the cross-repo system test suite, not in long-lived `docs/` verification transcripts.

## Non-goals

- Public discovery enablement.
- Group E2EE / MLS semantics.
- Multi-device direct E2EE protocol expansion.
- Service-side plaintext decrypt.
- Compatibility with obsolete HPKE wire objects.
