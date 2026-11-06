//! channels: per-channel gain/bias colour editor (`kind=editor`, protocol v1).
//!
//! Reads the host's shared-memory image, applies the settings-driven transform,
//! creates a NEW mapping and replies with its ImageRef. Then it blocks reading the
//! pipe until the host closes it (protocol v1 section 7 timing rule).
//!
//! Transform: `out = clamp(in * gain/100 + bias)` per RGB channel; alpha and stride
//! padding are untouched.
//!
//! The FILTER PRESETS now live in manifest.json (`presets`): the host writes their six
//! numbers into the sliders, so every preset reaches this binary as plain `settings`
//! values. The two legacy names below are kept only for old settings.json files that
//! still carry `preset=invert` / `preset=original` and no six numbers.
//!   invert   = gain -100 / bias 255  => 255 - in
//!   original = gain  100 / bias 0

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

mod shm;

use shm::{create_mapping, read_mapping, OwnedMapping};

const MAX_FRAME: usize = 16 * 1024 * 1024;
const GAIN_MIN: i32 = -400;
const GAIN_MAX: i32 = 400;
const BIAS_MIN: i32 = -255;
const BIAS_MAX: i32 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Channel {
    gain: i32,
    bias: i32,
}

impl Channel {
    const NEUTRAL: Channel = Channel { gain: 100, bias: 0 };
    const INVERT: Channel = Channel {
        gain: -100,
        bias: 255,
    };

    fn apply(self, v: u8) -> u8 {
        let n = v as i64 * self.gain as i64 / 100 + self.bias as i64;
        n.clamp(0, 255) as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preset {
    Custom,
    Invert,
    Original,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Params {
    r: Channel,
    g: Channel,
    b: Channel,
    preset: Preset,
}

impl Params {
    const DEFAULT: Params = Params {
        r: Channel::NEUTRAL,
        g: Channel::NEUTRAL,
        b: Channel::NEUTRAL,
        preset: Preset::Custom,
    };
}

fn int_setting(settings: Option<&Value>, key: &str, fallback: i32, lo: i32, hi: i32) -> i32 {
    let parsed = settings
        .and_then(|s| s.get(key))
        .and_then(|v| match v {
            Value::String(t) => t.parse::<i64>().ok(),
            Value::Number(n) => n.as_i64(),
            _ => None,
        })
        .unwrap_or(fallback as i64);
    (parsed as i32).clamp(lo, hi)
}

fn parse_params(settings: Option<&Value>) -> Params {
    let preset = match settings
        .and_then(|s| s.get("preset"))
        .and_then(Value::as_str)
        .unwrap_or("custom")
    {
        "invert" => Preset::Invert,
        "original" => Preset::Original,
        _ => Preset::Custom,
    };
    let mut p = Params::DEFAULT;
    p.preset = preset;
    p.r.gain = int_setting(settings, "r_gain", 100, GAIN_MIN, GAIN_MAX);
    p.g.gain = int_setting(settings, "g_gain", 100, GAIN_MIN, GAIN_MAX);
    p.b.gain = int_setting(settings, "b_gain", 100, GAIN_MIN, GAIN_MAX);
    p.r.bias = int_setting(settings, "r_bias", 0, BIAS_MIN, BIAS_MAX);
    p.g.bias = int_setting(settings, "g_bias", 0, BIAS_MIN, BIAS_MAX);
    p.b.bias = int_setting(settings, "b_bias", 0, BIAS_MIN, BIAS_MAX);
    match preset {
        Preset::Invert => {
            p.r = Channel::INVERT;
            p.g = Channel::INVERT;
            p.b = Channel::INVERT;
        }
        Preset::Original => {
            p.r = Channel::NEUTRAL;
            p.g = Channel::NEUTRAL;
            p.b = Channel::NEUTRAL;
        }
        Preset::Custom => {}
    }
    p
}

/// A preset is just a numeric group: write those numbers back so the settings
/// window can show them and the user can keep tuning from there.
fn preset_numbers(p: Params) -> Option<Value> {
    if p.preset == Preset::Custom {
        return None;
    }
    Some(json!({
        "r_gain": p.r.gain.to_string(),
        "g_gain": p.g.gain.to_string(),
        "b_gain": p.b.gain.to_string(),
        "r_bias": p.r.bias.to_string(),
        "g_bias": p.g.bias.to_string(),
        "b_bias": p.b.bias.to_string(),
    }))
}

/// Only the `width*4` real pixels of each row change: stride padding and alpha
/// are copied byte-for-byte from the host mapping.
fn transform(src: &[u8], width: u32, height: u32, stride: u32, p: Params) -> Vec<u8> {
    let mut out = src.to_vec();
    let row = stride as usize;
    let usable = width as usize * 4;
    for y in 0..height as usize {
        let base = y * row;
        for x in (0..usable).step_by(4) {
            let i = base + x;
            out[i] = p.r.apply(src[i]);
            out[i + 1] = p.g.apply(src[i + 1]);
            out[i + 2] = p.b.apply(src[i + 2]);
        }
    }
    out
}

fn main() {
    if let Err(e) = run() {
        eprintln!("channels: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let pipe = arg_value(&args, "--rupix-pipe").ok_or("missing --rupix-pipe")?;
    let protocol = arg_value(&args, "--rupix-protocol").unwrap_or_else(|| "1".into());
    if protocol != "1" {
        return Err(format!("unsupported protocol {protocol}"));
    }

    let mut stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe)
        .map_err(|e| format!("open pipe failed: {e}"))?;
    let request = read_frame(&mut stream)?;
    let value: Value =
        serde_json::from_slice(&request).map_err(|e| format!("request JSON: {e}"))?;
    let (response, owned) = build_response(&value)?;
    let bytes = serde_json::to_vec(&response).map_err(|e| format!("response JSON: {e}"))?;
    write_frame(&mut stream, &bytes)?;

    // Protocol v1 section 7: keep our reply mapping alive until the host closes
    // the pipe. Whoever exits first destroys the mapping.
    let mut sink = [0u8; 64];
    loop {
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(_) => continue,
        }
    }
    drop(owned);
    Ok(())
}

fn build_response(request: &Value) -> Result<(Value, Option<OwnedMapping>), String> {
    if let Some(action) = request.get("action").and_then(Value::as_str) {
        if action != "run" {
            return Ok((
                json!({"protocol":1,"ok":false,"error":"unsupported action"}),
                None,
            ));
        }
    }
    let params = parse_params(request.get("settings"));
    let name = request
        .get("image")
        .and_then(|i| i.get("name"))
        .and_then(Value::as_str)
        .ok_or("request has no image.name")?;
    let raster = read_mapping(name).map_err(|e| format!("read host mapping: {e}"))?;
    let rgba = transform(
        &raster.rgba,
        raster.width,
        raster.height,
        raster.stride,
        params,
    );
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let reply_name = format!("Local\\rupix-ext-channels-{}-{stamp}", std::process::id());
    let owned = create_mapping(
        &reply_name,
        raster.width,
        raster.height,
        raster.stride,
        &rgba,
    )
    .map_err(|e| format!("create reply mapping: {e}"))?;

    let mut response = json!({
        "protocol": 1,
        "ok": true,
        "image": {
            "name": reply_name,
            "width": raster.width,
            "height": raster.height,
            "stride": raster.stride
        }
    });
    if let Some(numbers) = preset_numbers(params) {
        response["settings"] = numbers;
    }
    Ok((response, Some(owned)))
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == key {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
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
    if body.len() > MAX_FRAME {
        return Err(format!("frame too large: {}", body.len()));
    }
    let len = (body.len() as u32).to_le_bytes();
    stream
        .write_all(&len)
        .map_err(|e| format!("write length failed: {e}"))?;
    stream
        .write_all(body)
        .map_err(|e| format!("write body failed: {e}"))?;
    stream.flush().map_err(|e| format!("flush failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(r_gain: i32, g_gain: i32, b_gain: i32, r_bias: i32) -> Params {
        Params {
            r: Channel {
                gain: r_gain,
                bias: r_bias,
            },
            g: Channel {
                gain: g_gain,
                bias: 0,
            },
            b: Channel {
                gain: b_gain,
                bias: 0,
            },
            preset: Preset::Custom,
        }
    }

    #[test]
    fn invert_is_exactly_the_numeric_group() {
        // gain -100 / bias 255 applied to 0,127,255 => 255,128,0
        let src = [0u8, 127, 255, 77];
        let p = Params {
            r: Channel::INVERT,
            g: Channel::INVERT,
            b: Channel::INVERT,
            preset: Preset::Invert,
        };
        assert_eq!(transform(&src, 1, 1, 4, p), vec![255, 128, 0, 77]);
        assert_eq!(Channel::INVERT.apply(200), 55);
    }

    #[test]
    fn original_is_byte_identical() {
        let src: Vec<u8> = (0..32).map(|i| (i * 7) as u8).collect();
        assert_eq!(transform(&src, 2, 2, 8, Params::DEFAULT), src);
    }

    #[test]
    fn per_channel_gain_and_bias_are_independent() {
        // R: 100*2 + 10 = 210; G: 50*1 = 50; B: 200*0 = 0; alpha untouched.
        let src = [100u8, 50, 200, 128];
        let p = custom(200, 100, 0, 10);
        assert_eq!(transform(&src, 1, 1, 4, p), vec![210, 50, 0, 128]);
    }

    #[test]
    fn padded_stride_and_alpha_are_copied_verbatim() {
        // 1x2 image, stride 8 (4 real bytes + 4 padding bytes 0xAA).
        let src = [
            10u8, 20, 30, 255, 0xAA, 0xAA, 0xAA, 0xAA, //
            40, 50, 60, 0, 0xAA, 0xAA, 0xAA, 0xAA,
        ];
        let out = transform(&src, 1, 2, 8, Params {
            r: Channel::INVERT,
            g: Channel::INVERT,
            b: Channel::INVERT,
            preset: Preset::Invert,
        });
        assert_eq!(out[0..4], [245, 235, 225, 255]);
        assert_eq!(out[4..8], [0xAA; 4]);
        assert_eq!(out[8..12], [215, 205, 195, 0]);
        assert_eq!(out[12..16], [0xAA; 4]);
    }

    #[test]
    fn settings_parse_custom_numbers() {
        let s = json!({
            "preset": "custom",
            "r_gain": "-100", "g_gain": "100", "b_gain": "0",
            "r_bias": "255", "g_bias": "0", "b_bias": "0"
        });
        let p = parse_params(Some(&s));
        assert_eq!(p.preset, Preset::Custom);
        assert_eq!(p.r, Channel::INVERT);
        assert_eq!(p.g, Channel::NEUTRAL);
        assert_eq!(p.b, Channel {
            gain: 0,
            bias: 0
        });
    }

    #[test]
    fn settings_clamp_out_of_range_and_bad_values() {
        let s = json!({"preset":"custom","r_gain":"99999","g_gain":"nope","b_bias":"999"});
        let p = parse_params(Some(&s));
        assert_eq!(p.r.gain, GAIN_MAX);
        assert_eq!(p.g.gain, 100);
        assert_eq!(p.b.bias, BIAS_MAX);
        assert_eq!(parse_params(None), Params::DEFAULT);
    }

    #[test]
    fn presets_report_their_numbers_back() {
        let inv = parse_params(Some(&json!({"preset":"invert"})));
        let n = preset_numbers(inv).expect("invert writes its numbers");
        assert_eq!(n["r_gain"], "-100");
        assert_eq!(n["b_bias"], "255");
        let org = parse_params(Some(&json!({"preset":"original"})));
        let n2 = preset_numbers(org).expect("original writes its numbers");
        assert_eq!(n2["r_gain"], "100");
        assert_eq!(n2["r_bias"], "0");
        assert!(preset_numbers(Params::DEFAULT).is_none(), "custom stays quiet");
    }

    #[test]
    fn unknown_action_is_rejected() {
        let (resp, owned) = build_response(&json!({"action":"settings"})).unwrap();
        assert_eq!(resp["ok"], false);
        assert!(owned.is_none());
    }

    #[test]
    fn missing_image_is_an_error() {
        assert!(build_response(&json!({"action":"run"})).is_err());
    }
}
