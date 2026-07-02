# Iceberg Connector

Adds Apache Iceberg REST, Glue, and Hive catalog workflows as an installable connector extension.

This connector is listed in the public Irodori extension marketplace.

## Connector

- Extension ID: `irodori.iceberg`
- Engine ID: `iceberg`
- Wire: `lakehouse`
- Default port: `443`
- Native ABI: `irodori.connector.native.v1`
- Driver linked: `true`

No desktop adapter source exists yet; this package starts from the refactored ABI shim and connector metadata.

Connector metadata lives in `connector.config.json` and `irodori.extension.json`.
The Rust code keeps native ABI exports in `src/lib.rs`, shared buffer/JSON helpers in `src/abi.rs`, and DuckDB-backed lakehouse behavior in `src/driver.rs`.

## Connection Metadata

- Endpoint modes: `catalog`, `objectStorage`, `jdbc`, `connectionString`
- Transport modes: `direct`, `sshTunnel`, `socks5Proxy`, `httpConnectProxy`, `proxyChain`
- TLS supported: `true`
- Custom driver options: `true`

| Auth method | Label | Secret purposes |
|---|---|---|
| `none` | No authentication | none |
| `connectionString` | Connection string / DSN | none |
| `awsSigV4` | AWS SigV4 | `token` |
| `awsProfile` | AWS shared config profile | none |
| `oauth2` | OAuth 2.0 | `token` |
| `catalogBearerToken` | Catalog bearer token | `token` |
| `catalogPassword` | Catalog user/password | `password` |
| `serviceAccountJson` | Service account JSON | `privateKey` |
| `azureAd` | Azure AD / Entra ID | `token` |
| `sasToken` | SAS token | `token` |
| `customDriverOptions` | Custom driver options | `password`, `token`, `privateKey`, `privateKeyPassphrase` |

## ABI Calls

The driver handles these JSON requests today:

| Method | Response |
|---|---|
| `health` / `ping` | Connector health, engine id, ABI version, and driver link status. |
| `describe` / `capabilities` | Embedded manifest and connector config. |
| `manifest` | Raw `irodori.extension.json`. |
| `config` | Raw `connector.config.json`. |
| `connect` | Opens an embedded DuckDB lakehouse runtime and creates a table view when a path is provided. |
| `query` | Runs SQL through the embedded runtime. |
| `metadata` | Reads view/table metadata through `information_schema`. |
| `close` | Removes the cached native connection. |

## Development


Generated extension repositories share `../target` across sibling repositories so Rust dependencies are compiled once per checkout. DuckDB and MotherDuck are driver-linked by default; set `IRODORI_CONNECTOR_LINK_DUCKDB=0` only when you need metadata-only DuckDB-compatible scaffolds.


```sh
make check
make build
```

Release packages place platform-specific native artifacts under `dist/native`.
