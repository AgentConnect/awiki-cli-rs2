# Site Pages Reference

## Purpose

Use this reference when the task is about **tenant bare-domain site pages** in `awiki-cli`.

This covers:

- the tenant root page at `GET /`
- tenant pages at `GET /pages/{slug}.md`
- admin operations through `awiki-cli site ...`

This file is a **reference**, not an entry skill.

## Boundary

- `page` = handle-level content pages for one identity's handle
- `site` = tenant bare-domain pages for one hosted domain
- If the user mentions a domain root, `/pages/*.md`, or a tenant admin flow, prefer `site`, not `page`

## Canonical Commands

- `awiki-cli site root get --domain <domain>`
- `awiki-cli site root set --domain <domain> [--markdown ... | --markdown-file ...]`
- `awiki-cli site page list --domain <domain>`
- `awiki-cli site page get --domain <domain> --slug <slug>`
- `awiki-cli site page create --domain <domain> --slug <slug> [--markdown ... | --markdown-file ...]`
- `awiki-cli site page update --domain <domain> --slug <slug> [--markdown ... | --markdown-file ...]`
- `awiki-cli site page rename --domain <domain> --slug <slug> --to <new_slug>`
- `awiki-cli site page delete --domain <domain> --slug <slug>`

## Decision Rules

- Need the tenant home page -> `site root get` or `site root set`
- Need a list of tenant pages -> `site page list`
- Need one tenant page body -> `site page get`
- Need to create or replace Markdown -> `site page create` or `site page update`
- Need to change a public slug -> `site page rename`
- Need to remove a tenant page -> `site page delete`

## Body Source Rules

- `site root set`, `site page create`, and `site page update` require exactly one body source
- Use either `--markdown` or `--markdown-file`
- Passing neither is invalid
- Passing both is invalid
- To clear content, pass an explicit empty string or an empty file

## Auth Notes

- The active identity must already be a configured tenant site admin on the backend
- The CLI does not derive the tenant domain from the current DID or handle
- Always require the explicit `--domain`

## Related References

- `06-pages.md`
- `02-identity.md`
