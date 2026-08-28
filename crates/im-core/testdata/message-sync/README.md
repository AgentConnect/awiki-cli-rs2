# AWiki Message Sync test fixture mirror

`lane-handoff-fixtures.json` is a test-only byte-for-byte mirror of the authoritative fixture at:

```text
message-service/docs/api/message-sync/fixtures/lane-handoff-fixtures.json
```

The mirror keeps the standalone `awiki-cli-rs2` CI independent from a sibling Message Service
checkout. It is not a second protocol source of truth. The AWiki System Test ownership gate compares
both files byte-for-byte and fails if they diverge.
