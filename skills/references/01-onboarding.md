# Onboarding Reference

## Purpose

Use this reference when you are handling first-time usable setup in `awiki-cli`, including local status inspection, identity creation or recovery, and basic end-state checks.

This file is a **workflow reference**, not an entry skill. Load it only when the task clearly involves first-time setup, migration from v1, registration, recovery, or an initial status check.

CLI installation, Awiki Skills installation, and workspace initialization are in `00-installation.md`. This file starts after those prerequisites are already complete.

WebSocket listener initialization, OpenClaw host-notification configuration, and current heartbeat limitations in HTTP mode are also documented in `00-installation.md`.

## Recommended Path After First Installation

This document covers the second half of "getting it usable". After the prerequisite installation is complete, proceed in the following order:

1. First run `awiki-cli id status --format json` to confirm whether the current state is no identity, local-only, or already has a handle-backed identity
2. Based on the result, either register a new handle or recover an existing handle
3. After that, run `awiki-cli status --format json` and `awiki-cli runtime status --format json` to confirm that identity and runtime have both entered a usable state
4. Send your handle to your friends; if you need to verify profile or identity state, check `02-identity.md`
5. Start message collaboration:
   - Direct messages and attachment send/receive: see `03-messaging.md`
   - Creating groups and multi-party collaboration: see `04-groups.md`

Boundary rules:

- If the problem is still about installation, `PATH`, workspace, or runtime initialization, go back to `00-installation.md`
- If installation and runtime preparation are already complete, and you are now about to start registration, recovery, or first use, stay in this file

## Current Status

- Status: **implemented workflow**
- Conceptually depends on:
  - identity
  - runtime
  - messaging
- Installation details are intentionally split into `00-installation.md`

## When to Use

- First `awiki-cli` setup after installation
- Migrating local identities or SQLite data from v1
- Registering a new handle-backed identity
- Recovering an existing handle
- Running one overall status check after registration

## Prerequisites

- `awiki-cli` is installed and executable
- The workspace has been initialized; otherwise, handle that first through `00-installation.md`
- If registration is needed, the user can provide a phone number or email address
- Before executing write operations, the user has explicitly approved identity creation and identity recovery

---

## Step 1: Check the Current Identity State

```bash
awiki-cli id status --format json
```

Common cases:

- No default identity: the summary contains something like "No default identity is configured"
- A local identity exists but user registration is not complete: it indicates that the current identity is still local-only
- A handle-backed identity already exists: you can skip registration and go directly to runtime initialization

To view all local identities:

```bash
awiki-cli id list --format json
```

If you are migrating from v1:

```bash
awiki-cli --migration id import-v1 --all --dry-run
```

---

## Step 2: Register the First Usable Identity

The goal of this section is to prepare a **handle-backed identity that can send and receive messages normally** for the current workspace.

- If you have already registered an awiki account on another device or in another environment and still remember your handle and bound phone number, prefer the "recover handle" path
- If this is your first awiki account registration, create a new handle using the phone-number or email path below

### 2.1 Choose a Registration Method: Phone First, Email Second

You first need to decide whether to register the handle with a **phone number** or an **email address**:

- Phone-number registration is the default recommendation
- If it is inconvenient to receive SMS verification codes in the current environment, fall back to email registration

#### Register with a Phone Number (Recommended Path)

Step 1: send a verification code to the phone number

```bash
awiki-cli id register \
  --handle your-handle \
  --phone +8613800138000 \
  --format json
```

Step 2: after receiving the SMS verification code, complete registration with the code

```bash
awiki-cli id register \
  --handle your-handle \
  --phone +8613800138000 \
  --otp 123456 \
  --format json
```

Behavior notes (simplified):

- The first step sends a one-time verification code to the specified phone number
- The second step verifies the code and completes the registration flow
- After registration succeeds, the CLI will:
  - Generate a local DID identity and key material
  - Complete handle registration in the backend
  - Write the JWT and other credentials into the local workspace

#### Register with an Email Address

If you cannot use a phone number, or prefer email registration, you can use:

```bash
awiki-cli id register \
  --handle your-handle \
  --email you@example.com \
  --wait \
  --format json
```

Behavior notes (simplified):

- If the email address is not yet verified, the CLI sends an activation email to that address
- `--wait` means the CLI polls the email-verification state until verification succeeds or times out
- After verification succeeds, the CLI will:
  - Generate a local DID identity and key material
  - Complete handle registration in the backend
  - Write the JWT and other credentials into the local workspace

After registration completes, run this again:

```bash
awiki-cli id status --format json
```

Expected result:

- A default identity exists
- The state has changed from local-only to a state usable for message send/receive

### 2.2 Existing Account Users: Recover a Handle (Optional)

If you already have an awiki account, and you still remember your handle and bound phone number, you can recover this identity through the recovery command instead of registering again:

```bash
awiki-cli id recover \
  --handle your-handle \
  --phone +8613800138000 \
  --otp 123456 \
  --format json
```

The recovery flow will:

- Verify the phone number and one-time verification code through the backend
- Regenerate the local DID identity and bind it to the handle
- Write credentials back into local storage

If needed, you can also continue with:

- `awiki-cli id bind ...`
- `awiki-cli id profile set ...`

For more detailed identity notes, see `02-identity.md`.

---

## Step 3: Run One Overall Status Check

After completing all previous steps, it is recommended to run one overall status check to confirm the baseline state of the CLI, identity, and runtime:

```bash
awiki-cli status --format json
awiki-cli runtime status --format json
```

Recommended meaning:

- `awiki-cli status`: inspect the overall state of the current workspace path, configuration source, and local identity storage
- `awiki-cli runtime status`: inspect the current runtime mode (`http`/`websocket`) and listener state

Both commands are read-only and are a good closing check for the first-use flow.

If you already completed runtime initialization in `00-installation.md`, this step is mainly to confirm that identity and runtime have both entered a usable state together.

---

## What Can You Do After Registration?

At this point, the key steps required for first use are complete:

- `awiki-cli` is installed correctly
- Awiki Skills are ready
- The workspace has been initialized
- At least one handle-backed identity exists
- The runtime mode is explicit, and initialization was already attempted during installation

The next two common paths are:

1. **Send your handle to your friends**
   - After registration, share your handle with your friends so they can message you through the handle
   - If you need to verify profile or identity state, see `02-identity.md`
2. **Start message collaboration**
   - Direct messages and attachment send/receive: see `03-messaging.md`
   - Creating groups and multi-party collaboration: see `04-groups.md`

## Security Notes

- Do not silently create, register, or recover an identity
- Before executing identity-related write operations, prefer a dry-run or a status check first
- Do not send real messages when no target has been provided by the user or is otherwise known

## Related References

- `00-installation.md`
- `02-identity.md`
- `03-messaging.md`
- `04-groups.md`
- `05-runtime.md`
- `08-debug.md`
