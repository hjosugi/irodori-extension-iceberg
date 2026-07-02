# Native Source

There is no existing desktop adapter source for this connector yet.



This directory is a migration staging area for `irodori.iceberg`. The active native
entrypoints live in `src/lib.rs`, shared ABI helpers live in `src/abi.rs`, and
engine behavior lives in `src/stub.rs` or `src/driver.rs`. Engine-specific
connect/query/metadata code should move from these snapshots into that behavior
module as the connector runtime contract is wired into the desktop app.

## Migration Snapshots

| Kind | Source | Destination | SHA-256 |
|---|---|---|---|
| `desktop-db-contract` | `apps/desktop/src-tauri/src/db/connection.rs` | `native/source/irodori-table/apps/desktop/src-tauri/src/db/connection.rs` | `54051346e0402f182d87e2f0e7692d8fc50a8cedd7e1ba4b02b2abfa1f514a47` |
| `desktop-db-contract` | `apps/desktop/src-tauri/src/db/profile.rs` | `native/source/irodori-table/apps/desktop/src-tauri/src/db/profile.rs` | `a4ac432937eb051c4b3434751b5153b33974b8294a3521745ed021da377c458f` |
| `desktop-db-contract` | `apps/desktop/src-tauri/src/db/transport.rs` | `native/source/irodori-table/apps/desktop/src-tauri/src/db/transport.rs` | `bcc16c52a19739b0706fd83143ae089ae91310af963885c1c75b5ba955e85add` |
| `desktop-db-contract` | `apps/desktop/src-tauri/src/db/query.rs` | `native/source/irodori-table/apps/desktop/src-tauri/src/db/query.rs` | `ca2605bf1645da61edf8195408717c682593fd552faf4e246a44a53f2bde665e` |
| `desktop-db-contract` | `apps/desktop/src-tauri/src/db/meta.rs` | `native/source/irodori-table/apps/desktop/src-tauri/src/db/meta.rs` | `de99fe0155f450c0fd9a439bee52cd7f66b7bc5174db500e00c8293360706be7` |
| `desktop-db-contract` | `apps/desktop/src-tauri/src/db/explain.rs` | `native/source/irodori-table/apps/desktop/src-tauri/src/db/explain.rs` | `a741b485e3bb4702634c02fd9d673f6e82680777118b56d9ffcbb1d40e6aec20` |
| `portable-connection-contract` | `../irodori-kit/irodori-connection/src/lib.rs` | `native/source/irodori-table/../irodori-kit/irodori-connection/src/lib.rs` | `05197e61760bf2afe02efd33380328e10c5730601c3003ff356f7579eb2c23d6` |
| `portable-connection-contract` | `../irodori-kit/irodori-connection/src/portable.rs` | `native/source/irodori-table/../irodori-kit/irodori-connection/src/portable.rs` | `ab45663fa9a101b66af686bfb45404c2c475e82578842f3f18af024ad5c9fbe8` |
| `secure-store-contract` | `../irodori-kit/irodori-secure-store/src/lib.rs` | `native/source/irodori-table/../irodori-kit/irodori-secure-store/src/lib.rs` | `85fb8b001936ba9090da8a4e3197d61bc54d833689d71efa1d0aeb99b91bdb9f` |
| `transport-contract` | `../irodori-kit/irodori-core/src/lib.rs` | `native/source/irodori-table/../irodori-kit/irodori-core/src/lib.rs` | `c356aaa20dd6fdaf71aae8b08febcf2cafbbe53bfda60e276bb71f57fc281510` |
| `transport-runtime` | `../irodori-kit/irodori-proxy/src/lib.rs` | `native/source/irodori-table/../irodori-kit/irodori-proxy/src/lib.rs` | `4ff9e90f61f69aa2f7a8c663b0a13e1b0f0c283b166d608a2a0a6613cd03f126` |
| `transport-runtime` | `../irodori-kit/irodori-proxy/src/plan.rs` | `native/source/irodori-table/../irodori-kit/irodori-proxy/src/plan.rs` | `b6e3be9778fd9b543d905dec39c0ddf579e739313e0bf24881f6152b6da94a39` |
| `transport-runtime` | `../irodori-kit/irodori-proxy/src/resolved.rs` | `native/source/irodori-table/../irodori-kit/irodori-proxy/src/resolved.rs` | `4b1ba3f95e49fd582dd82de39452d03a597abe36a8738772e6f4bdbab753772d` |

Engine status from `knowledge/engines.json`: `recognized_no_connector`.
