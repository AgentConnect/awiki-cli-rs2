# Tenants Reference

## Purpose

Use this reference for local CLI tenant profiles. A tenant is one atomic `backend_base_url + did_host` pair with an isolated workspace. Tenant commands select which backend and DID namespace the CLI uses; they do not manage remote site-page content.

## Current Status

- Status: **implemented**
- A default tenant is created from release metadata during first initialization when no tenant registry exists
- Additional self-hosted or non-default tenants can be created and selected explicitly

## Canonical Commands

- `awiki-cli tenant list`
- `awiki-cli tenant current`
- `awiki-cli tenant create <name> --backend-base-url <url> --did-host <domain> [--display-name <label>]`
- `awiki-cli tenant setup <name> --backend-base-url <url> --did-host <domain> [--display-name <label>]`
- `awiki-cli tenant use <name>`
- `awiki-cli tenant reconfigure <name> --backend-base-url <url> --did-host <domain>`
- `awiki-cli --tenant <name> <command>` for a one-command override without changing the persistent active tenant

## Command Boundaries

- `tenant create` creates a profile but does not activate it
- `tenant setup` is the idempotent onboarding path: it creates and activates a missing profile, or activates an existing profile only when both endpoints match exactly; it never rewrites existing endpoints
- `tenant use` activates an existing profile by name
- `tenant reconfigure` only changes a tenant that has no identities or local database data; create a new tenant when data already exists
- `awiki-cli init` initializes the currently selected tenant workspace after tenant selection
- `site ... --domain <domain>` manages remote public site content and never creates, switches, or reconfigures the local CLI tenant

Backend and DID host values belong to the tenant registry and must be managed through `tenant` commands rather than tenant `config.yaml`.

## Common Patterns

### Use the Release Default

1. `awiki-cli init`
2. `awiki-cli tenant current`

### Add a Self-Hosted Tenant

1. `awiki-cli tenant setup community --backend-base-url https://community.example.com --did-host community.example.com`
2. `awiki-cli init`
3. `awiki-cli tenant current`

### Inspect Without Persistently Switching

`awiki-cli --tenant community status`

## Side Effects and Confirmation

Safe inspection:

- `tenant list`
- `tenant current`

Require explicit confirmation:

- `tenant create`
- `tenant setup`
- `tenant use`
- `tenant reconfigure`

## Error Handling

- Endpoint/profile state is unclear -> `awiki-cli tenant list` and `awiki-cli tenant current`
- Command contract is unclear -> `awiki-cli schema tenant`
- Current workspace state is unclear after switching -> `awiki-cli status` and `awiki-cli doctor`

## Related References

- `00-installation.md`
- `01-onboarding.md`
- `11-site-pages.md`
