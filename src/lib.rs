//! Native connector ABI for Iceberg.
//!
//! Generated extension entrypoints stay small: `irodori-connector-abi` owns
//! buffer/JSON ABI mechanics, and `driver` owns connector behavior.

pub use irodori_connector_abi as abi;

mod driver;

irodori_connector_abi::irodori_export_connector!(
    engine: "iceberg",
    driver: driver,
    config: "../connector.config.json",
    manifest: "../irodori.extension.json",
    driver_linked: true,
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn buffer_from_str(value: &'static str) -> IrodoriConnectorBuffer {
        IrodoriConnectorBuffer {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }

    fn buffer_from_bytes(value: &'static [u8]) -> IrodoriConnectorBuffer {
        IrodoriConnectorBuffer {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }

    fn buffer_to_string(buffer: IrodoriConnectorBuffer) -> String {
        let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) };
        let value = std::str::from_utf8(bytes).unwrap().to_string();
        irodori_connector_free_buffer(buffer);
        value
    }

    fn buffer_to_json(buffer: IrodoriConnectorBuffer) -> Value {
        let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) };
        let value = serde_json::from_slice(bytes).unwrap();
        irodori_connector_free_buffer(buffer);
        value
    }

    fn call(request: &'static str) -> Value {
        buffer_to_json(irodori_connector_call_json(buffer_from_str(request)))
    }

    #[test]
    fn manifest_and_config_describe_the_same_connector() {
        let manifest: Value = serde_json::from_str(MANIFEST_JSON).unwrap();
        let config: Value = serde_json::from_str(CONFIG_JSON).unwrap();
        let connector = &manifest["contributes"]["connectors"][0];

        assert_eq!(manifest["id"], config["extensionId"]);
        assert_eq!(connector["engine"], ENGINE);
        assert_eq!(connector["engine"], config["connector"]["engine"]);
        assert_eq!(connector["module"], config["connector"]["module"]);
        assert_eq!(connector["connection"], config["connection"]);
        assert_eq!(config["runtime"]["driverLinked"], json!(true));
        assert!(config["connection"]["authMethods"]
            .as_array()
            .is_some_and(|methods| !methods.is_empty()));
        assert!(config["connection"]["secretPurposes"].as_array().is_some());
        assert!(manifest["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|permission| permission == "connectors"));
    }

    #[test]
    fn abi_exports_owned_json() {
        assert_eq!(irodori_extension_abi_version(), ABI_VERSION);
        assert_eq!(buffer_to_string(irodori_connector_engine_json()), ENGINE);
        assert_eq!(
            buffer_to_string(irodori_extension_manifest_json()),
            MANIFEST_JSON
        );
        assert_eq!(
            buffer_to_string(irodori_connector_config_json()),
            CONFIG_JSON
        );
    }

    #[test]
    fn call_json_reports_health_and_describes_metadata() {
        let health = call(r#"{"method":"health"}"#);
        assert_eq!(health["ok"], true);
        assert_eq!(health["engine"], ENGINE);
        assert_eq!(health["driverLinked"], json!(true));

        let describe = call(r#"{"method":"describe"}"#);
        assert_eq!(describe["ok"], true);
        assert_eq!(describe["driverLinked"], json!(true));
        assert_eq!(
            describe["manifest"]["id"],
            describe["config"]["extensionId"]
        );
        assert_eq!(describe["config"]["connector"]["engine"], ENGINE);
    }

    #[test]
    fn call_json_rejects_query_without_connection() {
        let response = call(r#"{"method":"query","sql":"select 1"}"#);
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "connector.connectionNotFound");
    }

    #[test]
    fn call_json_rejects_invalid_request_buffers() {
        let invalid_utf8 = buffer_to_json(irodori_connector_call_json(buffer_from_bytes(&[
            0xff, 0xfe,
        ])));
        assert_eq!(invalid_utf8["ok"], false);
        assert_eq!(invalid_utf8["error"]["code"], "connector.invalidRequest");

        let invalid_json = call("{");
        assert_eq!(invalid_json["ok"], false);
        assert_eq!(invalid_json["error"]["code"], "connector.invalidJson");

        let invalid_null = buffer_to_json(irodori_connector_call_json(IrodoriConnectorBuffer {
            ptr: std::ptr::null(),
            len: 1,
        }));
        assert_eq!(invalid_null["ok"], false);
        assert_eq!(invalid_null["error"]["code"], "connector.invalidRequest");
    }
}
