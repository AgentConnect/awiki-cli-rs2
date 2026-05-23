# Future awiki-me Integration

This repository only provides the reusable `awiki_im_core` SDK package. It does not modify `awiki-me`.

A future `awiki-me` integration can depend on the package with:

```yaml
dependencies:
  awiki_im_core:
    path: ../awiki-cli-rs2/packages/awiki_im_core
```

Recommended migration approach:

1. Add Rust-backed account and message gateway implementations alongside the existing Dart-only gateway.
2. Gate backend selection behind a Dart define:

```dart
const backend = String.fromEnvironment(
  'AWIKI_IM_BACKEND',
  defaultValue: 'dart',
);
```

3. Keep the existing Dart backend as production fallback while migrating identity, auth, profile, messages, groups, and finally realtime.
4. Keep app-specific mapping in `awiki-me`, for example `Message -> ChatMessage` and `Conversation -> ConversationSummary`. Do not push those UI/cache DTOs into `packages/awiki_im_core`.

Suggested order:

```text
identity restore/list/default
auth status/refresh
profile read/update/public profile
send direct/group text
inbox/history/conversations/markRead
group create/join/get/list/members
relationship APIs
realtime
remove Dart-only duplicate code
```
