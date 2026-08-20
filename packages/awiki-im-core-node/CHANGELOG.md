# Changelog

## 0.1.5

- Add `completeRegistrationWithOutcome` so an existing Handle remains a structured, secret-free continuation instead of being collapsed into an error.
- Add prepared registration Join activation so a Host can reuse the verified registration factor after explicit confirmation instead of sending another OTP.
- Add bounded realtime synchronization signals with explicit stream-close reconciliation.
- Add the identity-bound on-demand mail account, inbox, read, mark-read, and send facade.
- Add high-level durable group-rebind recovery and stable `group_not_member` / `group_identity_stale`
  error codes so product hosts can recover old groups after a Handle changes without parsing service text.
- Interpret timezone-less mail service timestamps as UTC before exposing canonical RFC 3339 values.
- Raise the native API contract to v8 so wrappers reject binaries missing prepared registration Join, recovery, Profile, group, realtime, mail, or mention methods.
- Run N-API futures on an addon-owned Tokio runtime with an 8 MiB worker stack and keep Payload sends behind the same boxed Core-future boundary as text and attachment sends.

## 0.1.4

- Add the local conversation timeline read for local-first Host UI.
- Add `createGroup` for private, open-join, transport-protected groups and return
  the Core-derived canonical conversation ID.
- Add `addGroupMember` with Core-owned Handle/DID resolution.
- Add local-only `hydrateDisplayProfiles` batch lookup for cached sender Handle
  and display-name projection without a Directory network request.
- Raise the native API contract to v4 for group-management, display-profile, and local timeline methods.

## 0.1.3

- Add the native API v2 external HTTP ANP authentication facade.
- Add opaque single-use request attempts, origin-scoped in-memory Bearer reuse,
  response token capture, and one bounded `401` signature retry.
- Add an explicit test-only literal-loopback HTTP option; production targets
  remain HTTPS-only.
- Preserve the published 0.1.2 open contract: the Node facade owns its private
  state-root Vault key and does not require host key material.
