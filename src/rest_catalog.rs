//! Iceberg REST catalog client.
//!
//! When a connection profile carries a `catalogUri` option, the connector
//! speaks the Iceberg REST catalog API (`/v1/config`, `/v1/namespaces`,
//! `/v1/namespaces/{ns}/tables`, table load) instead of scanning a single
//! table path. Namespaces surface as schemas, tables as objects, and every
//! table is exposed to DuckDB as a view over `iceberg_scan` on the table's
//! current metadata location.

use std::collections::VecDeque;
use std::time::Duration;

use serde_json::{json, Value};

use crate::driver::{load_extension, option_string, sql_string};

/// Upper bounds that keep catalog sync predictable on large warehouses.
const MAX_NAMESPACES: usize = 200;
const MAX_TABLES: usize = 500;
const MAX_PAGES_PER_LIST: usize = 50;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Multi-level namespace separator mandated by the Iceberg REST spec
/// (0x1F unit separator, percent-encoded in URLs).
const NAMESPACE_SEPARATOR: &str = "%1F";

pub(crate) struct RestCatalog {
    base: String,
    prefix: String,
    token: Option<String>,
    warehouse: Option<String>,
    table_filter: Option<(Vec<String>, String)>,
}

/// Reads catalog options from the connect request. Returns `Ok(None)` when no
/// `catalogUri` is configured, in which case the connector keeps its
/// single-table-path behavior. When a catalog URI is present the catalog is
/// contacted immediately (`GET /v1/config`) so connect fails fast with a clear
/// message instead of surfacing confusing scan errors later.
pub(crate) fn from_request(request: &Value) -> Result<Option<RestCatalog>, String> {
    let Some(raw_uri) = option_string(request, &["catalogUri", "catalogUrl", "catalogEndpoint"])
    else {
        return Ok(None);
    };
    let catalog_type = option_string(request, &["catalogType"])
        .unwrap_or_else(|| "rest".to_string())
        .to_ascii_lowercase();
    if catalog_type != "rest" {
        return Err(format!(
            "catalogType '{catalog_type}' is not supported yet; only 'rest' catalogs are \
             implemented. Clear catalogUri to scan a single table path instead."
        ));
    }
    let base = normalize_base_uri(&raw_uri);
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err(format!(
            "catalogUri must be an http(s) URL, got '{raw_uri}'. \
             For a direct table path, use the table path field instead."
        ));
    }
    let token = option_string(
        request,
        &["catalogToken", "catalogBearerToken", "bearerToken", "token"],
    );
    let warehouse = option_string(request, &["warehouse", "warehousePath"]);
    let table_filter = match option_string(request, &["tableIdentifier"]) {
        Some(identifier) => Some(parse_table_identifier(&identifier)?),
        None => None,
    };

    let mut catalog = RestCatalog {
        base,
        prefix: String::new(),
        token,
        warehouse,
        table_filter,
    };
    catalog.fetch_config()?;
    Ok(Some(catalog))
}

/// Strips trailing slashes and a trailing `/v1` so users can paste either the
/// catalog root or the versioned endpoint the UI placeholder shows.
fn normalize_base_uri(uri: &str) -> String {
    let mut base = uri.trim().trim_end_matches('/').to_string();
    if let Some(stripped) = base.strip_suffix("/v1") {
        base = stripped.trim_end_matches('/').to_string();
    }
    base
}

fn parse_table_identifier(identifier: &str) -> Result<(Vec<String>, String), String> {
    let mut levels: Vec<String> = identifier
        .split('.')
        .map(|level| level.trim().to_string())
        .filter(|level| !level.is_empty())
        .collect();
    if levels.len() < 2 {
        return Err(format!(
            "tableIdentifier '{identifier}' must be qualified as namespace.table"
        ));
    }
    let table = levels.pop().expect("levels has at least two entries");
    Ok((levels, table))
}

impl RestCatalog {
    fn fetch_config(&mut self) -> Result<(), String> {
        let mut url = format!("{}/v1/config", self.base);
        if let Some(warehouse) = &self.warehouse {
            url.push_str("?warehouse=");
            url.push_str(&encode_component(warehouse));
        }
        let config = self.http_get(&url).map_err(|err| {
            format!("Iceberg REST catalog is unreachable ({err}). Check the catalog URI.")
        })?;
        for source in ["overrides", "defaults"] {
            if let Some(prefix) = config
                .get(source)
                .and_then(|section| section.get("prefix"))
                .and_then(Value::as_str)
            {
                self.prefix = prefix.trim_matches('/').to_string();
                if !self.prefix.is_empty() {
                    break;
                }
            }
        }
        Ok(())
    }

    fn v1_url(&self, tail: &str) -> String {
        if self.prefix.is_empty() {
            format!("{}/v1/{tail}", self.base)
        } else {
            format!("{}/v1/{}/{tail}", self.base, self.prefix)
        }
    }

    fn http_get(&self, url: &str) -> Result<Value, String> {
        let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
        let mut request = agent.get(url);
        if let Some(token) = &self.token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let body = match request.call() {
            Ok(response) => response
                .into_string()
                .map_err(|err| format!("GET {url} failed while reading the response: {err}"))?,
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string().unwrap_or_default();
                let detail: String = body.chars().take(300).collect();
                return Err(format!("GET {url} returned HTTP {code}: {detail}"));
            }
            Err(err) => return Err(format!("GET {url} failed: {err}")),
        };
        serde_json::from_str(&body).map_err(|err| format!("GET {url} returned invalid JSON: {err}"))
    }

    fn list_namespaces(&self) -> Result<Vec<Vec<String>>, String> {
        let mut namespaces = Vec::new();
        let mut queue: VecDeque<Option<Vec<String>>> = VecDeque::from([None]);
        while let Some(parent) = queue.pop_front() {
            let base_url = match &parent {
                None => self.v1_url("namespaces"),
                Some(levels) => format!(
                    "{}?parent={}",
                    self.v1_url("namespaces"),
                    encode_namespace(levels)
                ),
            };
            let pages = self.paged(&base_url, parent.is_some(), "namespaces")?;
            for value in pages {
                let Some(levels) = namespace_levels(&value) else {
                    continue;
                };
                if namespaces.len() >= MAX_NAMESPACES {
                    return Ok(namespaces);
                }
                queue.push_back(Some(levels.clone()));
                namespaces.push(levels);
            }
        }
        Ok(namespaces)
    }

    fn list_tables(&self, namespace: &[String]) -> Result<Vec<String>, String> {
        let url = format!("namespaces/{}/tables", encode_namespace(namespace));
        let pages = self.paged(&self.v1_url(&url), false, "identifiers")?;
        Ok(pages
            .into_iter()
            .filter_map(|identifier| {
                identifier
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect())
    }

    /// Follows `next-page-token` pagination, collecting entries from `field`.
    fn paged(&self, base_url: &str, has_query: bool, field: &str) -> Result<Vec<Value>, String> {
        let mut entries = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_PAGES_PER_LIST {
            let url = match &page_token {
                None => base_url.to_string(),
                Some(token) => format!(
                    "{base_url}{}pageToken={}",
                    if has_query || base_url.contains('?') {
                        "&"
                    } else {
                        "?"
                    },
                    encode_component(token)
                ),
            };
            let response = self.http_get(&url)?;
            if let Some(values) = response.get(field).and_then(Value::as_array) {
                entries.extend(values.iter().cloned());
            }
            match response.get("next-page-token").and_then(Value::as_str) {
                Some(token) if !token.is_empty() => page_token = Some(token.to_string()),
                _ => return Ok(entries),
            }
        }
        Ok(entries)
    }

    fn load_table(
        &self,
        namespace: &[String],
        table: &str,
    ) -> Result<(Option<String>, Vec<Value>), String> {
        let url = self.v1_url(&format!(
            "namespaces/{}/tables/{}",
            encode_namespace(namespace),
            encode_component(table)
        ));
        let response = self.http_get(&url)?;
        let metadata_location = response
            .get("metadata-location")
            .and_then(Value::as_str)
            .map(str::to_string);
        let columns = response
            .get("metadata")
            .map(table_columns)
            .unwrap_or_default();
        Ok((metadata_location, columns))
    }
}

/// Synchronizes the DuckDB session with the catalog: lists namespaces and
/// tables, loads each table's current metadata location and schema, and
/// exposes every table as `"<namespace>"."<table>"` view over `iceberg_scan`.
/// Returns the connector metadata document plus non-fatal warnings (for
/// example a table whose storage is unreachable still shows up in the tree).
pub(crate) fn sync(
    catalog: &RestCatalog,
    conn: &duckdb::Connection,
) -> Result<(Value, Vec<String>), String> {
    load_extension(conn, "httpfs", false)?;
    load_extension(conn, "iceberg", true)?;

    let mut warnings = Vec::new();
    let tables: Vec<(Vec<String>, String)> = match &catalog.table_filter {
        Some((namespace, table)) => vec![(namespace.clone(), table.clone())],
        None => {
            let mut tables = Vec::new();
            'namespaces: for namespace in catalog.list_namespaces()? {
                for table in catalog.list_tables(&namespace)? {
                    if tables.len() >= MAX_TABLES {
                        warnings.push(format!(
                            "catalog lists more than {MAX_TABLES} tables; only the first \
                             {MAX_TABLES} are shown"
                        ));
                        break 'namespaces;
                    }
                    tables.push((namespace.clone(), table));
                }
            }
            tables
        }
    };

    let mut schemas: Vec<(String, Vec<Value>)> = Vec::new();
    for (namespace, table) in &tables {
        let schema_name = namespace.join(".");
        let (metadata_location, columns) = match catalog.load_table(namespace, table) {
            Ok(loaded) => loaded,
            Err(err) => {
                warnings.push(format!("failed to load table {schema_name}.{table}: {err}"));
                (None, Vec::new())
            }
        };
        match &metadata_location {
            Some(location) => {
                if let Err(err) = create_table_view(conn, &schema_name, table, location) {
                    warnings.push(format!(
                        "table {schema_name}.{table} is not queryable: {err}"
                    ));
                }
            }
            None => warnings.push(format!(
                "table {schema_name}.{table} has no metadata location; it cannot be scanned"
            )),
        }
        let object = json!({
            "schema": schema_name,
            "name": table,
            "kind": "table",
            "columns": columns,
            "indexes": [],
            "primaryKey": [],
            "foreignKeys": []
        });
        match schemas.iter_mut().find(|(name, _)| name == &schema_name) {
            Some((_, objects)) => objects.push(object),
            None => schemas.push((schema_name, vec![object])),
        }
    }

    let metadata = json!({
        "schemas": schemas
            .into_iter()
            .map(|(name, objects)| json!({ "name": name, "objects": objects }))
            .collect::<Vec<_>>()
    });
    Ok((metadata, warnings))
}

fn create_table_view(
    conn: &duckdb::Connection,
    schema: &str,
    table: &str,
    metadata_location: &str,
) -> Result<(), String> {
    let sql = format!(
        "create schema if not exists {schema_ident}; \
         create or replace view {schema_ident}.{table_ident} as \
         select * from iceberg_scan({location})",
        schema_ident = quote_identifier(schema),
        table_ident = quote_identifier(table),
        location = sql_string(scan_location(metadata_location)),
    );
    conn.execute_batch(&sql).map_err(|err| err.to_string())
}

/// Catalogs frequently hand out `file://` URIs for local warehouses; DuckDB
/// wants plain paths for those. Other schemes (s3://, gs://, ...) pass
/// through untouched.
fn scan_location(location: &str) -> &str {
    location
        .strip_prefix("file://")
        .or_else(|| location.strip_prefix("file:"))
        .unwrap_or(location)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn namespace_levels(value: &Value) -> Option<Vec<String>> {
    let levels: Vec<String> = value
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    if levels.is_empty() {
        None
    } else {
        Some(levels)
    }
}

/// Extracts `{name, dataType, nullable, ordinal}` columns from an Iceberg
/// table metadata document (v2 `schemas` + `current-schema-id`, or v1
/// `schema`).
fn table_columns(metadata: &Value) -> Vec<Value> {
    let schema = current_schema(metadata);
    let Some(fields) = schema
        .and_then(|schema| schema.get("fields"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let name = field.get("name").and_then(Value::as_str)?;
            Some(json!({
                "name": name,
                "dataType": field.get("type").map(type_to_string).unwrap_or_default(),
                "nullable": !field.get("required").and_then(Value::as_bool).unwrap_or(false),
                "ordinal": index + 1
            }))
        })
        .collect()
}

fn current_schema(metadata: &Value) -> Option<&Value> {
    if let Some(schemas) = metadata.get("schemas").and_then(Value::as_array) {
        let current_id = metadata.get("current-schema-id");
        let selected = match current_id {
            Some(id) => schemas
                .iter()
                .find(|schema| schema.get("schema-id") == Some(id)),
            None => None,
        };
        if let Some(schema) = selected.or_else(|| schemas.last()) {
            return Some(schema);
        }
    }
    metadata.get("schema")
}

fn type_to_string(iceberg_type: &Value) -> String {
    match iceberg_type {
        Value::String(name) => name.clone(),
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("struct") => "struct".to_string(),
            Some("list") => format!(
                "list<{}>",
                object
                    .get("element")
                    .map(type_to_string)
                    .unwrap_or_default()
            ),
            Some("map") => format!(
                "map<{}, {}>",
                object.get("key").map(type_to_string).unwrap_or_default(),
                object.get("value").map(type_to_string).unwrap_or_default()
            ),
            _ => Value::Object(object.clone()).to_string(),
        },
        other => other.to_string(),
    }
}

fn encode_namespace(levels: &[String]) -> String {
    levels
        .iter()
        .map(|level| encode_component(level))
        .collect::<Vec<_>>()
        .join(NAMESPACE_SEPARATOR)
}

/// Percent-encodes everything outside the URL-safe unreserved set.
fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_catalog_base_uris() {
        assert_eq!(
            normalize_base_uri("https://catalog.example.com/v1/"),
            "https://catalog.example.com"
        );
        assert_eq!(
            normalize_base_uri("https://catalog.example.com"),
            "https://catalog.example.com"
        );
        assert_eq!(
            normalize_base_uri("http://localhost:8181/catalog/v1"),
            "http://localhost:8181/catalog"
        );
    }

    #[test]
    fn strips_file_scheme_for_duckdb_scans() {
        assert_eq!(
            scan_location("file:///wh/t/metadata.json"),
            "/wh/t/metadata.json"
        );
        assert_eq!(
            scan_location("file:/wh/t/metadata.json"),
            "/wh/t/metadata.json"
        );
        assert_eq!(
            scan_location("s3://bucket/t/metadata.json"),
            "s3://bucket/t/metadata.json"
        );
    }

    #[test]
    fn parses_table_identifiers() {
        assert_eq!(
            parse_table_identifier("analytics.events").unwrap(),
            (vec!["analytics".to_string()], "events".to_string())
        );
        assert_eq!(
            parse_table_identifier("a.b.events").unwrap(),
            (vec!["a".to_string(), "b".to_string()], "events".to_string())
        );
        assert!(parse_table_identifier("events").is_err());
    }

    #[test]
    fn encodes_namespaces_and_components() {
        assert_eq!(
            encode_namespace(&["a".to_string(), "b c".to_string()]),
            "a%1Fb%20c"
        );
        assert_eq!(encode_component("a/b?c"), "a%2Fb%3Fc");
    }

    #[test]
    fn extracts_columns_from_v2_metadata() {
        let metadata = json!({
            "format-version": 2,
            "current-schema-id": 1,
            "schemas": [
                {"schema-id": 0, "fields": []},
                {"schema-id": 1, "fields": [
                    {"id": 1, "name": "id", "required": true, "type": "long"},
                    {"id": 2, "name": "tags", "required": false,
                     "type": {"type": "list", "element": "string", "element-id": 3,
                              "element-required": false}}
                ]}
            ]
        });
        let columns = table_columns(&metadata);
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0]["name"], "id");
        assert_eq!(columns[0]["dataType"], "long");
        assert_eq!(columns[0]["nullable"], false);
        assert_eq!(columns[1]["dataType"], "list<string>");
        assert_eq!(columns[1]["nullable"], true);
    }
}
