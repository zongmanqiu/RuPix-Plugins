//! RuPix 色阶分析插件（`kind=command`，id=`levels`）。
//!
//! 图进（共享内存 RGBA8）→ 四组 256 箱统计 → 文本报告 + 一张自己画的 PNG。
//! **不解码文件、不联网、不开窗口、不写任何地方**；PNG 只作为响应里的
//! `open_image` 字节交回宿主（协议 v1 §4），落盘与打开都由宿主做。
//!
//! 线格式：`u32 LE 长度 + JSON`（一请求一响应），与 `protocol/protocol-v1.md` 一致。

use std::fs::OpenOptions;
use std::io::{Read, Write};

use serde_json::{json, Value};

mod hist;
mod png;
mod shm;

use hist::Histogram;
use shm::read_mapping;

const MAX_FRAME: usize = 16 * 1024 * 1024;

fn main() {
    if let Err(e) = run() {
        eprintln!("rupix levels plugin: {e}");
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

    let want_image = wants_image(&value);
    let response = build_response(&value, want_image);
    let bytes = serde_json::to_vec(&response).map_err(|e| format!("response JSON: {e}"))?;
    write_frame(&mut stream, &bytes)
}

/// 请求 → 响应。独立成函数，好让"没给像素""未知 action""关掉图形"这几格
/// 能被单测直接命中，不必等真机冒烟。
fn build_response(request: &Value, want_image: bool) -> Value {
    // 协议要求：认不出的 action 必须如实报错，不许当 `run` 悄悄处理。
    if let Some(action) = request.get("action").and_then(Value::as_str) {
        if action != "run" {
            return json!({"protocol":1,"ok":false,"error":format!("不支持的 action：{action}")});
        }
    }
    let Some(name) = request
        .get("image")
        .and_then(|i| i.get("name"))
        .and_then(Value::as_str)
    else {
        return json!({"protocol":1,"ok":false,"error":
            "请求里没有整图像素（image 字段缺席）。只可能是这三种情况：\
             ① 插件没声明 image.read；② 当前是切片态（区域渲染），宿主手里没有整图；\
             ③ 这张图超过 8 MP，而宿主不认识本插件 0.1.7 起声明的 image.sampled\
             （老宿主会退回「只发路径」）。前两种无解，第三种升级看图程序即可。"});
    };
    let img = match read_mapping(name) {
        Ok(img) => img,
        Err(e) => return json!({"protocol":1,"ok":false,"error":e}),
    };
    let h = Histogram::accumulate(&img);
    if h.counted == 0 {
        return json!({"protocol":1,"ok":false,"error":
            "整图没有可见像素（全部 alpha=0），没有明暗分布可统计。"});
    }
    let mut resp = json!({"protocol":1,"ok":true,"text":h.report_with(want_image, input_of(request))});
    if want_image {
        let (ext, b64) = chart_payload(&h);
        resp["open_image"] = json!({"ext": ext, "base64": b64});
    }
    resp
}

/// 这份像素的来路：宿主在请求里给 `source` 与 `sample_step`。
///
/// 两个字段都是**可选**的（协议加法式）：老宿主一个都不给 ⇒ [`hist::Input::FULL`]，
/// 报文与 0.1.6 逐字相同。超 8 MP 的图宿主会抽样下发，那时两个字段都在。
fn input_of(request: &Value) -> hist::Input {
    let source = request.get("source").and_then(|s| {
        let w = s.get("width")?.as_u64()?;
        let h = s.get("height")?.as_u64()?;
        (w > 0 && h > 0).then_some((w as u32, h as u32))
    });
    let step = request
        .get("sample_step")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as u32;
    if source.is_none() && step <= 1 {
        // 老宿主 / 未抽样：走与 0.1.6 逐字相同的那条报文。
        return hist::Input::FULL;
    }
    hist::Input { source, step }
}

/// 画图那一腿单独留口：单测直接验它回的是 PNG 而不是别的序列化产物。
fn chart_payload(h: &Histogram) -> (String, String) {
    let bytes = png::encode_png(&png::draw_chart(h));
    ("PNG".to_string(), png::base64(&bytes))
}

/// 设置里的 `open_image` 开关。宿主声明式设置窗存的可能是 bool / 字符串 / 数字，
/// 三种都认；**没这一格时默认开**——这枚插件的价值一半在那张图上。
fn wants_image(request: &Value) -> bool {
    let Some(v) = request.get("settings").and_then(|s| s.get("open_image")) else {
        return true;
    };
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(1.0) != 0.0,
        Value::String(s) => !matches!(s.as_str(), "0" | "false" | "False" | "off" | ""),
        _ => true,
    }
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
    use crate::shm::Raster;

    #[test]
    fn missing_pixel_block_is_an_error_not_a_silent_empty_chart() {
        let req = json!({"protocol":1,"id":"levels","image_path":"C:\\a.png"});
        let r = build_response(&req, true);
        assert_eq!(r["ok"], json!(false));
        assert!(r["error"].as_str().unwrap_or_default().contains("image 字段缺席"));
        assert!(r.get("open_image").is_none(), "失败不许顺手回一张空图");
    }

    #[test]
    fn unknown_action_is_refused_instead_of_treated_as_run() {
        let r = build_response(&json!({"protocol":1,"action":"settings"}), true);
        assert_eq!(r["ok"], json!(false));
        assert!(r["error"]
            .as_str()
            .unwrap_or_default()
            .contains("不支持的 action：settings"));
    }

    #[test]
    fn the_image_switch_accepts_every_shape_the_host_may_store() {
        let setting = |v: Value| json!({"settings": {"open_image": v}});
        assert!(wants_image(&json!({})), "没这一格默认出图");
        assert!(wants_image(&setting(json!(true))));
        assert!(!wants_image(&setting(json!(false))));
        assert!(!wants_image(&setting(json!("0"))));
        assert!(wants_image(&setting(json!("1"))));
        assert!(!wants_image(&setting(json!(0))));
        assert!(wants_image(&setting(json!(1))));
    }

    #[test]
    fn chart_payload_really_is_a_png() {
        let h = Histogram::accumulate(&Raster {
            width: 2,
            height: 2,
            rgba: vec![255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255],
        });
        let (ext, b64) = chart_payload(&h);
        assert_eq!(ext, "PNG");
        assert!(
            b64.starts_with("iVBORw0KGgo"),
            "base64 必须以 PNG 签名开头：{}",
            &b64[..b64.len().min(24)]
        );
        assert_eq!(b64.len() % 4, 0, "base64 长度必须补齐到 4 的倍数");
        assert!(b64.bytes().all(|b| b.is_ascii_alphanumeric()
            || b == b'+'
            || b == b'/'
            || b == b'='));
    }

    #[test]
    fn frames_round_trip_through_the_length_prefix() {
        let mut buf = Vec::new();
        let body = serde_json::to_vec(&json!({"protocol":1,"ok":true,"text":"色阶分析"})).unwrap();
        write_frame(&mut buf, &body).expect("write must work");
        assert_eq!(
            u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize,
            body.len()
        );
        let back = read_frame(&mut std::io::Cursor::new(&buf[..])).expect("read must work");
        assert_eq!(back, body);
    }

    #[test]
    fn a_lying_frame_length_is_refused_before_allocating() {
        let mut buf = (MAX_FRAME as u32 + 1).to_le_bytes().to_vec();
        buf.extend_from_slice(b"x");
        let err = read_frame(&mut std::io::Cursor::new(&buf[..])).unwrap_err();
        assert!(err.contains("frame too large"));
    }

    #[test]
    fn arg_value_reads_both_forms() {
        let args = vec![
            "levels.exe".to_string(),
            "--rupix-pipe".to_string(),
            "\\\\.\\pipe\\rupix-ext-1".to_string(),
            "--rupix-protocol=1".to_string(),
        ];
        assert_eq!(arg_value(&args, "--rupix-pipe"), Some("\\\\.\\pipe\\rupix-ext-1"));
        assert_eq!(arg_value(&args, "--rupix-protocol"), Some("1"));
    }
}
