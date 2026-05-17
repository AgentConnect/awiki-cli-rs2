# awiki-cli Direct E2EE operations

## Status

- P5 secure direct messaging is implemented for CLI local validation and system-test focused runs.
- Harness map: [Direct E2EE cross-repo feature map](../../../awiki-harness/features/direct-e2ee.md).
- SDK authority: [ANP SDK Direct E2EE P5](../../../anp/anp/docs/e2e/direct-e2ee-p5-sdk.md).
- Service API: [message-service Direct API](../../../message-service/docs/api/ANP-client-server-api-direct.md).
- Public `ANPMessageService` discovery remains intentionally narrower than implementation and does not advertise `anp.direct.e2ee.v1` / `direct-e2ee` until separately approved.

## CLI responsibility

`awiki-cli` owns the user/agent orchestration around the ANP Go SDK direct E2EE engine:

- `msg send --secure on` for P5 direct init/cipher sends.
- `msg inbox` / `msg history` decrypt of P5 init/cipher messages when local state is available.
- runtime listener inbound decrypt and local notification normalization.
- `msg secure status`, `init`, `repair`, `failed`, `retry`, `drop` diagnostics and recovery.
- local identity-scoped stores for sessions, signed prekeys, OPKs, and pending/outbox records.

The CLI does not implement P5 cryptographic algorithms independently; it consumes `github.com/agent-network-protocol/anp/golang/direct_e2ee` through `internal/anpsdk`.

## Local state layout

Per identity, the Go SDK reference stores live under the identity directory:

```text
identities/<identity-id>/p5-e2ee-sessions/
identities/<identity-id>/p5-signed-prekeys/
identities/<identity-id>/p5-one-time-prekeys/
```

The CLI business SQLite additionally stores message indexes, local plaintext views, and E2EE outbox metadata. It must not expose or log root keys, chain keys, skipped message keys, nonces, private ratchet keys, OPK private material, or JWTs.

## Main secure direct flow

### First secure message

1. Sender calls `direct.e2ee.get_prekey_bundle` for the recipient.
2. Go SDK verifies the recipient stable bundle and optional OPK sidecar.
3. CLI sends `direct.send` with:
   - `meta.profile=anp.direct.e2ee.v1`;
   - `meta.security_profile=direct-e2ee`;
   - `meta.content_type=application/anp-direct-init+json`;
   - `meta.operation_id == meta.message_id`;
   - no `params.auth` in current P5 phase-1 behavior.
4. Sender local session enters pending-confirmation.

### Recipient decrypt and first reply

1. Recipient inbox/history/listener sees `application/anp-direct-init+json`.
2. CLI processes init through the Go SDK, persists session state, and presents decrypted plaintext locally.
3. Recipient reply with `--secure on` sends `application/anp-direct-cipher+json`.
4. Sender processes the first valid reply and moves to established state.

### Follow-up messages

- Established sessions send direct ciphers with P5 ratchet headers.
- `history` and listener decrypt ciphers into local plaintext views.
- Replay, tamper, and skip-window behavior is delegated to SDK state.

## Command surface

```bash
awiki-cli msg send --to DID --text "..." --secure on
awiki-cli msg inbox --scope direct --with DID
awiki-cli msg history --with DID

awiki-cli msg secure status --with DID
awiki-cli msg secure init --with DID
awiki-cli msg secure repair --with DID
awiki-cli msg secure failed
awiki-cli msg secure retry OUTBOX_ID
awiki-cli msg secure drop OUTBOX_ID
```

`status` and error outputs should be useful for repair while redacting private cryptographic material.

## Discovery and DID document posture

`awiki-cli` can operate secure direct locally, but generated DID documents currently keep the public `ANPMessageService` profile list conservative. See [`anp-service-discovery.md`](anp-service-discovery.md): direct E2EE is not advertised as public interop capability until discovery policy changes.

## Validation

Focused CLI checks:

```bash
cd awiki-cli
go test ./internal/anpsdk ./internal/message ./internal/store ./internal/runtime/... -count=1
```

Cross-service evidence is documented in [Direct E2EE system tests](../../../awiki-system-test/docs/direct-e2ee-system-tests.md).

## Non-goals

- Public discovery enablement.
- Group E2EE / MLS.
- Multi-device direct E2EE protocol semantics.
- Service-side plaintext decrypt.
- Compatibility with old HPKE service wire objects.
