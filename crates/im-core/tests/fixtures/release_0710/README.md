# release/0710 canonical-upgrade fixture

This directory contains the redacted schema-27 input used by the Canonical
Conversation migration gate. It is not synthesized from schema 28.

## Provenance

- Released component: `awiki-deamon` 0.1.76
- Exact source ref: `d7c853a986a29e0c0457284a6b2c3d81ec637e10`
- Released Linux artifact SHA-256:
  `3134862f360acb73ca61867fe7d547f4ecd100369ba2bd4153d724251b45ce95`
- Observed schema version: 27
- Observed schema fingerprint:
  `sha256:0b8b6b902f8460ff1ea6c122d6b8b687722890136d9b7adb6e52d9d636ef6690`

The artifact and schema fingerprint were read from the daemon actually running
for `awiki.info` on 2026-07-14. The fixture itself was created with that
artifact's `init-state` command in an isolated temporary state root and then
populated only with deterministic synthetic rows. No live database, identity,
message, credential, key, token, or phone data was copied.

`manifest.json` records the generator checksum, artifact provenance, fixture
checksum, row counts, and one-way conservation fingerprints.

## Regeneration

Run on a Linux host that can execute the exact released artifact:

```bash
python3 scripts/generate_release_0710_fixture.py \
  --daemon-binary /path/to/released/0.1.76/awiki-deamon \
  --source-ref d7c853a986a29e0c0457284a6b2c3d81ec637e10 \
  --artifact-version 0.1.76 \
  --output-dir crates/im-core/tests/fixtures/release_0710
```

Regeneration is valid only when the artifact checksum still matches the value
above. A different production schema must be reviewed and added as an explicit
supported fingerprint; the runtime detector must never accept arbitrary
schema-27 lookalikes.

The macOS AWiki Me release artifact is not represented by this Core fixture.
Its exact released checksum remains mandatory evidence for the App-to-App
`remote-upgrade-compat` gate.
