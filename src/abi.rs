#![allow(dead_code)]

use serde_json::{json, Value};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IrodoriConnectorBuffer {
    pub ptr: *const u8,
    pub len: usize,
}

pub fn owned_buffer(value: String) -> IrodoriConnectorBuffer {
    let mut bytes = value.into_bytes().into_boxed_slice();
    let buffer = IrodoriConnectorBuffer {
        ptr: bytes.as_mut_ptr(),
        len: bytes.len(),
    };
    std::mem::forget(bytes);
    buffer
}

pub fn json_buffer(value: Value) -> IrodoriConnectorBuffer {
    owned_buffer(value.to_string())
}

pub fn free_owned_buffer(buffer: IrodoriConnectorBuffer) {
    if buffer.ptr.is_null() {
        return;
    }
    unsafe {
        let slice = std::ptr::slice_from_raw_parts_mut(buffer.ptr as *mut u8, buffer.len);
        drop(Box::from_raw(slice));
    }
}

pub fn buffer_to_string(buffer: IrodoriConnectorBuffer) -> Result<String, ()> {
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

pub fn ok(mut payload: serde_json::Map<String, Value>) -> IrodoriConnectorBuffer {
    payload.insert("ok".to_string(), Value::Bool(true));
    json_buffer(Value::Object(payload))
}

pub fn error(code: &str, message: impl Into<String>) -> IrodoriConnectorBuffer {
    json_buffer(json!({
        "ok": false,
        "error": {
            "code": code,
            "message": message.into()
        }
    }))
}

pub fn parse_request(
    buffer: IrodoriConnectorBuffer,
) -> Result<Option<Value>, IrodoriConnectorBuffer> {
    let request = buffer_to_string(buffer).map_err(|_| {
        error(
            "connector.invalidRequest",
            "Connector request buffer must be empty or valid UTF-8 JSON.",
        )
    })?;
    let trimmed = request.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_str::<Value>(trimmed)
        .map(Some)
        .map_err(|err| {
            error(
                "connector.invalidJson",
                format!("Connector request must be valid JSON: {err}"),
            )
        })
}

pub fn request_method(request: Option<&Value>) -> Result<&str, IrodoriConnectorBuffer> {
    match request {
        None => Ok("health"),
        Some(value) => value
            .get("method")
            .and_then(Value::as_str)
            .filter(|method| !method.trim().is_empty())
            .ok_or_else(|| {
                error(
                    "connector.invalidRequest",
                    "Connector request needs a string method.",
                )
            }),
    }
}

pub fn string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

pub fn profile_field<'a>(request: &'a Value, field: &str) -> Option<&'a str> {
    string_field(request, field).or_else(|| {
        request
            .get("profile")
            .and_then(|profile| string_field(profile, field))
    })
}

pub fn connection_id(request: Option<&Value>) -> String {
    request
        .and_then(|value| {
            string_field(value, "connectionId")
                .or_else(|| string_field(value, "id"))
                .or_else(|| {
                    value
                        .get("profile")
                        .and_then(|profile| string_field(profile, "id"))
                })
        })
        .unwrap_or("default")
        .trim()
        .to_string()
}

pub fn max_rows(request: &Value) -> usize {
    request
        .get("maxRows")
        .or_else(|| request.get("limit"))
        .and_then(Value::as_u64)
        .unwrap_or(10_000)
        .clamp(1, 100_000) as usize
}
