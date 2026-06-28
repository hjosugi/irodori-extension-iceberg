# Iceberg Connector

Adds Apache Iceberg REST, Glue, and Hive catalog workflows as an installable connector extension.

This connector is listed in the public Irodori extension marketplace.

## Connector

- Extension ID: `irodori.iceberg`
- Engine ID: `iceberg`
- Wire: `lakehouse`
- Default port: `443`
- Native ABI: `irodori.connector.native.v1`
- Driver linked: `false`

No desktop adapter source exists yet; this package starts from the ABI shim and connector metadata.

Connector metadata lives in `connector.config.json` and `irodori.extension.json`.
The Rust code exports the native ABI plus self-description calls. Engine-specific connect/query/metadata behavior should be linked behind `irodori_connector_call_json`.

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

The scaffold handles these JSON requests today:

| Method | Response |
|---|---|
| `health` / `ping` | Connector health, engine id, ABI version, and driver link status. |
| `describe` / `capabilities` | Embedded manifest and connector config. |
| `manifest` | Raw `irodori.extension.json`. |
| `config` | Raw `connector.config.json`. |


Driver operations such as `connect`, `query`, and `metadata` intentionally return `connector.driverNotLinked` until the engine implementation is connected.

## Development


Generated extension repositories share `../target` across sibling repositories so Rust dependencies are compiled once per checkout. Driver-linked DuckDB scaffolds are opt-in: run the scaffold with `IRODORI_CONNECTOR_LINK_DUCKDB=1` or `IRODORI_CONNECTOR_LINKED_DRIVERS=duckdb,motherduck` only when you need the local DuckDB driver.


```sh
make check
make build
```

Release packages place platform-specific native artifacts under `dist/native`.
