# com.rupix.ocr

使用 **Windows.Media.Ocr** 的本地 OCR 插件。不联网、不带模型文件，依赖系统已安装的
OCR 语言包（本机实测 `zh-Hans-CN` / `en-US`）。

- 能力：`image.read`（读取宿主传来的当前图片路径）
- 协议：v1（命名管道 + JSON）
- 结果：清洗后的识别文本；宿主打开结果窗，可滚动、可复制
- 大图保护（0.2.0）：像素数超过 4,000,000 时先软件降采样到 ≤4MP（盒式均值、保持宽高比）
  再识别，防止大图超时

构建后产物：`<workspace>/.temp/plugin-dist/com.rupix.ocr/`，由根 `build.ps1`
镜像到 `.build/docs/com.rupix.ocr_0.2.0.zip`。

冒烟（需要一张有文字的图片）：

```powershell
cd <宿主>\main
cargo run --release --example plugin_smoke -- ..\.temp\plugin-dist\com.rupix.ocr <图片路径>
```

没有可用语言包时插件返回 `ok=false` 与“没有可用的 OCR 语言包”，宿主按错误提示。
