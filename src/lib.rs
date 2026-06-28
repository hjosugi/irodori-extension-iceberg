//! Native connector ABI for Iceberg.
//!
//! Connector behavior is declared in ../connector.config.json and
//! ../irodori.extension.json so packaging can customize metadata without
//! changing Rust code.

const ABI_VERSION: u32 = 1;
const ENGINE: &str = "iceberg";
const CONFIG_JSON: &str = include_str!("../connector.config.json");
const MANIFEST_JSON: &str = include_str!("../irodori.extension.json");
const HEALTH_RESPONSE_JSON: &str =
    r#"{"ok":true,"engine":"iceberg","abiVersion":1,"driverLinked":false}"#;
const DESCRIBE_RESPONSE_JSON: &str = concat!(
    r#"{"ok":true,"engine":"iceberg","abiVersion":1,"driverLinked":false,"manifest":"#,
    include_str!("../irodori.extension.json"),
    r#","config":"#,
    include_str!("../connector.config.json"),
    r#"}"#
);
const INVALID_REQUEST_RESPONSE_JSON: &str = r#"{"ok":false,"error":{"code":"connector.invalidRequest","message":"Connector request buffer must be empty or valid UTF-8 JSON."}}"#;
const NOT_LINKED_RESPONSE_JSON: &str = r#"{"ok":false,"error":{"code":"connector.driverNotLinked","message":"The native connector metadata is available, but the engine-specific driver entrypoint is not linked in this package yet."}}"#;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IrodoriConnectorBuffer {
    pub ptr: *const u8,
    pub len: usize,
}

fn static_buffer(value: &'static str) -> IrodoriConnectorBuffer {
    IrodoriConnectorBuffer {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

fn buffer_to_string(buffer: IrodoriConnectorBuffer) -> Result<String, ()> {
    if buffer.ptr.is_null() {
        return if buffer.len == 0 {
            Ok(String::new())
        } else {
            Err(())
        };
    }
    let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) };
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| ())
}

#[no_mangle]
pub extern "C" fn irodori_extension_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn irodori_connector_engine_json() -> IrodoriConnectorBuffer {
    static_buffer(ENGINE)
}

#[no_mangle]
pub extern "C" fn irodori_extension_manifest_json() -> IrodoriConnectorBuffer {
    static_buffer(MANIFEST_JSON)
}

#[no_mangle]
pub extern "C" fn irodori_connector_config_json() -> IrodoriConnectorBuffer {
    static_buffer(CONFIG_JSON)
}

#[no_mangle]
pub extern "C" fn irodori_connector_call_json(
    request: IrodoriConnectorBuffer,
) -> IrodoriConnectorBuffer {
    let Ok(request) = buffer_to_string(request) else {
        return static_buffer(INVALID_REQUEST_RESPONSE_JSON);
    };
    if request.trim().is_empty() || request.contains(r#""health""#) || request.contains(r#""ping""#)
    {
        return static_buffer(HEALTH_RESPONSE_JSON);
    }
    if request.contains(r#""describe""#) || request.contains(r#""capabilities""#) {
        return static_buffer(DESCRIBE_RESPONSE_JSON);
    }
    if request.contains(r#""manifest""#) {
        return static_buffer(MANIFEST_JSON);
    }
    if request.contains(r#""config""#) {
        return static_buffer(CONFIG_JSON);
    }
    static_buffer(NOT_LINKED_RESPONSE_JSON)
}

#[no_mangle]
pub extern "C" fn irodori_connector_free_buffer(_buffer: IrodoriConnectorBuffer) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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

    fn buffer_to_json(buffer: IrodoriConnectorBuffer) -> Value {
        let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) };
        serde_json::from_slice(bytes).unwrap()
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
    fn abi_exports_static_json() {
        assert_eq!(irodori_extension_abi_version(), ABI_VERSION);
        assert!(irodori_extension_manifest_json().len > 0);
        assert!(irodori_connector_config_json().len > 0);
        assert_eq!(irodori_connector_engine_json().len, ENGINE.len());
    }

    #[test]
    fn call_json_reports_health_and_describes_metadata() {
        let health = buffer_to_json(irodori_connector_call_json(buffer_from_str(
            r#"{"method":"health"}"#,
        )));
        assert_eq!(health["ok"], true);
        assert_eq!(health["engine"], ENGINE);
        assert_eq!(health["driverLinked"], false);

        let describe = buffer_to_json(irodori_connector_call_json(buffer_from_str(
            r#"{"method":"describe"}"#,
        )));
        assert_eq!(describe["ok"], true);
        assert_eq!(
            describe["manifest"]["id"],
            describe["config"]["extensionId"]
        );
        assert_eq!(describe["config"]["connector"]["engine"], ENGINE);
    }

    #[test]
    fn call_json_rejects_driver_operations_until_linked() {
        let response = buffer_to_json(irodori_connector_call_json(buffer_from_str(
            r#"{"method":"query","sql":"select 1"}"#,
        )));
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "connector.driverNotLinked");
    }

    #[test]
    fn call_json_rejects_invalid_request_buffers() {
        let invalid_utf8 = buffer_to_json(irodori_connector_call_json(buffer_from_bytes(&[
            0xff, 0xfe,
        ])));
        assert_eq!(invalid_utf8["ok"], false);
        assert_eq!(invalid_utf8["error"]["code"], "connector.invalidRequest");

        let invalid_null = buffer_to_json(irodori_connector_call_json(IrodoriConnectorBuffer {
            ptr: std::ptr::null(),
            len: 1,
        }));
        assert_eq!(invalid_null["ok"], false);
        assert_eq!(invalid_null["error"]["code"], "connector.invalidRequest");
    }
}
