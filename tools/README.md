# Connector Metadata

`connector.source.json` is the human-editable source of truth for each extension.
`connector.config.json` and `irodori.extension.json` are generated packaging
artifacts kept for compatibility with the current native ABI and marketplace
layout.

The shared connector metadata generator lives in the `irodori-table`
coordinator repository. This extension repository keeps generated artifacts and
local README helpers only.

## Commands

Regenerate English README files from the generated connector metadata:

```sh
python3 tools/generate_readmes.py
```
