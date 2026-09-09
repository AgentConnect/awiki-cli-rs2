# Upgrade Reference

## Purpose

Use this reference when you are handling version-upgrade tasks in `awiki-cli`, including upgrading the CLI, refreshing Awiki Skills, and restarting the listener when needed.

This file is a **reference**, not an entry skill. Load it only when the task clearly involves upgrade, update, npm upgrades, outdated versions, or skill refresh.

## When to Use

- The user wants to upgrade `awiki-cli`
- The CLI indicates that a new version is available
- The CLI indicates that the current version is lower than the minimum supported version
- The user wants to refresh Awiki Skills in the current Agent

## Upgrade `awiki-cli`

Recommended path:

```bash
awiki-cli upgrade
```

This command reads the current tenant's version policy. When an upgrade is needed,
it validates the declared package and installs the tenant's recommended target.
Tenant switching changes the policy and cache used for subsequent checks.

To inspect the decision without installing anything:

```bash
awiki-cli --dry-run upgrade --format json
```

Versions use SemVer release precedence: `2.1.0-rc.7 < 2.1.0-rc.8 < 2.1.0`.
Build metadata such as `+build.7` does not change precedence. Equality with the
minimum is allowed. A lower version blocks guarded commands while keeping help,
version, upgrade and tenant switching available. Development builds intentionally
do not enforce the minimum; test this gate with a versioned binary.

If you want to run the global npm upgrade directly, use:

```bash
npm install -g {{AWIKI_CLI_CHANNEL_BASE_URL}}/awiki-cli.tgz
```

After the upgrade is complete, open a new shell and run:

```bash
awiki-cli version
```

## Refresh Awiki Skills

Upgrading the CLI does not automatically refresh the Awiki Skills already installed in the current Agent. To refresh the skill, run the installation command again.

```bash
npx skills add {{AWIKI_CLI_CHANNEL_BASE_URL}} --agent <your-agent-id> -y -g
```

If you are unsure about the value of `--agent`, make sure to go back to `00-installation.md` and check the table.

## Verification

First confirm the current version:

```bash
awiki-cli version
```

If you are currently using the websocket listener, also run:

```bash
awiki-cli runtime listener restart
awiki-cli runtime listener status
```

## Related References

- `00-installation.md`
- `05-runtime.md`
