# 引擎与模型许可（随包）

`engine/` 下的可执行文件与模型**不是本仓 Rust 源码的编译产物**，随本插件一起分发：

| 组件 | 来源与版本 | 许可证 |
|---|---|---|
| `CurSeenOcr.exe` | RapidOcrOnnx 官方 CLI，工程版本 1.2.2（固定提交 `675e73fe…`）；静态链 onnxruntime 与 OpenCV | Apache-2.0（上游） |
| `ch_PP-OCRv4_det_mobile.onnx`、`ch_PP-OCRv4_rec_mobile.onnx`、`ch_ppocr_mobile_v2.0_cls_mobile.onnx`、字典 `ppocr_keys_v1.txt` | PP-OCRv4 系列的检测 / 识别 / 方向分类模型与配套字典 | Apache-2.0 |

每个文件的 SHA256、来源 URL 与完整登记见插件仓根目录的 `THIRD-PARTY-NOTICES.md`；
构建脚本 `plugins/ocr/fetch_assets.py` 会在每次打包前逐个复核哈希，对不上即判红。

引擎只在本机读取插件写下的临时 PNG，**不联网、不上传图片、不写插件目录以外的位置**；
临时文件用完即删。识别范围与符号清单见随包的 `README.md`。
