# 第三方组件许可与说明（RuPix-Plugin）

> **读取日期 2026-11-11。版本与许可证一律实读 `cargo metadata`（各插件自己的 `Cargo.lock`），
> 不许凭记忆写。** 机器判据 = `python tools/check_licenses.py`（白名单在
> `tools/check_licenses.py` 的 `ALLOWED` 里，加一个许可证家族必须同时改那里和本表）。

## 一、本仓自身的授权（实读，不是政策）

| 位置 | 声明 |
|---|---|
| 仓库根 `LICENSE` | **AGPL-3.0** 全文 |
| 每枚插件目录 `plugins/<id>/LICENSE` | **MIT**（`Copyright (c) 2026 RuPix`） |
| 每枚 `manifest.json` 的 `license` 字段 | `MIT` |

⇒ 现状是"仓库 AGPL、插件目录各自 MIT"，同一著作权人分别授权（插件是可单独取用的产物）。

★ **每枚发布 ZIP 必带一份 `LICENSE-NOTICE.md`**（2026-11-01 定稿），只写两个事实：
这份产物按 **MIT**、整仓按 **AGPL-3.0**；对应源码 = tag `<id>_<version>` 下的 `plugins/<id>/`
（Gitee / GitHub 两条链接）。它**不是仓库里的文件**，而是 `tools/make_catalog.py::license_notice()`
打包时现生成的——所以不存在"忘了放"，也不必手工维护第二份措辞。这份文件只是许可与源码位置的
说明，不是法律意见。
★ **为什么不做 `SOURCE-OFFER.md` 那种书面要约**（同日定稿）：那张"三年内书面提供源码"
的要约卡是**物理分发时代**的遗留，而我们是开源项目、源码就在网上——没有需要"另行提供"的东西，
于是连名字也不再叫 offer。**解码器仓同日撤掉了它自己的那一枚**：那边的 `raw` 静态链接 LGPL 的
`rawler`，"重链接材料"是真义务、不是仪式，内容并进了包内 `README.md` 的「许可与源码」一节。

## 二、插件与直接依赖

| 插件 | 形态 | 直接依赖（实读 `Cargo.toml`） |
|---|---|---|
| `checkup` | 命令型（图片体检） | `serde_json` 1、`windows` 0.62（只开 `Win32_Foundation` / `Win32_System_Memory`） |
| `levels` | 命令型（色阶分析） | `serde_json` 1、`windows` 0.62（同上两个 feature）；**零新增 crate**——PNG 编码、CRC32、Adler-32 与 base64 均在本插件内实现 |
| `scan` | 命令型（条码识别） | `serde_json` 1、`windows` 0.62（同上两个 feature）、**`rxing` 0.6**（ZXing 的 Rust 移植，`default-features = false`） |
| `ocr` | 命令型（文字识别） | `serde_json` 1、`windows` 0.62（同上两个 feature）；**零新增 crate**——PNG 编码、CRC-32 / Adler-32 / stored deflate 均在本插件内实现。它另带**随包外部引擎与模型**，见下一节 |
| `channels` | 编辑型（通道调色） | `serde_json` 1、`windows` 0.62（`Win32_Foundation` / `Win32_Security` / `Win32_System_Memory`）；**零新增 crate**——只比前三枚多开一个 `windows` feature |

★ **`scan` 为什么关默认特性**：rxing 的 `default` 含 `image` + `image_formats`，会拖进一整套
图片解码栈（png / jpeg / avif / tiff…）。插件**不解文件**——像素是宿主解好给的（AGENTS §一.2
「不依赖解码器」），关掉之后依赖树仍然有 **87 枚**包（`chrono-tz` 是 rxing 解析 DataMatrix/Aztec
日期段的非可选依赖，砍不掉），`scan.exe` 因此是各插件 exe 里最大的一枚（整包最大的则是
`ocr`，它另带随包引擎与模型，见下一节）。这笔账写在这里，
是为了下次有人说"顺手加个解码库"时能看见代价。

★ 「不随包任何外部可执行文件」这条曾经成立；**`ocr` 是唯一例外**，按 AGENTS §三.7
在下一节登记。其余四枚的产物仍只有各自的 `<id>.exe` + `manifest.json`，条码解码走的是
**编进 `scan.exe` 的 Rust crate**，不是随包一枚外部 CLI。

### 随包外部可执行文件与模型（`ocr`）

> 这一节不存在 ⇒ 不随包。来源 / 版本 / 许可证 / 获取方式四要素逐项写实；
> 每个文件的 SHA256 由 `plugins/ocr/fetch_assets.py` 在构建前逐个复核，对不上即判红。

**引擎**

| 项 | 内容 |
|---|---|
| 文件 | `engine/CurSeenOcr.exe`（16,798,720 B） |
| 来源 | 已发布的 `OCR_0.3.0.zip` 中的 `OCR/ocr_tool/CurSeenOcr.exe`；ZIP SHA256 `17674a462fe162ed2363444359678600cc06c761539a85cf316fdee9688dd4aa` |
| 上游与版本 | [RapidOcrOnnx](https://github.com/RapidAI/RapidOcrOnnx) 官方 CLI 源码副本，固定提交 `675e73fe4c8b9e1d0be558bb5959a1b37696ed90`，工程版本 1.2.2 |
| 许可证 | Apache-2.0（上游）；其静态链接的 onnxruntime 为 MIT、OpenCV 为 Apache-2.0 |
| 获取方式 | `python plugins/ocr/fetch_assets.py` 按固定 URL 下载并校验，缓存与产物都落在工作区 `.temp/`，**不进源码仓** |
| 文件 SHA256 | `93a647539174950bb33f17c2002102fcc22116c1c0f9fe2616ce039c0567e5a2` |

**模型与字典**：PP-OCRv4 系列，Apache-2.0。四个文件取自已发布的 `OCR_0.3.0.zip`
（`OCR/ocr_tool/`），SHA256 与 CurSeen 的 `tools/ocr/upstream.lock.json` 版本锁逐字一致。

| 文件 | 大小 (B) | SHA256 |
|---|---:|---|
| `ch_PP-OCRv4_det_mobile.onnx` | 4,745,517 | `d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9` |
| `ch_ppocr_mobile_v2.0_cls_mobile.onnx` | 585,532 | `e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c` |
| `ch_PP-OCRv4_rec_mobile.onnx` | 10,857,958 | `48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b` |
| `ppocr_keys_v1.txt` | 26,249 | `28b2362ad4ab2dc38769aa72feb535e3a9ddb3fd2a7585a05920e6393b1dc7f7` |

★ 模型与字典是**训练时配对**的：CTC 输出通道数由字典长度定死，改字典不能扩充字符集。
随包只有**一套** rec + 一份字典（6623 字，其中 6270 个 CJK 统一汉字、94 个 ASCII
可打印字符与常见符号）：简体中文、繁体中文、英文在同一批码位里，一次识别同时出，
不做语言开关。日文 / 韩文 / 西里尔等模型不在包内；`ocr` 的 README 对用户如实列明这个边界。

## 三、许可证家族账（87 枚解析后包，按家族计数）

> 2026-11-07 复核：`channels` 只多开了一个 `windows` feature，没有引入新 crate，
> `python tools/check_licenses.py --list` 仍是 **87 枚**，下表计数与家族一个都没漂。

| 家族 | 出现次数 | 判定 | 谁引入 |
|---|---|---|---|
| `MIT` | 78 | 可再分发 | serde / windows / rxing 三棵树 |
| `Apache-2.0` | 68 | 可再分发（多为 MIT OR Apache 双许可） | 同上 |
| `CC0-1.0` | 6 | 公有领域 dedication，无义务 | `encoding` 的字符集索引子包（`encoding-index-*`、`encoding_index_tests`） |
| `Unlicense` | 4 | 公有领域 | `memchr` 等（多为 Unlicense OR MIT） |
| `BSL-1.0` | 1 | Boost 许可，宽松 | `ryu`（Apache-2.0 OR BSL-1.0，取其一即可） |
| `Unicode-3.0` | 1 | Unicode 许可（数据表） | `unicode-ident`（`(MIT OR Apache-2.0) AND Unicode-3.0`） |

> 全部落在可再分发家族里，**没有 GPL / AGPL 系的第三方依赖**（本仓自身是 AGPL，
> 但编进插件 exe 的第三方都不是）。`AGPL` 与 `LGPL` 不在 `ALLOWED` 里——不是"它们不好"，
> 是"它们会给我们自己的分发加义务"，加进白名单需要单独拍板。

★ **逐包清单不在本文件里手抄**。87 行手抄表是没人会更新的表，而一份过期的登记表读起来像
一份保证，比没有更糟。全集的机器可读来源是：

```
python tools/check_licenses.py --list
```

本文件只留**政策**（家族账、随包规矩、加依赖的规矩），门禁只留**判据**（`ALLOWED`）。
★ **逐包表随包走**：每枚发布 ZIP 里带一份**该插件自己的** `THIRD-PARTY-NOTICES.md`
（包名 / 版本 / SPDX 表达式），由 `tools/make_catalog.py::third_party_notices()` 从
`cargo metadata` 现生成——第三方 crate 是静态编进 `<id>.exe` 的，许可识别要随作品到达收件人，
只指回仓库根是弱形态。它用的就是门禁那一份解析，所以**表与门禁不可能各说一套**。
`CC0-1.0` 与 `BSL-1.0` 是 2026-10-31 随 `scan` 引入的 rxing 依赖树加进来的，
两族在解码器仓的同名门禁里早就允许 ⇒ 这是**对齐既有政策**，不是逐包放行。

## 四、加依赖的规矩

1. 新依赖必须能进 `ALLOWED`；进不去（GPL 系、来源不明、无 `license` 字段）就是**判不采用**，
   不是"加一行白名单"。
2. 加完跑 `python tools/check_licenses.py`（它自己扫 `plugins/*/Cargo.toml`，**不需要**把新插件
   登记到某个清单里——清单式门禁正是它会失明的地方），再用
   `python tools/check_licenses.py --list` 重读全集，把**新出现的家族**补进 §三 那张表。
   逐包行不抄进本文件（抄了就会过期）。
3. 门禁的失败臂由 `--selftest` 自证：`python tools/check_licenses.py --selftest`
   （不在白名单要报 / 无许可证要报 / AND-OR 复合表达式不许误报 / 本仓自写 crate 不判 /
   空目录算失明不算通过 / 本仓现状必须扫得到货）。
