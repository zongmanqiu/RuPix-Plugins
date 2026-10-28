# RuPix 插件仓库

RuPix 的非解码器扩展全部住在这里：插件是**独立进程**，通过
`manifest.json` + 命名管道 / 共享内存与宿主通信，不进入宿主地址空间。
宿主只做发现、启停、传参与收结果；插件自己管自己的依赖与界面。

> 本仓库与 `RuPix-Decoder/` 是并列的独立源码区：
> 解码器负责“文件字节 → RGBA”，插件负责“已上屏图片 / 路径 → 文本、编辑结果或动作”。

## 目录

```text
RuPix-Plugin/
├─ plugins/                 每个插件一个目录（<id>/manifest.json + 入口）
├─ protocol/                manifest / 请求响应 / 共享内存契约
├─ tools/                   manifest 校验等通用门禁
├─ build.ps1                根构建入口
├─ plugins.json             可下载清单（由 tools/make_catalog.py 生成；与远程镜像同源）
├─ 插件开发规范.md          协议与字段的正式说明
├─ 开发指南.md              新增、构建、验收、发布的顺序
└─ AGENTS.md                本仓库硬规则
```

宿主安装形态：

```text
<exe 目录>/
├─ rupix.exe
├─ plugins/                 # 已安装（右键「插件」菜单从这里取）
│  └─ com.example.ocr/
│     ├─ manifest.json
│     └─ ocr.exe
└─ docs/
   ├─ plugins.json          # 可下载清单（字段/顺序同解码器 docs/decoders.json）
   └─ com.example.ocr_1.0.0.zip
```

## 构建

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

当前有 `com.rupix.echo`（诊断回显）、`com.rupix.ocr`（Windows 本地 OCR）与
`com.rupix.doodle`（独立窗口涂鸦，编辑型）三个插件。
构建会先跑 manifest 校验，再逐个调用插件自己的 `build.ps1`；产物写
`../.temp/plugin-dist/`；根 `build.ps1` 再打包成 `../.build/docs/<id>_<ver>.zip`，
并写 `../.build/docs/plugins.json`。清单里的 `gitee` / `github` 指向本仓库的
release 资产（tag = `<id>_<version>`，asset = `<id>_<version>.zip`）；发布时把同目录
ZIP 上传到对应 release 即可，宿主不需要重新发版。

冒烟：

```powershell
cd <宿主>\main
cargo run --release --example plugin_smoke -- ..\.temp\plugin-dist\com.rupix.ocr ..\.temp\ocr-test.png
```

## 文档地图

| 想知道 | 去哪 |
|---|---|
| 写一个插件 | [`插件开发规范.md`](插件开发规范.md) |
| manifest 字段 / 请求响应 / 共享内存 | [`protocol/`](protocol/) |
| 新增、构建、验收、发布顺序 | [`开发指南.md`](开发指南.md) |
| 本仓库硬规则 | [`AGENTS.md`](AGENTS.md) |
| 文档登记 | [`DOCS_MANIFEST.md`](DOCS_MANIFEST.md) |

## 许可证

本仓库 **AGPL-3.0**；全文见 [`LICENSE`](LICENSE)。
插件若打包第三方组件，必须在自己的目录内附许可证与来源说明。
