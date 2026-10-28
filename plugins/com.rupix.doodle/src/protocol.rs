//! Protocol v1 request/response handling (pure JSON logic, headless-testable).
//!
//! Frame transport (u32 LE length + body) lives in `main.rs`; this module only
//! validates the JSON contract so a malformed host request can never reach the
//! UI or the shared-memory layer.

use serde_json::{json, Value};

pub const PROTOCOL: u32 = 1;
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Only mappings from the host (`Local\rupix-ext-...`) may be opened.
pub const SHM_PREFIX: &str = "Local\\rupix-ext-";

/// Host-provided shared-memory description from the request `image` field.
#[derive(Clone, Debug, PartialEq)]
pub struct ShmRef {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunRequest {
    pub id: String,
    pub image_path: Option<String>,
    pub image: Option<ShmRef>,
    pub args: Vec<String>,
}

/// Parse one request body. `protocol` must be 1 and `action` must be `run`.
pub fn parse_request(body: &[u8]) -> Result<RunRequest, String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|e| format!("请求 JSON 解析失败：{e}"))?;
    let obj = value.as_object().ok_or("请求不是 JSON 对象")?;
    if obj.get("protocol").and_then(Value::as_u64) != Some(PROTOCOL as u64) {
        return Err("协议版本不是 1".into());
    }
    let action = obj
        .get("action")
        .and_then(Value::as_str)
        .ok_or("请求缺少 action")?;
    if action != "run" {
        return Err(format!("未知 action：{action}"));
    }
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or("请求缺少 id")?
        .to_string();
    let image_path = obj
        .get("image_path")
        .and_then(Value::as_str)
        .map(str::to_string);
    let image = match obj.get("image") {
        None | Some(Value::Null) => None,
        Some(v) => Some(parse_shm_ref(v)?),
    };
    let args = obj
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok(RunRequest {
        id,
        image_path,
        image,
        args,
    })
}

fn parse_shm_ref(v: &Value) -> Result<ShmRef, String> {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .ok_or("image 缺少 name")?;
    if !name.starts_with(SHM_PREFIX) {
        return Err(format!("共享内存名字前缀非法：{name}"));
    }
    let width = non_zero_u32(v, "width")?;
    let height = non_zero_u32(v, "height")?;
    let stride = non_zero_u32(v, "stride")?;
    if (stride as u64) < (width as u64 * 4) {
        return Err("image stride < width*4".into());
    }
    Ok(ShmRef {
        name: name.to_string(),
        width,
        height,
        stride,
    })
}

fn non_zero_u32(v: &Value, key: &str) -> Result<u32, String> {
    let n = v
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("image 缺少 {key}"))?;
    if n == 0 || n > u32::MAX as u64 {
        return Err(format!("image {key} 非法"));
    }
    Ok(n as u32)
}

/// Apply response: the new mapping that holds the redrawn full-resolution
/// image. Host opens it before closing the pipe.
pub fn ok_image(name: &str, width: u32, height: u32) -> String {
    json!({
        "protocol": PROTOCOL,
        "ok": true,
        "image": {
            "name": name,
            "width": width,
            "height": height,
            "stride": width * 4,
        },
    })
    .to_string()
}

/// Cancel (Esc / X button) response.
pub fn cancel_response() -> String {
    json!({"protocol": PROTOCOL, "ok": false, "error": "用户取消"}).to_string()
}

/// Hard failure response (bad request, mapping missing, window failed...).
pub fn error_response(msg: &str) -> String {
    json!({"protocol": PROTOCOL, "ok": false, "error": msg}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> String {
        r#"{
            "protocol": 1,
            "id": "com.rupix.doodle",
            "action": "run",
            "image_path": "C:\\图片\\a.png",
            "image": {"name": "Local\\rupix-ext-abc", "width": 800, "height": 600, "stride": 3200},
            "args": []
        }"#
        .to_string()
    }

    #[test]
    fn parses_full_run_request() {
        let req = parse_request(sample_request().as_bytes()).expect("valid request");
        assert_eq!(req.id, "com.rupix.doodle");
        assert_eq!(req.image_path.as_deref(), Some("C:\\图片\\a.png"));
        let img = req.image.expect("image present");
        assert_eq!(img.name, "Local\\rupix-ext-abc");
        assert_eq!(img.width, 800);
        assert_eq!(img.height, 600);
        assert_eq!(img.stride, 3200);
        assert!(req.args.is_empty());
    }

    #[test]
    fn tolerates_missing_optional_fields() {
        let body = br#"{"protocol":1,"action":"run","id":"x"}"#;
        let req = parse_request(body).expect("minimal request");
        assert_eq!(req.id, "x");
        assert!(req.image.is_none());
        assert!(req.image_path.is_none());
        assert!(req.args.is_empty());
    }

    #[test]
    fn rejects_wrong_protocol_and_action() {
        let bad = br#"{"protocol":2,"action":"run","id":"x"}"#;
        assert!(parse_request(bad).is_err());
        let bad = br#"{"protocol":1,"action":"shutdown","id":"x"}"#;
        assert!(parse_request(bad).is_err());
        assert!(parse_request(b"not json").is_err());
        assert!(parse_request(b"[1,2]").is_err());
    }

    #[test]
    fn rejects_bogus_image_fields() {
        let cases = [
            r#"{"protocol":1,"action":"run","id":"x","image":{"name":"C:\\evil","width":4,"height":1,"stride":16}}"#,
            r#"{"protocol":1,"action":"run","id":"x","image":{"name":"Local\\rupix-ext-a","width":0,"height":1,"stride":4}}"#,
            r#"{"protocol":1,"action":"run","id":"x","image":{"name":"Local\\rupix-ext-a","width":4,"height":1,"stride":8}}"#,
            r#"{"protocol":1,"action":"run","id":"x","image":{"name":"Local\\rupix-ext-a","width":4,"height":1}}"#,
        ];
        for case in cases {
            assert!(parse_request(case.as_bytes()).is_err(), "{case}");
        }
    }

    #[test]
    fn response_shapes_match_contract() {
        // serde_json sorts object keys, so assert on parsed fields, not the
        // exact serialized text.
        let ok: Value = serde_json::from_str(&ok_image("Local\\rupix-ext-doodle-1-2", 800, 600))
            .expect("ok response must be valid JSON");
        assert_eq!(ok.get("protocol").and_then(Value::as_u64), Some(1));
        assert_eq!(ok.get("ok").and_then(Value::as_bool), Some(true));
        let img = ok
            .get("image")
            .and_then(Value::as_object)
            .expect("ok response carries image");
        assert_eq!(
            img.get("name").and_then(Value::as_str),
            Some("Local\\rupix-ext-doodle-1-2")
        );
        assert_eq!(img.get("width").and_then(Value::as_u64), Some(800));
        assert_eq!(img.get("height").and_then(Value::as_u64), Some(600));
        assert_eq!(img.get("stride").and_then(Value::as_u64), Some(3200));

        let cancel: Value =
            serde_json::from_str(&cancel_response()).expect("cancel response must be valid JSON");
        assert_eq!(cancel.get("protocol").and_then(Value::as_u64), Some(1));
        assert_eq!(cancel.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(cancel.get("error").and_then(Value::as_str), Some("用户取消"));

        let err: Value = serde_json::from_str(&error_response("共享内存打开失败"))
            .expect("error response must be valid JSON");
        assert_eq!(err.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(err.get("error").and_then(Value::as_str), Some("共享内存打开失败"));
    }
}
