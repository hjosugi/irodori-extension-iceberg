<!-- i18n: language-switcher -->
[English](README.md) | [日本語](README.ja.md)

# Iceberg Connector

Native Irodori Table connector extension for Iceberg.

This crate packages the connector metadata, native ABI exports, and driver implementation used by the Irodori extension marketplace.

## Connector

- Extension ID: `irodori.iceberg`
- Engine ID: `iceberg`
- Wire protocol: `lakehouse`
- Default port: `443`
- Native ABI: `irodori.connector.native.v1`
- Driver linked: `yes`
- Marketplace visibility: `public`
- Package version: `0.1.3`

The package uses the connector metadata and native driver directly; no desktop adapter source snapshot is required.

Connector metadata lives in `connector.config.json` and `irodori.extension.json`.
The Rust crate exports the native ABI from `src/lib.rs`, uses `irodori-connector-abi` for shared JSON/buffer helpers, and keeps connector behavior in `src/driver.rs`.

## Connection Metadata

- Endpoint modes: `catalog`, `objectStorage`, `jdbc`, `connectionString`
- Transport modes: `direct`, `sshTunnel`, `socks5Proxy`, `httpConnectProxy`, `proxyChain`
- TLS supported: `yes`
- TLS required by default: `yes`
- Custom driver options: `yes`

### Endpoint Fields

| Field | Label | Type | Required |
| --- | --- | --- | --- |
| `catalogType` | Catalog type | `string` | yes |
| `catalogUri` | Catalog URI | `uri` | no |
| `warehouse` | Warehouse path | `string` | no |
| `tableIdentifier` | Table identifier | `string` | no |
| `storageBackend` | Storage backend | `string` | no |
| `region` | Cloud region | `string` | no |
| `credentialVending` | Credential vending | `boolean` | no |

## Authentication

The connector advertises these authentication modes so clients can render the right credential fields. Driver-specific or provider-specific values can still be passed through `options` when needed.

| Auth method | Label | Kind | Secret purposes |
| --- | --- | --- | --- |
| `none` | No authentication | `none` | none |
| `connectionString` | Connection string / DSN | `connectionString` | none |
| `awsDefaultCredentialsChain` | AWS default credential chain | `iam` | none |
| `awsSigV4` | AWS SigV4 | `iam` | `token` |
| `awsProfile` | AWS shared config profile | `iam` | none |
| `awsSso` | AWS IAM Identity Center / SSO | `iam` | `token` |
| `webIdentity` | AWS web identity | `iam` | `token` |
| `awsAssumeRole` | AWS STS assume role | `iam` | `token` |
| `sessionToken` | AWS session token | `token` | `token` |
| `oauth2` | OAuth 2.0 | `oauth2` | `token` |
| `catalogBearerToken` | Catalog bearer token | `token` | `token` |
| `catalogPassword` | Catalog user/password | `userPassword` | `password` |
| `serviceAccountJson` | Service account JSON | `serviceAccount` | `privateKey` |
| `serviceAccountJwt` | Service account JWT private key | `privateKey` | `privateKey`, `privateKeyPassphrase` |
| `serviceAccountImpersonation` | Service account impersonation | `iam` | `token` |
| `googleApplicationDefaultCredentials` | Application Default Credentials | `iam` | none |
| `workloadIdentity` | Workload identity federation | `iam` | `token` |
| `azureAd` | Azure AD / Entra ID | `azureAd` | `token` |
| `servicePrincipal` | Service principal | `oauth2` | `token` |
| `servicePrincipalCertificate` | Service principal certificate | `oauth2` | `privateKey`, `privateKeyPassphrase` |
| `managedIdentity` | Managed identity | `managedIdentity` | none |
| `sasToken` | SAS token | `token` | `token` |
| `customDriverOptions` | Custom driver options | `custom` | `password`, `token`, `privateKey`, `privateKeyPassphrase` |

## Native ABI Calls

| Method | Response |
| --- | --- |
| `health` | Returns connector health, engine id, ABI version, and driver status. |
| `describe` | Returns the embedded manifest and connector config. |
| `manifest` | Returns raw `irodori.extension.json`. |
| `config` | Returns raw `connector.config.json`. |
| `connect` | Opens and validates a native connector connection. |
| `query` | Runs a connector query and returns structured rows or JSON results. |
| `metadata` | Reads schemas, tables, columns, indexes, collections, or equivalent metadata. |
| `close` | Closes and removes a cached native connection. |

## Development

All extension crates in this checkout share `../target` so dependencies compile once across sibling repositories.

```sh
make check
make build
```

Release packages place platform-specific native artifacts under `dist/native`.

## License

0BSD. You can use, copy, modify, and distribute this project for almost any purpose.
