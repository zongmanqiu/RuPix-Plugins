# RuPix 插件仓库

> **仓库入口**：[**Gitee**](https://gitee.com/qiuzongman/RuPix-Plugins) 
| [**GitHub**](https://github.com/zongmanqiu/RuPix-Plugins)

## 发布插件

| 插件 | 版本 | 形态 | ZIP 大小 | 发行日期 | Gitee | GitHub |
|---|---:|---:|---:|---:|---|---|
| `checkup`（图片体检） | 0.1.0 | 命令型 | 140,159 B | 2026-10-31 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/checkup_0.1.0/checkup_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/checkup_0.1.0/checkup_0.1.0.zip) |
| `scan`（条码识别） | 0.1.0 | 命令型 | 798,029 B | 2026-10-31 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/scan_0.1.0/scan_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/scan_0.1.0/scan_0.1.0.zip) |
| `levels`（色阶分析） | 0.1.6 | 命令型 | 125,708 B | 2026-11-06 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/levels_0.1.6/levels_0.1.6.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/levels_0.1.6/levels_0.1.6.zip) |
| `channels`（通道调色） | 0.1.1 | 编辑型 | 108,679 B | 2026-11-06 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/channels_0.1.1/channels_0.1.1.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/channels_0.1.1/channels_0.1.1.zip) |

- 表中字节数与日期抄自 [`plugins.json`](plugins.json)（**真源在那份清单里**，本表只是门面；
  不一致由 `python tools/check_docs.py` 判红）。清单**不记哈希**（2026-11-02 起：
  官方产物只认名称与版本号），`size` 只用于上表这列显示。
- ★ **上架状态以远端为准**：表中每枚插件的两条地址由维护者打 tag + 传 ZIP 后才会生效；未上架时
  宿主点「下载」会如实报失败，不会静默跳过。上架步骤见
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

- 源码只推主源（Gitee）的 `main` 分支；GitHub 仓库是自动从 Gitee 复制并推送的镜像，
  不另行手工推送。镜像延迟期间，GitHub 侧读到的仍是上一版清单。
  ★ release 的 ZIP 附件与说明不属于 git 内容、镜像不带：这两样必须 Gitee / GitHub
  两边分别上传，见 `开发指南.md` §四。

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
│  ├─ README.md             插件一览
│  └─ <id>/HISTORY.md       每枚插件自己的版本历史（发 release 抄的就是这一页）
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

- **每枚插件一份 `plugins/<id>/HISTORY.md`**（中文，只记对外可见的变化）。发 release 时
  说明文字就从这一页抄——这一格曾经缺着，而缺着它构建照样全绿，所以它现在由
  `python tools/check_docs.py` 第 5 查门禁判：`README.md` / `HISTORY.md` / `LICENSE`
  三份齐备、**每条**标题 `## <版本> — <日期> — protocol N`、首条的版本 / 日期 / protocol
  = manifest 的 `version` / `released` / `protocol`、条目里不许写发布进度。
  ★ 版本历史不是 manifest `description` 的第二份副本（那字段只描述当前能力），
  而是"这一版相对上一版对外变了什么"的唯一落点；少了它，发布流程就缺一块材料。
- 版本源头 = 各插件 `plugins/<id>/manifest.json` 的 `version`；发行日期 = `plugins.json` 的
  `released`；协议版本与它本身的史 = `protocol/protocol-v1.md`。
- **历史条目只记版本事实，不记发布进度**：`待上传` / `未发布` / `release tag` / ZIP 是否生成
  一律不进历史——发布状态的唯一真源是 `plugins.json`。未发布版本的中间改动**并入下一条**。
- **ZIP 里带插件自己的 `README.md`，不带 `HISTORY.md`**：历史是给维护者写 release 说明用的，
  装完插件的用户不需要它（解码器仓同理：每档一份 `HISTORY.md` 在源码仓，不进 DLL 的包）。
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
