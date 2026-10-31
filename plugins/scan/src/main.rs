//! RuPix 条码识别插件（`kind=command`）。
//!
//! 数据来源只有一条：宿主在请求里给的共享内存整图（协议 v1 §3 的 `image` + §5 的布局）。
//! **不解码文件、不联网、不开窗口、不写任何地方，也不执行载荷**——图进、清单出。
//!
//! 线格式：`u32 LE 长度 + JSON`（一请求一响应），与 `protocol/protocol-v1.md` 一致。

use std::fs::OpenOptions;
use std::io::{Read, Write};

use serde_json::{json, Value};

mod scan;
mod shm;

use scan::{find_codes, prepare, report, SCAN_MAX_PIXELS};
use shm::read_mapping;

const MAX_FRAME: usize = 16 * 1024 * 1024;

fn main() {
    if let Err(e) = run() {
        eprintln!("rupix scan plugin: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let pipe = arg_value(&args, "--rupix-pipe").ok_or("missing --rupix-pipe")?;
    let protocol = arg_value(&args, "--rupix-protocol").unwrap_or("1");
    if protocol != "1" {
        return Err(format!("unsupported protocol {protocol}"));
    }

    let mut stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(|e| format!("open pipe failed: {e}"))?;
    let request = read_frame(&mut stream)?;
    let value: Value =
        serde_json::from_slice(&request).map_err(|e| format!("request JSON: {e}"))?;

    let response = build_response(&value);
    let bytes = serde_json::to_vec(&response).map_err(|e| format!("response JSON: {e}"))?;
    write_frame(&mut stream, &bytes)
}

/// 请求 → 响应。独立成函数是为了让"没给像素""未知 action"这两条分支能被单测直接命中。
fn build_response(request: &Value) -> Value {
    if let Some(action) = request.get("action").and_then(Value::as_str) {
        if action != "run" {
            return json!({"protocol":1,"ok":false,"error":format!("不支持的 action：{action}")});
        }
    }
    let name = request
        .get("image")
        .and_then(|i| i.get("name"))
        .and_then(Value::as_str);
    let Some(name) = name else {
        return json!({"protocol":1,"ok":false,"error":
            "请求里没有整图像素（image 字段缺席）。宿主只在两种情况下下发：\
             本插件声明了 image.read，且当前整图不超过宿主的编辑档上限（8 MP）。\
             条码不需要 8 MP——把图裁近一点再扫，识别率反而更高。"});
    };
    let img = match read_mapping(name) {
        Ok(img) => img,
        Err(e) => return json!({"protocol":1,"ok":false,"error":e}),
    };
    let field = prepare(&img, SCAN_MAX_PIXELS);
    let findings = find_codes(&field);
    let (open_url, open_image) = scan::open_request(
        &findings,
        setting_on(request, "open_url"),
        setting_on(request, "open_image"),
    );
    let mut resp = json!({"protocol":1,"ok":true,"text":report(&findings, &field)});
    if let Some(url) = open_url {
        resp["open_url"] = json!(url);
    }
    if let Some((ext, data)) = open_image {
        resp["open_image"] = json!({"ext": ext, "base64": data});
    }
    resp
}

/// 宿主下发设置里的一个布尔项（`"1"` = 勾上）。
///
/// 没声明 `settings.read` 的插件收不到 `settings`；老宿主也不发——两种情况都当"没勾"。
fn setting_on(request: &Value, key: &str) -> bool {
    request
        .get("settings")
        .and_then(|s| s.get(key))
        .and_then(Value::as_str)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn read_frame(stream: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut len = [0u8; 4];
    stream
        .read_exact(&mut len)
        .map_err(|e| format!("read length failed: {e}"))?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(format!("frame too large: {len}"));
    }
    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("read body failed: {e}"))?;
    Ok(body)
}

fn write_frame(stream: &mut impl Write, body: &[u8]) -> Result<(), String> {
    stream
        .write_all(&(body.len() as u32).to_le_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("write response failed: {e}"))
}

fn arg_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == key {
            return it.next().map(String::as_str);
        }
        if let Some(v) = arg.strip_prefix(key).and_then(|s| s.strip_prefix('=')) {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_flip_the_two_open_switches() {
        let on = json!({"protocol":1,"settings":{"open_url":"1","open_image":"true"}});
        assert!(setting_on(&on, "open_url"));
        assert!(setting_on(&on, "open_image"));
        let off = json!({"protocol":1,"settings":{"open_url":"0"}});
        assert!(!setting_on(&off, "open_url"));
        assert!(!setting_on(&off, "open_image"), "缺的项按没勾处理");
        assert!(
            !setting_on(&json!({"protocol":1}), "open_url"),
            "老宿主没有 settings"
        );
    }

    #[test]
    fn reads_both_arg_forms() {
        let args = vec![
            "scan.exe".to_string(),
            "--rupix-pipe".to_string(),
            "\\\\.\\pipe\\rupix-ext-1".to_string(),
            "--rupix-protocol=1".to_string(),
        ];
        assert_eq!(
            arg_value(&args, "--rupix-pipe"),
            Some("\\\\.\\pipe\\rupix-ext-1")
        );
        assert_eq!(arg_value(&args, "--rupix-protocol"), Some("1"));
    }

    #[test]
    fn missing_pixels_are_an_explained_failure_not_an_empty_scan() {
        // 只有路径没有像素 = 超上限或宿主太旧。这时候"未识别到条码"是谎话，
        // 因为压根没扫过。
        let r = build_response(&json!({"protocol":1,"image_path":"C:\\a.png"}));
        assert_eq!(r["ok"], json!(false));
        let err = r["error"].as_str().unwrap_or_default();
        assert!(err.contains("image 字段缺席"), "{err}");
        assert!(err.contains("裁近"), "要给一条能照做的下一步：{err}");
    }

    #[test]
    fn unknown_action_is_refused() {
        let r = build_response(&json!({"protocol":1,"action":"settings"}));
        assert_eq!(r["ok"], json!(false));
        assert!(r["error"]
            .as_str()
            .unwrap_or_default()
            .contains("不支持的 action：settings"));
    }

    #[test]
    fn a_mapping_name_that_is_not_there_fails_cleanly() {
        // 名字对不上（宿主已回收映射）：报错误，不报"没找到码"。
        let r = build_response(
            &json!({"protocol":1,"image":{"name":"Local\\rupix-ext-nope","width":2,"height":2,"stride":8}}),
        );
        assert_eq!(r["ok"], json!(false));
        assert!(r["error"]
            .as_str()
            .unwrap_or_default()
            .contains("打开共享内存失败"));
    }

    #[test]
    fn frames_round_trip_and_a_lying_length_is_refused() {
        let mut buf = Vec::new();
        write_frame(&mut buf, "清单".as_bytes()).expect("write must work");
        let back = read_frame(&mut std::io::Cursor::new(&buf[..])).expect("read must work");
        assert_eq!(back, "清单".as_bytes());

        let mut bad = (MAX_FRAME as u32 + 1).to_le_bytes().to_vec();
        bad.extend_from_slice(b"x");
        assert!(read_frame(&mut std::io::Cursor::new(&bad[..]))
            .unwrap_err()
            .contains("frame too large"));
    }
}
