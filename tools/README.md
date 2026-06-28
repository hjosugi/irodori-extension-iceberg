# Connector Metadata Generation

`connector.source.json` is the human-editable source of truth for each extension.
`connector.config.json` and `irodori.extension.json` are generated packaging
artifacts kept for compatibility with the current native ABI and marketplace
layout.

## Commands

Check that generated artifacts are in sync:

```sh
python3 tools/generate_connector_metadata.py generate --check
```

Regenerate artifacts after editing one or more `connector.source.json` files:

```sh
python3 tools/generate_connector_metadata.py generate
```

Regenerate one extension:

```sh
python3 tools/generate_connector_metadata.py generate irodori-extension-trino-presto
```

Recreate presets and source files from the existing generated JSON:

```sh
python3 tools/generate_connector_metadata.py bootstrap --write
```

