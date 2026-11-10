//! RuPix 离线文字识别插件（`kind=command`）。
//!
//! 数据线：宿主把**屏上朝向**的整图像素放在共享内存里（协议 v1 §3/§5）→ 本插件
//! 合成白底写成临时 PNG → 起插件目录里的 `engine/CurSeenOcr.exe` 一次识别 →
//! 把 `textLine[N](文字)` 按号拼回纯文本 → 回 `text` 由宿主自动复制进剪贴板。
//!
//! 不解码文件、不联网、不上传图片、不写插件目录以外的东西；临时 PNG 写完即删。
//! 线格式 `u32 LE 长度 + JSON`，一请求一响应。

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

mod png;
mod shm;

const MAX_FRAME: usize = 16 * 1024 * 1024;
/// 一次识别的墙钟上限（起进程 + 加载 dll/模型 + 推理）。实测单张 < 1 s，
/// 25 s 是给冷启动与慢盘留的余量；超了主动杀掉子进程，不留孤儿。
const ENGINE_TIMEOUT: Duration = Duration::from_secs(25);

const DET: &str = "ch_PP-OCRv4_det_mobile.onnx";
const CLS: &str = "ch_ppocr_mobile_v2.0_cls_mobile.onnx";

/// 只带一套 rec 模型。`ppocr_keys_v1.txt` 的 6623 字里 6270 个是 CJK 统一汉字：
/// 简体与繁体在这套 Unicode 码位里是同一批字，字典从不分语言，所以简 / 繁 / 英
/// 本来就是**一次识别同时出**，不需要也不应该做成二选一。
/// ★ 另挂一套繁体专用 rec 看似"更全"，实测会把简体 `正品促销` 输出成 `正品促銷`、
///   `划算` 认成 `切算`；换来的只是 `擊` / `螢` 这类字典外繁体字。为两个字让用户
///   先判断图片语种，是把实现约束塞进 UI。
/// ★ 字典在训练时定死（CTC 输出通道数 = 字典字符数 + blank），改 txt 加字符只会让
///   解码整体错位，所以"多认几个希腊字母 / 符号"不能靠改字典实现。
const REC: &str = "ch_PP-OCRv4_rec_mobile.onnx";
const KEYS: &str = "ppocr_keys_v1.txt";

/// 包里必须齐的五个文件。少任何一个都在**启动前**说明白，不给一个"点了必然失败"的结果。
const REQUIRED: [&str; 5] = ["CurSeenOcr.exe", DET, CLS, REC, KEYS];

/// 临时 PNG 的守卫：正常返回、报错返回、被宿主掐掉都要尽量清掉自己写的文件。
/// ★ 引擎 CLI 写死了 `isOutputResultImg=true`，且没有开关：它会在输入图旁边额外落
///   `<name>-result.jpg` 与 `<name>-result.txt`。不删的话每次识别都在插件目录留两份垃圾，
///   下一次打包还会把它们带进 ZIP（实测四份共 86 KB）。
struct TempPng(PathBuf);
impl Drop for TempPng {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let base = self.0.to_string_lossy();
        let _ = fs::remove_file(format!("{base}-result.jpg"));
        let _ = fs::remove_file(format!("{base}-result.txt"));
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("rupix ocr plugin: {e}");
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

fn engine_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("取插件路径失败：{e}"))?;
    Ok(exe
        .parent()
        .ok_or("插件路径没有父目录")?
        .join("engine"))
}

/// 请求 → 响应。独立成函数，让"没给像素 / 未知 action / 假映射"这些失败臂能被单测直接命中。
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
        // 抽样（image.sampled）会把笔画跳掉，所以本插件**不声明**那个能力：
        // 超 8 MP 时宿主只发 image_path，这里如实报错而不是假装"没识别出文字"。
        return json!({"protocol":1,"ok":false,"error":
            "请求里没有整图像素（image 字段缺席）。宿主只在声明了 image.read 且整图不超过 8 MP 时下发像素；\
             请把大图缩小或裁近一点再来识别。"});
    };
    let raster = match shm::read_mapping(name) {
        Ok(r) => r,
        Err(e) => return json!({"protocol":1,"ok":false,"error":e}),
    };
    let dir = match engine_dir() {
        Ok(d) => d,
        Err(e) => return json!({"protocol":1,"ok":false,"error":e}),
    };
    // 带 pid，避免同目录被多个窗口并发调用时互相覆盖。
    let tmp = dir.join(format!("rupix_ocr_{}.png", std::process::id()));
    let _guard = TempPng(tmp.clone());
    match do_ocr(&raster, &dir, &tmp) {
        Ok(text) => json!({"protocol":1,"ok":true,"text":text}),
        Err(e) => json!({"protocol":1,"ok":false,"error":e}),
    }
}

fn do_ocr(r: &shm::Raster, dir: &Path, tmp: &Path) -> Result<String, String> {
    for name in REQUIRED {
        if !dir.join(name).is_file() {
            return Err(format!(
                "引擎文件缺失：{name}（插件包没装完整，请重新安装本插件）"
            ));
        }
    }
    let bytes = png::encode_png(r);
    fs::write(tmp, &bytes).map_err(|e| format!("写临时图片失败：{e}"))?;

    let mut cmd = Command::new(dir.join("CurSeenOcr.exe"));
    cmd.current_dir(dir)
        .arg("-d")
        .arg(dir)
        .arg("-1")
        .arg(DET)
        .arg("-2")
        .arg(CLS)
        .arg("-3")
        .arg(REC)
        .arg("-4")
        .arg(KEYS)
        .arg("-i")
        .arg(tmp)
        .arg("-t")
        .arg("4")
        .arg("-p")
        .arg("50")
        .arg("-s")
        .arg("1024")
        // ★ -a 0 关角度分类器：实测一行 50+ 字符、宽高比 > 15:1 的扁长英文行会被
        //   cls 误判成 180°，rec 拿着倒图输出乱码；看图软件里的图本身是正的，
        //   失去"自动纠正倒置截图"这个代价可以接受。
        .arg("-a")
        .arg("0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("引擎没跑起来：{e}"))?;
    let mut out_pipe = child.stdout.take().ok_or("引擎 stdout 没接上")?;
    let mut err_pipe = child.stderr.take().ok_or("引擎 stderr 没接上")?;
    // 两条管道必须在另一条线程里抽干：只 wait 不读，输出一多就会卡在管道缓冲上。
    let h_out = thread::spawn(move || {
        let mut v = Vec::new();
        let _ = out_pipe.read_to_end(&mut v);
        v
    });
    let h_err = thread::spawn(move || {
        let mut v = Vec::new();
        let _ = err_pipe.read_to_end(&mut v);
        v
    });

    let deadline = Instant::now() + ENGINE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("等待引擎失败：{e}")),
        }
    };
    let stdout = h_out.join().unwrap_or_default();
    let stderr = h_err.join().unwrap_or_default();
    let status = status.ok_or_else(|| {
        format!(
            "引擎超时（{} 秒），已终止；这张图可能过大或磁盘过慢",
            ENGINE_TIMEOUT.as_secs()
        )
    })?;

    if !status.success() {
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        let brief: String = String::from_utf8_lossy(detail)
            .chars()
            .filter(|c| *c != '\r' && *c != '\n')
            .take(300)
            .collect();
        return Err(format!(
            "引擎退出码 {}：{}",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".into()),
            brief
        ));
    }
    // 引擎把控制台设成 CP_UTF8，重定向后 printf 仍吐 UTF-8 字节；不能按 GBK 解。
    let text = parse_lines(&String::from_utf8_lossy(&stdout));
    if text.trim().is_empty() {
        return Err(
            "没有识别出文字（引擎跑通了，但框里没有可读的文本行）。请确认图片里确实有文字、且文字不要太小或太糊。"
                .into(),
        );
    }
    Ok(text)
}

/// 把 `textLine[N](文字)` 按 N 排序拼回一段文本。只认行首完整标记，
/// 括号取**最后一个**（识别结果本身带括号也不会被截断）。
fn parse_lines(text: &str) -> String {
    let mut list: Vec<(u64, String)> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_end().strip_prefix("textLine[") else {
            continue;
        };
        let Some(close) = rest.find("](") else {
            continue;
        };
        let Ok(n) = rest[..close].parse::<u64>() else {
            continue;
        };
        let body = &rest[close + 2..];
        let Some(end) = body.rfind(')') else {
            continue;
        };
        list.push((n, body[..end].to_string()));
    }
    list.sort_by_key(|x| x.0);
    list.into_iter()
        .map(|x| x.1)
        .collect::<Vec<_>>()
        .join("\n")
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
    fn text_lines_are_sorted_and_brackets_survive() {
        let s = "modelsPath=.\ntextLine[2](第二行(带括号))\ntextLine[0](第一行)\n\
                 textLine[1]()\n=====End detect=====\n";
        assert_eq!(parse_lines(s), "第一行\n\n第二行(带括号)");
        assert_eq!(parse_lines("random output"), "");
        assert_eq!(parse_lines("textLine[3]()"), "");
    }

    #[test]
    fn only_one_rec_model_is_shipped_no_language_switch() {
        // 负向判据：把繁体专用模型或语言下拉加回来却没同步改产品形态，这里要红。
        assert_eq!(REQUIRED.len(), 5, "随包只有一套 rec + 一份字典");
        assert!(!REQUIRED.iter().any(|n| n.contains("cht")));
        assert_eq!(REC, "ch_PP-OCRv4_rec_mobile.onnx");
        assert_eq!(KEYS, "ppocr_keys_v1.txt");
        let m = include_str!("../manifest.json");
        assert!(!m.contains("\"language\""), "设置里不该再有识别语言下拉");
        assert!(!m.contains("cht"), "manifest 不该再提繁体专用档");
        assert!(m.contains("\"name\": \"OCR 文字识别\""));
    }

    #[test]
    fn missing_pixels_are_an_explained_failure() {
        let r = build_response(&json!({"protocol":1,"image_path":"C:\\a.png"}));
        assert_eq!(r["ok"], json!(false));
        let err = r["error"].as_str().unwrap_or_default();
        assert!(err.contains("image 字段缺席"), "{err}");
        assert!(err.contains("8 MP"), "要给出能照做的下一步：{err}");
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
        write_frame(&mut buf, "文字".as_bytes()).expect("write must work");
        let back = read_frame(&mut std::io::Cursor::new(&buf[..])).expect("read must work");
        assert_eq!(back, "文字".as_bytes());
        let mut bad = (MAX_FRAME as u32 + 1).to_le_bytes().to_vec();
        bad.extend_from_slice(b"x");
        assert!(read_frame(&mut std::io::Cursor::new(&bad[..]))
            .unwrap_err()
            .contains("frame too large"));
    }

    #[test]
    fn reads_both_arg_forms() {
        let args = vec![
            "ocr.exe".to_string(),
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
}
