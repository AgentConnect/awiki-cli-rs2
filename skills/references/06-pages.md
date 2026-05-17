# Pages Reference

## Purpose

Use this reference when you are handling **handle-level content-page** lifecycle tasks in `awiki-cli`, including creating pages, listing pages, reading pages, updating pages, renaming slugs, and deleting pages.

This file is a **reference**, not an entry skill. Load it only when the task clearly involves handle-level content pages, slugs, markdown publishing, or visibility changes. For tenant bare-domain pages, use `references/11-site-pages.md` instead.

## Current Status

- Status: **implemented**

## When to Use

- Create a content page
- List or read pages
- Update markdown or visibility
- Rename or delete a page slug

## Core Concepts

- **slug**: the page identifier in the CLI contract
- **title**: the display title of the page
- **markdown body**: the page content provided by `--markdown` or `--markdown-file`
- **visibility**: `public`, `draft`, or `unlisted`
- **scope**: this reference only covers pages bound to one handle; it does not cover tenant bare-domain site pages

## Decision Rules

- Need to create a new page -> `page create`
- Need a list view -> `page list`
- Need to inspect one page -> `page get`
- Need to modify the body or visibility -> `page update`
- Need to change the slug -> `page rename`
- Need to delete the page -> `page delete`

## Canonical Commands

- `awiki-cli page create --slug <slug> --title <title> [--markdown ... | --markdown-file ...] [--visibility public|draft|unlisted]`
- `awiki-cli page list`
- `awiki-cli page get --slug <slug>`
- `awiki-cli page update --slug <slug> [--title ...] [--markdown ... | --markdown-file ...] [--visibility ...]`
- `awiki-cli page rename --slug <slug> --to <new_slug>`
- `awiki-cli page delete --slug <slug>`

## Boundary

- `page` = handle-level content page.
- `site` = tenant bare-domain page with explicit `--domain`.
- Do not route tenant root or `/pages/{slug}.md` tasks to `awiki-cli page ...`.

## Common Patterns

### Dry-Run First, Then Create from a File

1. `awiki-cli page create --slug hiring --title "Hiring" --markdown-file ./hiring.md --dry-run`
2. `awiki-cli page create --slug hiring --title "Hiring" --markdown-file ./hiring.md`

### Update Only Visibility

`awiki-cli page update --slug hiring --visibility draft`

## Side Effects and Confirmation

- Require explicit confirmation:
  - `page create`
  - `page update`
  - `page rename`
  - `page delete`

## Error Handling

- The slug or body is unclear -> check `awiki-cli schema page create` or `page update`
- identity/auth problem -> confirm the current active identity and registration state
- markdown file-path problem -> confirm that the file is readable before retrying

## Implementation Notes

- The body source is mutually exclusive: either inline markdown or a markdown file
- Keep examples slug-first and avoid using service-internal identifiers

## Related References

- `02-identity.md`
- `08-debug.md`
