//! RuPix diagnostic plugin: connect to the host pipe, echo the request JSON.
//!
//! Wire format: u32 LE length + JSON body (see ../../protocol/protocol-v1.md).
//! This plugin has no dependencies and intentionally does not parse the full
//! request: it proves process launch, pipe handshake and frame framing.
//!
//! It also carries the **settings arm** (`settings_ui: true`): `action="settings"`
//! pops a Yes/No box and returns `{"echo.upper": "0|1"}`. The host stores it in
//! `plugins/com.rupix.echo/settings.json` and sends it back on every `run`,
//! which is what makes "gear → window → change → survives restart" testable
//! end to end with zero dependencies.

use std::fs::OpenOptions;
use std::io::{Read, Write};

const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Settings key: echo the request text in upper case.
const KEY_UPPER: &str = "echo.upper";

// Settings UI needs exactly one Win32 call; pulling in the `windows` crate just
// for MessageBoxW would dwarf this plugin. Declaring it by hand keeps the
// "zero dependency" property that makes this binary a good link probe.
#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, ty: u32) -> i32;
}

#[cfg(windows)]
const MB_YESNO: u32 = 0x0000_0004;
#[cfg(windows)]
const MB_ICONQUESTION: u32 = 0x0000_0020;
#[cfg(windows)]
const IDYES: i32 = 6;

fn main() {
    if let Err(e) = run() {
        eprintln!("rupix echo plugin: {e}");
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
        .map_err(|e| format!("open pipe {pipe} failed: {e}"))?;

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

    let request = String::from_utf8_lossy(&body);
    // Settings arm: the host only sends this action to plugins that declared
    // `settings_ui`, so a plain `run`-only plugin never has to know about it.
    let response = if json_string(&request, "action").as_deref() == Some("settings") {
        settings_response()
    } else {
        let text: String = request.chars().take(4096).collect();
        let text = if setting_on(&request, KEY_UPPER) {
            text.to_uppercase()
        } else {
            text
        };
        format!(
            "{{\"protocol\":1,\"ok\":true,\"text\":{}}}",
            quote(&format!("echo: {text}"))
        )
    };
    let bytes = response.as_bytes();
    stream
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .and_then(|()| stream.write_all(bytes))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("write response failed: {e}"))?;
    Ok(())
}

/// The whole settings window: one question box, answer becomes the stored value.
///
/// The host is a dumb store — it never interprets `echo.upper`, it just sends
/// back whatever we wrote, so changing this plugin's settings shape never needs
/// a host upgrade.
fn settings_response() -> String {
    let yes = ask_yes_no("回显结果转为大写？", "回显诊断 · 设置");
    settings_body(yes)
}

/// 响应体与"窗"分开：单测不能弹窗，判据只钉协议形状。
fn settings_body(yes: bool) -> String {
    let value = if yes { "1" } else { "0" };
    format!(
        "{{\"protocol\":1,\"ok\":true,\"settings\":{{{}}}}}",
        quote(KEY_UPPER).to_string() + ":" + &quote(value)
    )
}

#[cfg(windows)]
fn ask_yes_no(question: &str, caption: &str) -> bool {
    let q = wide(question);
    let c = wide(caption);
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the
    // call; a null owner window is allowed for a standalone message box.
    let answer = unsafe {
        MessageBoxW(
            core::ptr::null_mut(),
            q.as_ptr(),
            c.as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        )
    };
    answer == IDYES
}

#[cfg(not(windows))]
fn ask_yes_no(_question: &str, _caption: &str) -> bool {
    false
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Minimal `"key":"value"` lookup — enough for `action` and flat settings.
fn json_string(text: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = text.find(&pat)? + pat.len();
    let mut out = String::new();
    let mut chars = text[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        hex.push(chars.next()?);
                    }
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

/// Is `settings.<key>` "1" / true? The host sends settings as an object of
/// scalars; only `"1"` counts as on (values arrive as strings).
fn setting_on(text: &str, key: &str) -> bool {
    let pat = format!("\"{key}\":");
    let Some(pos) = text.find(&pat) else {
        return false;
    };
    let rest = text[pos + pat.len()..].trim_start();
    if let Some(value) = rest.strip_prefix('"') {
        value.starts_with('1')
    } else {
        rest.starts_with('1') || rest.starts_with("true")
    }
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

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_arg_forms() {
        let args = vec![
            "echo.exe".to_string(),
            "--rupix-pipe".to_string(),
            "\\\\.\\pipe\\rupix-ext-test".to_string(),
            "--rupix-protocol=1".to_string(),
        ];
        assert_eq!(
            arg_value(&args, "--rupix-pipe"),
            Some("\\\\.\\pipe\\rupix-ext-test")
        );
        assert_eq!(arg_value(&args, "--rupix-protocol"), Some("1"));
    }

    #[test]
    fn quotes_json_control_chars() {
        assert_eq!(quote("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[test]
    fn reads_action_and_settings() {
        let req = r#"{"protocol":1,"id":"com.rupix.echo","action":"settings","settings":{"echo.upper":"1"}}"#;
        assert_eq!(json_string(req, "action").as_deref(), Some("settings"));
        assert!(setting_on(req, KEY_UPPER));

        let run = r#"{"protocol":1,"id":"com.rupix.echo","action":"run","settings":{"echo.upper":"0"}}"#;
        assert_eq!(json_string(run, "action").as_deref(), Some("run"));
        assert!(!setting_on(run, KEY_UPPER));

        // 没带设置区（老宿主 / 没声明读权限）= 关，不是错误。
        assert!(!setting_on(r#"{"action":"run"}"#, KEY_UPPER));
    }

    #[test]
    fn settings_response_carries_the_key() {
        let body = settings_body(false);
        assert!(body.contains("\"ok\":true"), "{body}");
        assert!(body.contains("\"echo.upper\":\"0\""), "{body}");
    }
}
