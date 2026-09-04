# release/0714 Core schema-36 fixture

This directory contains the locked, offline input for the schema-36 Core
migration gate. It was generated independently in this public repository from
the historical AWiki source; no file was copied from `awiki-system-test`.

The historical `awiki-deamon` initializes an empty schema-36 database in an
isolated temporary state root. The public generator then inserts only fixed
synthetic identifiers and message content under `.fixture.invalid`. It never
reads a live database, identity, message, credential, key, token, or service
response.

## Provenance

- AWiki source ref: `e2cf7f4cd00debba5353980e6d33c3ba682cdd0c`
- AWiki daemon version: `0.1.91`
- ANP source ref: `59475cf76b23838a911a7263287ce6b7399d8e02`
- ANP version: `0.9.3`
- Source schema: `36`
- Schema fingerprint:
  `sha256:72822725c300c3aa03c436a667351c9d653abe2db7c78003289a1b410d3fd9aa`
- Fixture SHA-256:
  `3f1b1ad19e9f7057bb98f413811038cb99205343c0d054950dcc0e1e2acbe4e0`

`manifest.json` additionally locks the generator checksum, locally built
historical daemon checksum, row-count oracles, and pre-migration conservation
digest.

## Regeneration

Build the daemon from the exact AWiki and ANP refs above, then run:

```bash
python3 scripts/generate_release_0714_core_fixture.py \
  --daemon-binary /path/to/historical/awiki-deamon \
  --source-ref e2cf7f4cd00debba5353980e6d33c3ba682cdd0c \
  --artifact-version 0.1.91 \
  --output-dir /path/to/new-empty-output-directory
```

The checked-in fixture is immutable test evidence. Regeneration must target a
new directory and requires explicit review of every checksum and oracle change.
