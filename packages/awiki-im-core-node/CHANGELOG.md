# Changelog

## 0.2.0

- Add the native API v3 local conversation timeline read for local-first Host UI.
- Add the native API v4 multi-identity facade and trusted-host Skill Agent
  Controller provisioning without exposing registration tokens to JavaScript.
- Reserve and test `stateRoot/.host` as a Host-owned namespace that SDK clear
  preserves while permission hardening rejects symlinks.

## 0.1.3

- Add the native API v2 external HTTP ANP authentication facade.
- Add opaque single-use request attempts, origin-scoped in-memory Bearer reuse,
  response token capture, and one bounded `401` signature retry.
- Add an explicit test-only literal-loopback HTTP option; production targets
  remain HTTPS-only.
- Preserve the published 0.1.2 open contract: the Node facade owns its private
  state-root Vault key and does not require host key material.
