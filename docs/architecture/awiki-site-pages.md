# awiki-cli Site Pages

## Summary

`awiki-cli site` is the CLI surface for **tenant bare-domain site pages**.

It is intentionally separate from the existing `awiki-cli page` commands:

- `page` = handle-level content pages bound to the active identity's handle
- `site` = tenant bare-domain pages bound to one hosted domain such as `xianglianggongshi.cn`

The CLI never infers the tenant domain from the current DID or handle. Every `site` command requires `--domain`.

## Command Tree

```bash
awiki-cli site root get --domain xianglianggongshi.cn
awiki-cli site root set --domain xianglianggongshi.cn --markdown-file ./root.md

awiki-cli site page list --domain xianglianggongshi.cn
awiki-cli site page get --domain xianglianggongshi.cn --slug about
awiki-cli site page create --domain xianglianggongshi.cn --slug about --markdown-file ./about.md
awiki-cli site page update --domain xianglianggongshi.cn --slug about --markdown-file ./about-v2.md
awiki-cli site page rename --domain xianglianggongshi.cn --slug about --to intro
awiki-cli site page delete --domain xianglianggongshi.cn --slug intro
```

## Public Contract

- Tenant root page maps to `GET /`
- Tenant page maps to `GET /pages/{slug}.md`
- Returned content is raw Markdown only
- `site root set`, `site page create`, and `site page update` require exactly one content source: `--markdown` or `--markdown-file`

## Auth and Errors

- Management calls go through `POST /site/rpc`
- Authentication follows the normal DID auth flow used by `awiki-cli`
- A `403` or RPC code `-32001` is surfaced as structured CLI `forbidden`
- Slug conflicts are surfaced as `conflict`
- Validation errors such as invalid domain, slug, or overlong body are surfaced as `invalid_argument`

## Notes

- Root pages support `get` and `set` only
- Normal site pages support `list`, `get`, `create`, `update`, `rename`, and `delete`
- Renaming a slug changes the public URL directly; the old URL is not retained
