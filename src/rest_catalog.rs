//! Iceberg REST catalog client.
//!
//! When a connection profile carries a `catalogUri` option, the connector
//! speaks the Iceberg REST catalog API (`/v1/config`, `/v1/namespaces`,
//! `/v1/namespaces/{ns}/tables`, table load) instead of scanning a single
//! table path. Namespaces surface as schemas, tables as objects, and every
//! table is exposed to DuckDB as a view over `iceberg_scan` on the table's
//! current metadata location.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::driver::{load_extension, option_string, sql_string};

/// Upper bounds that keep catalog sync predictable on large warehouses.
const MAX_NAMESPACES: usize = 200;
const MAX_TABLES: usize = 500;
const MAX_PAGES_PER_LIST: usize = 50;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Default multi-level namespace separator mandated by the Iceberg REST spec
/// (0x1F unit separator, percent-encoded in URLs). Catalogs may override it
/// through the `namespace-separator` config property.
const NAMESPACE_SEPARATOR: &str = "%1F";

/// Default OAuth2 scope for the client credentials flow, per the Iceberg REST
/// spec's deprecated-but-standard `oauth/tokens` endpoint.
const DEFAULT_OAUTH2_SCOPE: &str = "catalog";

pub(crate) struct RestCatalog {
    base: String,
    prefix: String,
    namespace_separator: String,
    auth: Auth,
    warehouse: Option<String>,
    table_filter: Option<(Vec<String>, String)>,
}

/// How catalog requests are authenticated.
enum Auth {
    None,
    /// User-supplied bearer token, sent as-is. Takes precedence over OAuth2
    /// because it needs no extra round trip and already worked before OAuth2
    /// support existed.
    StaticBearer(String),
    /// OAuth2 client credentials flow (`POST oauth/tokens`): the token is
    /// fetched at connect and refreshed once whenever the catalog answers 401.
    OAuth2(OAuth2Client),
}

struct OAuth2Client {
    /// Token endpoint. Defaults to `{catalogUri}/v1/oauth/tokens` (the spec's
    /// catalog-hosted endpoint, which is not under the catalog prefix); an
    /// explicit `oauth2ServerUri` option or a `oauth2-server-uri` property in
    /// the catalog's `/v1/config` response overrides it.
    token_endpoint: String,
    /// True when the user set `oauth2ServerUri` themselves, in which case the
    /// catalog config cannot override the endpoint.
    endpoint_pinned: bool,
    client_id: Option<String>,
    client_secret: String,
    scope: String,
    access_token: Mutex<Option<String>>,
}

/// Distinguishes 401 responses (which trigger one OAuth2 token refresh) from
/// every other HTTP failure.
enum HttpFailure {
    Unauthorized(String),
    Other(String),
}

impl HttpFailure {
    fn into_message(self) -> String {
        match self {
            HttpFailure::Unauthorized(message) | HttpFailure::Other(message) => message,
        }
    }
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
    let auth = match token {
        Some(token) => Auth::StaticBearer(token),
        None => match oauth2_from_request(request, &base)? {
            Some(client) => Auth::OAuth2(client),
            None => Auth::None,
        },
    };
    let warehouse = option_string(request, &["warehouse", "warehousePath"]);
    let table_filter = match option_string(request, &["tableIdentifier"]) {
        Some(identifier) => Some(parse_table_identifier(&identifier)?),
        None => None,
    };

    let mut catalog = RestCatalog {
        base,
        prefix: String::new(),
        namespace_separator: NAMESPACE_SEPARATOR.to_string(),
        auth,
        warehouse,
        table_filter,
    };
    // Fetch the OAuth2 token before /v1/config so connect fails fast with a
    // credential-shaped message instead of a confusing 401 from the catalog.
    if let Auth::OAuth2(client) = &catalog.auth {
        client.fetch_token()?;
    }
    catalog.fetch_config()?;
    Ok(Some(catalog))
}

/// Builds the OAuth2 client credentials configuration, if any OAuth2 option is
/// present. Credential material is resolved in this order:
///
/// 1. `credential` option in the spec's `clientId:clientSecret` form (a value
///    without a colon is a bare client secret, matching Iceberg clients);
/// 2. `oauth2ClientId` / `oauth2ClientSecret` options;
/// 3. the profile's `user` / `password` fields — the desktop app keeps
///    `password` session-only (never persisted), so this is the channel that
///    keeps the client secret out of saved settings.
fn oauth2_from_request(request: &Value, base: &str) -> Result<Option<OAuth2Client>, String> {
    let credential = option_string(
        request,
        &["credential", "oauth2Credential", "catalogCredential"],
    );
    let client_id_option = option_string(request, &["oauth2ClientId", "clientId"]);
    let client_secret_option = option_string(request, &["oauth2ClientSecret", "clientSecret"]);
    let server_uri = option_string(request, &["oauth2ServerUri", "oauthServerUri"]);
    if credential.is_none()
        && client_id_option.is_none()
        && client_secret_option.is_none()
        && server_uri.is_none()
    {
        return Ok(None);
    }

    let (credential_id, credential_secret) = match &credential {
        Some(value) => parse_credential(value),
        None => (None, None),
    };
    let client_id = credential_id
        .or(client_id_option)
        .or_else(|| option_string(request, &["user"]));
    let client_secret = credential_secret
        .or(client_secret_option)
        .or_else(|| option_string(request, &["password"]));
    let Some(client_secret) = client_secret else {
        return Err(
            "OAuth2 catalog authentication is configured but no client secret was found. \
             Set the credential option (clientId:clientSecret), or set oauth2ClientId and \
             put the client secret in the connection password field (which is kept out of \
             saved settings)."
                .to_string(),
        );
    };
    let (token_endpoint, endpoint_pinned) = match &server_uri {
        Some(uri) => (resolve_token_endpoint(base, uri), true),
        None => (format!("{base}/v1/oauth/tokens"), false),
    };
    let scope = option_string(request, &["scope", "oauth2Scope"])
        .unwrap_or_else(|| DEFAULT_OAUTH2_SCOPE.to_string());
    Ok(Some(OAuth2Client {
        token_endpoint,
        endpoint_pinned,
        client_id,
        client_secret,
        scope,
        access_token: Mutex::new(None),
    }))
}

/// Splits the spec's `credential` form: `clientId:clientSecret`, or a bare
/// client secret when there is no colon (matching pyiceberg/iceberg-java).
fn parse_credential(credential: &str) -> (Option<String>, Option<String>) {
    match credential.split_once(':') {
        Some((id, secret)) => {
            let id = id.trim();
            let secret = secret.trim();
            (
                (!id.is_empty()).then(|| id.to_string()),
                (!secret.is_empty()).then(|| secret.to_string()),
            )
        }
        None => {
            let secret = credential.trim();
            (None, (!secret.is_empty()).then(|| secret.to_string()))
        }
    }
}

/// `oauth2ServerUri` may be absolute or a path relative to the catalog base.
fn resolve_token_endpoint(base: &str, uri: &str) -> String {
    let uri = uri.trim();
    if uri.starts_with("http://") || uri.starts_with("https://") {
        uri.trim_end_matches('/').to_string()
    } else {
        format!("{base}/{}", uri.trim_start_matches('/'))
    }
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
        for source in ["overrides", "defaults"] {
            if let Some(separator) = config
                .get(source)
                .and_then(|section| section.get("namespace-separator"))
                .and_then(Value::as_str)
            {
                if !separator.is_empty() {
                    self.namespace_separator = separator.to_string();
                    break;
                }
            }
        }
        // Catalogs may advertise their token endpoint; honor it for future
        // refreshes unless the user pinned one explicitly.
        if let Auth::OAuth2(client) = &mut self.auth {
            if !client.endpoint_pinned {
                for source in ["overrides", "defaults"] {
                    if let Some(uri) = config
                        .get(source)
                        .and_then(|section| section.get("oauth2-server-uri"))
                        .and_then(Value::as_str)
                    {
                        if !uri.is_empty() {
                            client.token_endpoint = resolve_token_endpoint(&self.base, uri);
                            break;
                        }
                    }
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

    /// Sends an authenticated GET. When the catalog answers 401 in OAuth2
    /// mode the token is refreshed once (it may simply have expired) and the
    /// request retried once; a second 401 fails with a clear message.
    fn http_get(&self, url: &str) -> Result<Value, String> {
        match self.http_get_once(url) {
            Err(HttpFailure::Unauthorized(message)) => {
                let Auth::OAuth2(client) = &self.auth else {
                    return Err(self.scrub(&message));
                };
                client.fetch_token().map_err(|err| {
                    format!("catalog rejected the OAuth2 token and refreshing it failed: {err}")
                })?;
                match self.http_get_once(url) {
                    Err(HttpFailure::Unauthorized(message)) => Err(format!(
                        "catalog still rejects requests after refreshing the OAuth2 token \
                         ({}). Check that the credential grants access to this catalog.",
                        self.scrub(&message)
                    )),
                    other => other.map_err(|failure| self.scrub(&failure.into_message())),
                }
            }
            other => other.map_err(|failure| self.scrub(&failure.into_message())),
        }
    }

    fn http_get_once(&self, url: &str) -> Result<Value, HttpFailure> {
        let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
        let mut request = agent.get(url);
        if let Some(token) = self.bearer_token() {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let body = match request.call() {
            Ok(response) => response.into_string().map_err(|err| {
                HttpFailure::Other(format!(
                    "GET {url} failed while reading the response: {err}"
                ))
            })?,
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string().unwrap_or_default();
                let detail: String = body.chars().take(300).collect();
                let message = format!("GET {url} returned HTTP {code}: {detail}");
                return Err(if code == 401 {
                    HttpFailure::Unauthorized(message)
                } else {
                    HttpFailure::Other(message)
                });
            }
            Err(err) => return Err(HttpFailure::Other(format!("GET {url} failed: {err}"))),
        };
        serde_json::from_str(&body)
            .map_err(|err| HttpFailure::Other(format!("GET {url} returned invalid JSON: {err}")))
    }

    fn bearer_token(&self) -> Option<String> {
        match &self.auth {
            Auth::None => None,
            Auth::StaticBearer(token) => Some(token.clone()),
            Auth::OAuth2(client) => client.current_token(),
        }
    }

    /// Removes credential material from a message before it can surface in an
    /// error. The driver additionally redacts request-derived secrets; this
    /// covers the fetched access token, which only this module knows.
    fn scrub(&self, message: &str) -> String {
        let mut message = message.to_string();
        match &self.auth {
            Auth::None => {}
            Auth::StaticBearer(token) => message = redact_value(&message, token),
            Auth::OAuth2(client) => {
                message = redact_value(&message, &client.client_secret);
                if let Some(token) = client.current_token() {
                    message = redact_value(&message, &token);
                }
            }
        }
        message
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
                    encode_namespace(levels, &self.namespace_separator)
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
        let url = format!(
            "namespaces/{}/tables",
            encode_namespace(namespace, &self.namespace_separator)
        );
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
            encode_namespace(namespace, &self.namespace_separator),
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

impl OAuth2Client {
    /// Exchanges the client credentials for an access token
    /// (`grant_type=client_credentials`) and stores it for later requests.
    /// Error messages never include the client secret or a token.
    fn fetch_token(&self) -> Result<String, String> {
        let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "client_credentials"),
            ("scope", &self.scope),
            ("client_secret", &self.client_secret),
        ];
        if let Some(client_id) = &self.client_id {
            form.push(("client_id", client_id));
        }
        let endpoint = &self.token_endpoint;
        let body = match agent.post(endpoint).send_form(&form) {
            Ok(response) => response.into_string().map_err(|err| {
                format!("OAuth2 token request to {endpoint} failed while reading the response: {err}")
            })?,
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string().unwrap_or_default();
                return Err(format!(
                    "OAuth2 token request to {endpoint} was rejected (HTTP {code}{}). \
                     Check the client id, client secret, and scope.",
                    oauth_error_detail(&body)
                ));
            }
            Err(err) => {
                return Err(format!(
                    "OAuth2 token endpoint {endpoint} is unreachable ({err}). \
                     Check the oauth2ServerUri option or the catalog URI."
                ))
            }
        };
        let response: Value = serde_json::from_str(&body).map_err(|err| {
            format!("OAuth2 token response from {endpoint} is not valid JSON: {err}")
        })?;
        let token = response
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                format!("OAuth2 token response from {endpoint} did not include an access_token")
            })?
            .to_string();
        if let Ok(mut guard) = self.access_token.lock() {
            *guard = Some(token.clone());
        }
        Ok(token)
    }

    fn current_token(&self) -> Option<String> {
        self.access_token
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }
}

/// Formats the `error`/`error_description` fields of an OAuth2 error response
/// (RFC 6749 section 5.2) without echoing the raw body.
fn oauth_error_detail(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return String::new();
    };
    let error = value.get("error").and_then(Value::as_str).unwrap_or("");
    let description = value
        .get("error_description")
        .and_then(Value::as_str)
        .unwrap_or("");
    match (error.is_empty(), description.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!(", {error}"),
        (true, false) => format!(", {description}"),
        (false, false) => format!(", {error}: {description}"),
    }
}

fn redact_value(message: &str, secret: &str) -> String {
    if secret.is_empty() {
        message.to_string()
    } else {
        message.replace(secret, "****")
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

fn encode_namespace(levels: &[String], separator: &str) -> String {
    levels
        .iter()
        .map(|level| encode_component(level))
        .collect::<Vec<_>>()
        .join(separator)
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
            encode_namespace(&["a".to_string(), "b c".to_string()], NAMESPACE_SEPARATOR),
            "a%1Fb%20c"
        );
        assert_eq!(
            encode_namespace(&["a".to_string(), "b".to_string()], "%2E"),
            "a%2Eb"
        );
        assert_eq!(encode_component("a/b?c"), "a%2Fb%3Fc");
    }

    #[test]
    fn parses_oauth2_credentials() {
        assert_eq!(
            parse_credential("client-id:client-secret"),
            (
                Some("client-id".to_string()),
                Some("client-secret".to_string())
            )
        );
        assert_eq!(
            parse_credential("only-secret"),
            (None, Some("only-secret".to_string()))
        );
        assert_eq!(
            parse_credential("id-only:"),
            (Some("id-only".to_string()), None)
        );
        assert_eq!(parse_credential(""), (None, None));
        // Extra colons belong to the secret, matching `split(":", 2)` clients.
        assert_eq!(
            parse_credential("id:se:cret"),
            (Some("id".to_string()), Some("se:cret".to_string()))
        );
    }

    #[test]
    fn resolves_oauth2_token_endpoints() {
        let base = "https://catalog.example.com";
        assert_eq!(
            resolve_token_endpoint(base, "https://auth.example.com/oauth/tokens/"),
            "https://auth.example.com/oauth/tokens"
        );
        assert_eq!(
            resolve_token_endpoint(base, "/v1/oauth/tokens"),
            "https://catalog.example.com/v1/oauth/tokens"
        );
        assert_eq!(
            resolve_token_endpoint(base, "v1/oauth/tokens"),
            "https://catalog.example.com/v1/oauth/tokens"
        );
    }

    #[test]
    fn formats_oauth_error_details_without_echoing_bodies() {
        assert_eq!(
            oauth_error_detail(r#"{"error":"invalid_client","error_description":"bad"}"#),
            ", invalid_client: bad"
        );
        assert_eq!(oauth_error_detail(r#"{"error":"invalid_client"}"#), ", invalid_client");
        assert_eq!(oauth_error_detail("credential=oops not json"), "");
        assert_eq!(oauth_error_detail("{}"), "");
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
