//! RuPix 图片体检插件（`kind=command`）。
//!
//! 数据来源只有一条：宿主在请求里给出的共享内存整图（协议 v1 §3 的 `image` + §5 的布局）。
//! 本插件**不解码文件、不联网、不开窗口、不写任何地方**——图进、Markdown 出。
//!
//! 线格式：`u32 LE 长度 + JSON`（一请求一响应），与 `protocol/protocol-v1.md` 一致。

use std::fs::OpenOptions;
use std::io::{Read, Write};

use serde_json::{json, Value};

mod analysis;
mod shm;

use analysis::{report, PALETTE_CHOICES, PALETTE_DEFAULT};
use shm::read_mapping;

const MAX_FRAME: usize = 16 * 1024 * 1024;

fn main() {
    if let Err(e) = run() {
        eprintln!("rupix checkup plugin: {e}");
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
    let (want_colors, palette_arg_ok) = palette_choice(&args);

    let mut stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(|e| format!("open pipe failed: {e}"))?;
    let request = read_frame(&mut stream)?;
    let value: Value =
        serde_json::from_slice(&request).map_err(|e| format!("request JSON: {e}"))?;

    // 设置里选了色数就用它（宿主设置窗写的就是这一格）；没选/不合法回落 `--colors`。
    let want = settings_palette(&value).unwrap_or(want_colors);
    let response = build_response(&value, want, palette_arg_ok);
    let bytes = serde_json::to_vec(&response).map_err(|e| format!("response JSON: {e}"))?;
    write_frame(&mut stream, &bytes)
}

/// 请求 → 响应。**独立成函数**是为了让"没给像素""未知 action"这两条分支能被单测
/// 直接命中——它们是这条通道最容易出事故的两格，不该等真机冒烟才发现。
fn build_response(request: &Value, want_colors: usize, palette_arg_ok: bool) -> Value {
    // 协议要求：认不出的 action 必须如实报错，不许当 `run` 悄悄处理。
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
             超上限的图请换小一档的画幅再看，宿主的下载清单里有更省内存的解码档。"});
    };
    let img = match read_mapping(name) {
        Ok(img) => img,
        Err(e) => return json!({"protocol":1,"ok":false,"error":e}),
    };
    json!({"protocol":1,"ok":true,"text":report(&img, want_colors, palette_arg_ok)})
}

/// 宿主下发设置里的色数（设置窗的「主色板色数」那一格）。
fn settings_palette(request: &Value) -> Option<usize> {
    let raw = request.get("settings")?.get("palette")?.as_str()?;
    let n = raw.parse::<usize>().ok()?;
    PALETTE_CHOICES.contains(&n).then_some(n)
}

/// `--colors` 只认表里的档位；其余一律回落默认，并带回"参数不合法"这个旗子，
/// 由报告自己声明用了哪一档——就近取一档会让用户以为自己设的是 12。
fn palette_choice(args: &[String]) -> (usize, bool) {
    let Some(raw) = arg_value(args, "--colors") else {
        return (PALETTE_DEFAULT, true);
    };
    match raw.parse::<usize>() {
        Ok(n) if PALETTE_CHOICES.contains(&n) => (n, true),
        _ => (PALETTE_DEFAULT, false),
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

    fn frame(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).expect("fixture must serialize")
    }

    #[test]
    fn reads_both_arg_forms_for_the_palette() {
        let args = vec![
            "checkup.exe".to_string(),
            "--colors".to_string(),
            "16".to_string(),
            "--rupix-pipe=\\\\.\\pipe\\rupix-ext-1".to_string(),
        ];
        assert_eq!(arg_value(&args, "--colors"), Some("16"));
        assert_eq!(
            arg_value(&args, "--rupix-pipe"),
            Some("\\\\.\\pipe\\rupix-ext-1")
        );
        assert_eq!(palette_choice(&args), (16, true));
    }

    #[test]
    fn palette_arg_falls_back_and_flags_itself() {
        let off = vec!["checkup.exe".to_string(), "--colors=12".to_string()];
        assert_eq!(palette_choice(&off), (PALETTE_DEFAULT, false));
        let junk = vec!["checkup.exe".to_string(), "--colors=big".to_string()];
        assert_eq!(palette_choice(&junk), (PALETTE_DEFAULT, false));
        // 没给参数 = 默认档，且不算"参数不合法"。
        assert_eq!(palette_choice(&["checkup.exe".to_string()]), (8, true));
    }

    #[test]
    fn missing_pixel_block_is_an_error_not_a_silent_empty_report() {
        // 超上限 / 老宿主：只有路径，没有 image。必须把原因说出来。
        let req = json!({"protocol":1,"id":"checkup","image_path":"C:\\a.png"});
        let r = build_response(&req, 8, true);
        assert_eq!(r["ok"], json!(false));
        let err = r["error"].as_str().unwrap_or_default();
        assert!(err.contains("image 字段缺席"), "{err}");
        assert!(err.contains("8 MP"), "{err}");
    }

    #[test]
    fn unknown_action_is_refused_instead_of_treated_as_run() {
        let r = build_response(&json!({"protocol":1,"action":"settings"}), 8, true);
        assert_eq!(r["ok"], json!(false));
        assert!(
            r["error"]
                .as_str()
                .unwrap_or_default()
                .contains("不支持的 action：settings"),
            "{r}"
        );
    }

    #[test]
    fn the_settings_palette_wins_over_the_command_line_arg() {
        // 宿主设置窗写的就是这一格；`--colors` 只是没有设置时的兜底。
        let req = json!({"settings": {"palette": "16"}});
        assert_eq!(settings_palette(&req), Some(16));
        // 不认识的档位不许被当成"就近取一档"。
        let req = json!({"settings": {"palette": "12"}});
        assert_eq!(settings_palette(&req), None);
        // 没这一格（老宿主 / 用户没装设置）就不挡路。
        assert_eq!(settings_palette(&json!({"action":"run"})), None);
    }

    #[test]
    fn frames_round_trip_through_the_length_prefix() {
        let mut buf = Vec::new();
        let body = frame(&json!({"protocol":1,"ok":true,"text":"体检"}));
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
        // 头四字节写"比上限还大"：直接报错，不许先 vec![0; len] 把内存吃掉。
        let mut buf = (MAX_FRAME as u32 + 1).to_le_bytes().to_vec();
        buf.extend_from_slice(b"x");
        let err = read_frame(&mut std::io::Cursor::new(&buf[..])).unwrap_err();
        assert!(err.contains("frame too large"), "{err}");
    }
}
