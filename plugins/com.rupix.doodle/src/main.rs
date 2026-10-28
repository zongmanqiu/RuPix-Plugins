//! RuPix doodle editor plugin: paint on a preview, apply redraws at full
//! resolution (protocol v1, see ../../protocol/protocol-v1.md).
//!
//! Flow: connect the host pipe -> read one request frame -> open the host's
//! read-only shared memory with the full-resolution RGBA image -> run the
//! editor window -> on apply, publish the redrawn image as a NEW shared memory
//! and answer with its name; then block until the host closes the pipe. The
//! host copies the pixels before closing, so exiting early would destroy the
//! mapping.

mod protocol;
mod raster;
mod shm;
mod ui;

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::protocol::{
    cancel_response, error_response, ok_image, parse_request, MAX_FRAME,
};

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rupix doodle plugin: {e}");
            2
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32, String> {
    let args: Vec<String> = std::env::args().collect();
    let pipe = arg_value(&args, "--rupix-pipe").ok_or("missing --rupix-pipe")?;
    let proto = arg_value(&args, "--rupix-protocol").unwrap_or("1");
    if proto != "1" {
        return Err(format!("unsupported protocol {proto}"));
    }

    let mut stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(|e| format!("open pipe {pipe} failed: {e}"))?;

    match session(&mut stream) {
        Ok(code) => Ok(code),
        Err(e) => {
            // Tell the host why the session failed, then wait for it to close
            // the pipe before exiting (same lifetime rule as the ok path).
            let resp = error_response(&e);
            let _ = write_frame(&mut stream, resp.as_bytes());
            drain(&mut stream);
            Err(e)
        }
    }
}

fn session(stream: &mut std::fs::File) -> Result<i32, String> {
    let body = read_frame(stream)?;
    let request = parse_request(&body)?;
    let image = request
        .image
        .as_ref()
        .ok_or("请求缺少 image（共享内存）")?;
    let img = shm::read_input(&image.name)?;
    if img.width != image.width || img.height != image.height {
        return Err("image 尺寸与共享内存头不一致".into());
    }

    match ui::run_editor(img)? {
        ui::Outcome::Applied { pixels } => {
            let name = shm::output_name(std::process::id(), unix_millis());
            // Keep the mapping alive until the host closed the pipe.
            let guard = shm::publish_output(&name, image.width, image.height, &pixels)?;
            let response = ok_image(&name, image.width, image.height);
            write_frame(stream, response.as_bytes())?;
            // The host opens our mapping and copies the pixels BEFORE closing
            // the pipe; whoever exits first destroys the mapping.
            drain(stream);
            drop(guard);
            Ok(0)
        }
        ui::Outcome::Cancelled => {
            write_frame(stream, cancel_response().as_bytes())?;
            drain(stream);
            Ok(0)
        }
    }
}

fn read_frame(stream: &mut std::fs::File) -> Result<Vec<u8>, String> {
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

fn write_frame(stream: &mut std::fs::File, body: &[u8]) -> Result<(), String> {
    if body.len() > MAX_FRAME {
        return Err(format!("frame too large: {}", body.len()));
    }
    stream
        .write_all(&(body.len() as u32).to_le_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("write response failed: {e}"))
}

/// Read until the host closes the pipe (EOF or error). This is the
/// "keep the mapping alive" wait demanded by the editor protocol.
fn drain(stream: &mut std::fs::File) {
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
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
    fn parses_both_arg_forms() {
        let args = vec![
            "doodle.exe".to_string(),
            "--rupix-pipe".to_string(),
            "\\\\.\\pipe\\rupix-ext-test".to_string(),
            "--rupix-protocol=1".to_string(),
        ];
        assert_eq!(
            arg_value(&args, "--rupix-pipe"),
            Some("\\\\.\\pipe\\rupix-ext-test")
        );
        assert_eq!(arg_value(&args, "--rupix-protocol"), Some("1"));
        assert_eq!(arg_value(&args, "--missing"), None);
    }

    #[test]
    fn output_name_includes_pid_and_millis() {
        let name = shm::output_name(1234, 1728000000000);
        assert!(name.starts_with("Local\\rupix-ext-doodle-1234-"));
    }
}
