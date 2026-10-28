# RuPix 插件仓库规则（AGENTS.md）

> 对**本仓库根**（`RuPix-Plugin/`，独立仓库）生效的硬约束。
> 门面见 `README.md`；契约见 `插件开发规范.md` 与 `protocol/`；
> 修改、构建、发布顺序见 `开发指南.md`。
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
| `command` | `.exe` | 无自建窗口，返回文本/错误 | 发现 + 启停；运行编排接入中 |
| `editor` | `.exe` | 图片进、图片出（涂鸦等） | 只发现，不运行 |
| `panel` | 可选 | 宿主托管面板 | 只保留类型，未实现 |

- 入口必须是插件目录内的相对路径；禁止 `..`、盘符、绝对路径。
- `manifest.json` 的 `schema` 与 `protocol` 当前都只接受 `1`。
- `platforms` 必须包含 `windows-x64`。
- 具体字段与校验见 [`插件开发规范.md`](插件开发规范.md)。

## 三、安全底线（硬）

1. 默认不联网；需要联网的插件必须在 `capabilities` 里声明 `network`，并在
   首次运行时让用户明确同意。
2. 不写注册表、不写 `%APPDATA%`；插件自己的数据只放插件目录或宿主传给它的
   临时目录。
3. 不启动子进程、不写任意文件，除非 manifest 声明
   `process.spawn` / `file.write`。
4. 不把用户图片上传到网络；OCR 默认本地。
5. 不把密钥、令牌、私人路径写进仓库；公开仓库按 `RuPix-Decoder` 同款
   无密钥规则执行。
6. 插件崩溃不得拖垮宿主：宿主只读管道结果，超时/退出即终止。

## 四、构建与发布（硬）

- 一键入口：`powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1`
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
