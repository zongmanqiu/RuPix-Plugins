# RuPix 插件仓库

> **仓库入口**：[**Gitee**](https://gitee.com/qiuzongman/RuPix-Plugins) 
| [**GitHub**](https://github.com/zongmanqiu/RuPix-Plugins)

## 发布插件

| 插件 | 版本 | 形态 | ZIP 大小 | 发行日期 | Gitee | GitHub |
|---|---:|---:|---:|---:|---|---|
| `checkup`（图片体检） | 0.1.0 | 命令型 | 144,972 B | 2026-10-31 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/checkup_0.1.0/checkup_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/checkup_0.1.0/checkup_0.1.0.zip) |
| `scan`（条码识别） | 0.1.0 | 命令型 | 853,722 B | 2026-10-31 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/scan_0.1.0/scan_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/scan_0.1.0/scan_0.1.0.zip) |

- 表中字节数与日期抄自 [`plugins.json`](plugins.json)（**真源在那份清单里**，本表只是门面；
  不一致由 `python tools/check_docs.py` 判红）。`sha256` 只在清单里给，宿主页下载后
  **size 与 sha256 两个都核对**：size 挡截断/多收，sha256 挡同字节数替换。
- ★ **上架状态（2026-11-01 实测）**：这两枚的 release tag **还没创建**——上面四条地址目前 404，
  宿主点「下载」会如实报失败。上架动作（打 tag + 传 ZIP）由维护者手工做，步骤见
  [`docs/releasing.md`](docs/releasing.md) §1；ZIP 在本仓构建末尾产出到 `<工作区>/.build/docs/`。
- ZIP 内容 = `manifest.json` + `<id>.exe` + `README.md` + `LICENSE` + **`LICENSE-NOTICE.md`**
  + **`THIRD-PARTY-NOTICES.md`**（这枚插件自己的逐包许可表：包名 / 版本 / SPDX 表达式，
  从 `cargo metadata` 现生成——与许可证门禁同一份解析，所以两者不可能各说一套）。
  `LICENSE-NOTICE.md` 说两个事实：本产物 MIT、整仓 AGPL-3.0；对应源码在 tag
  `<id>_<version>` 的 `plugins/<id>/`（Gitee / GitHub 两条链接）。两份都是
  `tools/make_catalog.py` 打包时**现生成**，不在仓库里，所以不会忘也不会失配。
  ★ 我们是开源项目，**不放**"三年内书面提供源码"那类要约卡（源码公开在网上，无需另行提供）；
  写法与理由见 [`docs/licensing.md`](docs/licensing.md) §1。
- **不随包任何外部可执行文件**：条码解码是编进 `scan.exe` 的 Rust crate（`rxing`），
  依赖与许可证账见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

### 远端已发布的历史资产（旧 id）

两镜像上的 tag 仍是三个旧名字：`com.rupix.doodle_0.1.0`、`com.rupix.echo_0.1.1`、
`com.rupix.ocr_0.2.0`。这三枚**已从本仓源码移除**，不再构建、也不在清单里；
**tag 与附件不撤**（撤对外资产是破坏性动作，老用户还要按老清单取包）。

- 主源（Gitee）`main` 的 `plugins.json` 已经是上面表里这两枚；**镜像（GitHub）还没推**，
  选了 GitHub 下载源的用户目前拿到的仍是旧清单。

## 安装

**普通用户**不用碰这个仓库：宿主「设置 → 插件 → 可下载」点「下载」就够了——宿主读随包的
`<exe>/docs/plugins.json`，点「刷新」才按所选下载源去拉远端清单，装完自动启用。

**手工安装**（离线 / 想直接取包）：解压 `<id>_<version>.zip`，把**整个目录**放进 exe 同级的
`plugins/`，形状是 `<exe>/plugins/<id>/manifest.json` + 入口 exe；重启或让设置页重新扫描即可。
★ 别把 ZIP 平铺进 `plugins/`：宿主按"目录里有没有 `manifest.json`"发现插件，散落的 `exe` 不算插件。

```text
<exe 目录>/
├─ rupix.exe
├─ plugins/                 # 已安装（右键「插件」菜单从这里取）
│  └─ scan/
│     ├─ manifest.json      # 唯一被宿主解析的文件
│     ├─ scan.exe
│     ├─ README.md / LICENSE     # 随包给人看，宿主不解析
│     └─ settings.json      # 宿主写的插件私有设置区
├─ docs/
│  ├─ decoders.json         # 解码器清单（已有）
│  ├─ plugins.json          # 插件可下载清单（本仓生成后复制进来）
│  └─ scan_0.1.0.zip        # 离线兜底包：宿主默认走远程，取不到才回退这份本地包
└─ rupix.ini
```

## 兼容性与清单

- **协议区间**：宿主收 `protocol` ∈ `1..=1`，握手回插件自己声明的版本。
  ★ **下界永不抬、只增不改**——旧插件在新宿主上照常跑，与解码器 ABI 的兼容性立约同构。
- **加法式演进**：新能力只走"新可选字段 / 新响应字段 / 新能力位"，老宿主不认识就忽略 ⇒
  给 manifest 加字段**不需要**抬 `min_host`，除非插件真的依赖宿主的新行为。
- **机器可读清单**：[`plugins.json`](plugins.json)（顶层一个数组，一项一枚插件）。
  两份镜像的 raw 地址也就是宿主内置的默认刷新地址：
  - Gitee：`https://raw.giteeusercontent.com/qiuzongman/RuPix-Plugins/raw/main/plugins.json`
  - GitHub：`https://raw.githubusercontent.com/zongmanqiu/RuPix-Plugins/refs/heads/main/plugins.json`
  - 生效地址写在宿主 `rupix.ini` 的 `plugin_index_gitee` / `plugin_index_github`（用户可改），
    上面两条只是内置默认。字段含义与校验规则见 [`docs/releasing.md`](docs/releasing.md) §4。

## 目录

```text
RuPix-Plugin/
├─ plugins/                 每个插件一个目录（<id>/manifest.json + 入口 + 可选 build.ps1）
│  └─ README.md             插件一览
├─ protocol/                manifest / 请求响应 / 共享内存契约（正本）
├─ docs/                    贡献者长期文档（地图 docs/README.md）
├─ tools/                   manifest / 许可证 / 秘密 / 文档四道门禁与清单生成器
├─ build.ps1                根构建入口（门禁 → 各插件 → 打包 + 清单）
├─ plugins.json             可下载清单（由 tools/make_catalog.py 生成，入库留档）
├─ 插件开发规范.md          协议与字段的正式说明
├─ 开发指南.md              新增、构建、验收、发布的顺序
├─ THIRD-PARTY-NOTICES.md   依赖与许可证账
└─ AGENTS.md                本仓库硬规则
```

## 构建

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

构建会先跑四条门禁（`tools/check_manifests.py` / `check_licenses.py` / `check_secrets.py` /
`check_docs.py`），再逐个调用插件自己的 `build.ps1`；编译中间产物落仓库外 `../.temp/plugin-target/`，
发布暂存落 `../.temp/plugin-dist/`，打包产物与清单镜像到 `../.build/docs/`
（源码目录里**不留** `target/` 与 ZIP）。命令、门禁顺序与故障排查见
[`docs/building.md`](docs/building.md)。

冒烟（分析型插件要**真实像素**）：探针第二腿会用宿主自己的解码栈真解一次那张图，
再经共享内存送进插件，把整份报告打到 stdout（所以要先让解码器目录可寻址）。

```powershell
cd <宿主>\main
set RUPIX_DECODERS_DIR=<工作区>\.build\decoders
cargo run --release --example plugin_smoke -- ..\.temp\plugin-dist\checkup <一张真实图片>
```

## 版本与历史

- 版本源头 = 各插件 `plugins/<id>/manifest.json` 的 `version`；发行日期 = `plugins.json` 的
  `released`；协议版本 = `protocol/protocol-v1.md`。**本仓不逐枚维护 `HISTORY.md`**——
  插件只有两枚、每次对外变化都在 manifest 的 `description` 与 README 里写着，另立一份
  编年史就是第二真源。真正的历史账在宿主仓库的 `HISTORY.md` 与工作区计划区。
- 版本、大小、发布状态以 `plugins.json` 为准（README 的表只是门面，且由门禁对着清单判）。

## 文档地图

| 想知道 | 去哪 |
|---|---|
| 写一个插件（manifest 字段 / 请求响应 / 能力位） | [`插件开发规范.md`](插件开发规范.md) |
| 契约逐字正本（帧、共享内存布局、错误码） | [`protocol/protocol-v1.md`](protocol/protocol-v1.md) |
| 新增、构建、验收、发布的顺序 | [`开发指南.md`](开发指南.md) |
| 本仓结构与运行模型 | [`docs/architecture.md`](docs/architecture.md) |
| 本机构建、门禁、target 目录 | [`docs/building.md`](docs/building.md) |
| 改完代码怎么验（门禁 / 单测 / 冒烟 / 真机） | [`docs/testing.md`](docs/testing.md) |
| 版本、tag、双镜像上传、清单字段与校验 | [`docs/releasing.md`](docs/releasing.md) |
| 许可证与第三方归属 | [`docs/licensing.md`](docs/licensing.md) |
| 本仓专属坑 | [`docs/pitfalls.md`](docs/pitfalls.md) |
| 每枚插件自己怎么说 | [`plugins/<id>/README.md`](plugins/README.md) |
| 全部长期文档的登记簿 | [`DOCS_MANIFEST.md`](DOCS_MANIFEST.md) |
| 文档索引（贡献者视角入口） | [`docs/README.md`](docs/README.md) |
| 本仓库硬规则 | [`AGENTS.md`](AGENTS.md) |

宿主侧怎么消费这些产物（发现、启停、能力闸、结果窗）写在**宿主仓库**的
`main/docs/扩展体系.md`——本仓不假设宿主 clone 在哪一层，所以那条不写成相对链接
（写成了就会在独立克隆里被自家链接门禁判红）。

## 许可证

本仓库 **AGPL-3.0**；全文见 [`LICENSE`](LICENSE)。插件目录各自附 **MIT**（`plugins/<id>/LICENSE`），
manifest 的 `license` 字段也是 `MIT`——同一著作权人对"仓库源码"与"可单独取用的插件"分别授权。
每枚发布 ZIP 里都带两份生成的说明文件：`LICENSE-NOTICE.md`（许可 = 这枚插件 MIT / 整仓 AGPL-3.0，
对应源码的 tag 与目录）与 `THIRD-PARTY-NOTICES.md`（**这枚插件自己的逐包许可表**，因为第三方 crate
是静态编进 `<id>.exe` 的，许可识别要随作品一起到用户手里，而不是只指回仓库）。写法与理由见
[`docs/licensing.md`](docs/licensing.md) §1–§2。★ 这里**没有**"三年内书面提供源码"那类要约：
源码本来就公开在网上，无需另行提供。
第三方 crate 与许可证家族账见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)；
若将来要在插件目录里随包外部可执行文件，必须先在那份文件里立一节登记来源，**没立就不随包**。
