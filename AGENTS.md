# RuPix 插件仓库规则（AGENTS.md）

> 对**本仓库根**（`RuPix-Plugin/`，独立仓库）生效的硬约束。
> 门面见 `README.md`；契约见 `插件开发规范.md` 与 `protocol/`；
> 修改、构建、发布顺序见 `开发指南.md`；**长期文档（架构 / 构建 / 测试 / 发布 / 许可 / 踩坑）见 `docs/`**。
>
> **目录名铁律**：本目录叫 **`RuPix-Plugin/`**。它是宿主之外的独立源码区，
> 不是宿主 `main/src/extensions/` 的别名。宿主只消费发布产物
> （`<exe>/plugins/<id>/`），永远不 import 本仓库代码。

---

## 一、仓库边界（硬）

1. **不依赖宿主**：不得依赖 `rupix` / `rupix_lib`、`windui` / `windows`，
   不得把宿主的 `src/` 当库引用。插件是独立进程，宿主只通过
   `manifest.json` + 协议 v1 与它通信。
2. **不依赖解码器**：插件不实现图片格式解码路由，不加载 `decoders/*.dll`，
   不认领文件后缀。图片由宿主解好后交给插件。
3. **进程外**：每个插件是独立可执行文件；宿主不 `LoadLibrary` 插件，也不把
   插件地址空间与宿主混在一起。
4. **只认 manifest**：宿主发现一个插件只需要 `manifest.json` + 入口文件；
   不允许“没有 manifest 的隐式插件”。
5. **本仓库只放源码与文档**：编译中间产物、发布 ZIP、日志、临时探针一律
   不落源码目录。默认落点：
   - 构建中间产物 → `../.temp/plugin-target/`
   - 发布暂存 → `../.temp/plugin-dist/`
   - 宿主镜像 → `../.build/docs/<id>_<ver>.zip`
6. **单向关系**：本仓库 → 发布目录 → 宿主扩展目录。插件不得反向写宿主源码。

## 二、插件形态（硬）

| `kind` | 入口 | 用途 | 当前宿主状态 |
|---|---|---|---|
| `command` | `.exe` | 无自建窗口，返回文本/错误 | 已接入：右键菜单启动 + 结果窗 + 可取消；声明 `image.read` 时与编辑型收**同一份**整图像素（屏上朝向、≤8 MP） |
| `editor` | `.exe` | 图片进、图片出（在图上画、裁剪这类） | 已接入：右键菜单启动，整图 ≤8MP |
| `panel` | 可选 | 宿主托管面板 | 只保留类型，未实现 |

- 入口必须是插件目录内的相对路径；禁止 `..`、盘符、绝对路径。
- `manifest.json` 的 `schema` 只接受 `1`；`protocol` 必须落在宿主支持区间
  （当前 `1..=1`，只增不改、下界永不抬——旧插件在宿主升级后继续可跑）。
- `platforms` 必须包含 `windows-x64`。
- 具体字段与校验见 [`插件开发规范.md`](插件开发规范.md)。

## 三、安全底线（硬）

1. 默认不联网；需要联网的插件必须在 `capabilities` 里声明 `network`。
   ★ 2026-10-29 宿主口径：**安装即视为同意**，宿主不再弹首用同意窗；
   宿主也不提供网络通道，联网是插件自己的行为。
2. 不写注册表、不写 `%APPDATA%`；插件自己的数据只放插件目录或宿主传给它的
   临时目录。
3. 不启动子进程、不写任意文件，除非 manifest 声明
   `process.spawn` / `file.write`。
4. 不把用户图片上传到网络；识别类活儿（文字 / 条码）在本地做，不联网、不外传。
5. 不把密钥、令牌、私人路径写进仓库；公开仓库按 `RuPix-Decoder` 同款无密钥规则执行，
   而且**这条已经机器化**：`python tools/check_secrets.py`（六条高置信判据 + `--selftest`，
   扫 0 个判失败）已接进根 `build.ps1`。泄了再删文件不算解决——它已经在 git 历史里，
   第一动作是作废那把凭据。
6. 插件崩溃不得拖垮宿主：宿主只读管道结果，超时/退出即终止。
7. ★ **随包外部可执行文件的前置账**：要在插件目录里放一枚**不是本仓写的**可执行文件
   （解码/识别工具之类），必须先在 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)
   给它立一节（来源 / 版本 / 许可证 / 获取方式）。**那一节不存在 ⇒ 不随包**。
   纯 Rust crate 依赖由 `tools/check_licenses.py` 自动覆盖，不需要手工登记。

## 四、构建与发布（硬）

- 一键入口：`powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1`
- **文档门禁**：`python tools/check_docs.py`（四问：相对链接可达 / 每份文档在 `DOCS_MANIFEST.md`
  登记 / 正文无本机绝对路径（围栏代码块豁免）/ 根 `README.md` 的「发布插件」门面表与
  `plugins.json` **逐格判等值**（id 集合、版本、形态、字节数、日期、两条下载地址））。
  ★ 数字抄进 README 是**刻意的门面**（用户从 README 找下载地址，与解码器仓同一口径），
  但它必须有守卫——只靠注释互指的两份数字一定会漂；失败臂由 `--selftest` 自证，
  "表里一行都没扫到"判**失败**不算通过。
- **秘密门禁**：`python tools/check_secrets.py`（根 `build.ps1` 在许可证门禁之后跑它）。
  判据与解码器仓**同源但各扫各树**；只有六条高置信判据（私钥块 / 带够长实体的令牌前缀 /
  URL 内嵌凭据 / 带引号的口令字面量 / 邮箱（署名与许可证文件豁免）/ 含真名的 `C:\Users\<名字>`），
  **宁可少报也不误报到没人看**；`--selftest` 14 条自证，扫 0 个判**失败**。
- **供应链门禁**：`python tools/check_licenses.py`（根 `build.ps1` 在 manifest 校验之后跑它）。
  它**自己扫 `plugins/*/Cargo.toml`** ⇒ 加插件不需要往任何清单里登记（清单式门禁失明的正是"忘了加一行"）；
  扫不到 manifest 判**失败**，不算通过。加许可证家族要同时改 `ALLOWED` 与 `THIRD-PARTY-NOTICES.md`；
  它的失败臂由 `--selftest` 自证。
- **每枚插件目录三份人读文件**：`plugins/<id>/README.md`（当前能力与限制）、
  **`plugins/<id>/HISTORY.md`（版本历史，发 release 的说明文字从这里抄）**、`LICENSE`。
  ★ 前两份由 `tools/check_docs.py` 第 5 查判存在与对齐：**首条 `## <版本>` = manifest 的
  `version`，同一条里要有 `released` 的日期**，条目里**不许**写发布进度
  （`待上传` / `未发布` / `release tag` / ZIP 是否生成——那些的真源是 `plugins.json`）。
  改 `version` 必须**同一次**加一条历史，否则门禁红。
- 每个插件目录可有自己的 `build.ps1`；根脚本负责顺序调用、校验 manifest、
  汇总产物。
- 发布产物形态：
  ```text
  ../.build/docs/<id>_<ver>.zip
  ├─ manifest.json
  ├─ <entry>.exe
  └─ （插件自带依赖，扁平放在同目录或子目录）
  ```
- 同一 `id` 只保留一份；旧版本在构建末尾统一清理，不在循环里删。
- 发布目录不出现 `target/`、`.pdb`、日志、测试夹具。

## 五、脚本与编码

- 本仓库的 `.ps1` / `.bat` **一律 ASCII**（PowerShell 5.1 按 ANSI 读无 BOM
  脚本，中文注释会被解错）。注释写英文。
- Python 工具统一 UTF-8；输出用 ASCII 冒号与方括号，避免控制台代码页问题。
- 文档中文 UTF-8；路径用相对路径或 `<宿主>` 占位，不写本机绝对路径。

## 六、修改流程

1. 改协议 / manifest → 同步 `protocol/` + `插件开发规范.md` + 宿主
   `main/docs/扩展体系.md`（接口变了，两边都要能独立读懂）。
2. 加插件 → `plugins/<id>/`，必须有 `manifest.json` 与入口。
3. 构建 → 根 `build.ps1` 全绿；产物镜像到 `.build/docs/plugins.json`（ZIP 同目录）。
4. 宿主验收 → 打开设置「插件」页能看到；右键「插件」子菜单能触发。
5. 文档同步 → `DOCS_MANIFEST.md` 登记新增文档。
