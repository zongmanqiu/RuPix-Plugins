# 架构（architecture.md）

> 一页讲清：插件是什么、宿主怎么调它、数据走哪条线。契约细节见
> [`../插件开发规范.md`](../插件开发规范.md) 与 [`../protocol/protocol-v1.md`](../protocol/protocol-v1.md)。

## 1. 进程模型：进程外，宿主管道调起

```
宿主 rupix.exe                                  插件 <id>.exe（独立进程）
  │  CreateProcessW(entry, [manifest.args..., --rupix-pipe <名>, --rupix-protocol 1])
  ├──────────── 命名管道 \\.\pipe\rupix-ext-<pid>-<id>-<seq> ────────────┤
  │  请求帧：u32 LE 长度 + JSON                                           │
  │  响应帧：u32 LE 长度 + JSON                                           │
  │                                                                       │
  │  大块数据不走管道：共享内存 Local\rupix-ext-…（32 字节头 + RGBA8）      │
  └───────────────────────────────────────────────────────────────────────┘
```

- **不 `LoadLibrary`**：插件崩溃不拖垮宿主；宿主只读管道结果，超时/退出即终止进程
  （Job Object 兜底，宿主异常退出时整组子进程被杀）。
- **一次调用 = 一条请求帧 + 一条响应帧**；帧上限 16 MiB；默认超时 30 s
  （manifest `timeout_ms` 可声明，上限 30 分钟）。
- `action` 目前两种：`run`（缺省语义）与 `settings`（只发给 `settings_ui=true` 的插件）。

## 2. 三类插件（`kind`）

| kind | 入口 | 形态 | 宿主侧 |
|---|---|---|---|
| `command` | `.exe` | 无窗口，返回文本/错误 | 右键菜单、结果窗、可取消；声明 `image.read` 时收整图像素 |
| `editor` | `.exe` | 图片进、图片出 | 同尺寸共享内存回写；≤8 MP 闸 |
| `panel` | — | 宿主托管面板 | **只保留类型，未实现** |

## 3. 数据线：什么走管道、什么走共享内存

| 数据 | 线路 | 为什么 |
|---|---|---|
| 请求/响应 JSON | 管道 | 小、需要一问一答 |
| 整图像素（送入） | 共享内存（宿主建、只读） | 8 MP RGBA ≈ 32 MB，管道扛不住 |
| 编辑结果（回写） | 共享内存（插件建、宿主拷走后插件才退） | 同上；谁先退出谁毁掉映射 |
| 打开请求（`open_url` / `open_image`） | **管道 JSON**（图片字节走 base64） | 载荷是码里那几十 KB～几 MB，不值得为它开一条映射；帧上限 16 MiB 是它的天花板 |
| 插件设置 | `plugins/<id>/settings.json`（宿主代存） | 便携铁律：只在 exe 同级 |

像素的**朝向语义**：送进去的是**屏上朝向**那一份（宿主已按旋转/镜像转正），回传必须同尺寸
同朝向，宿主按**送入时冻结**的朝向转回源朝向——正本见《插件开发规范》§6.2。

## 4. 能力（capabilities）：声明即约定，只有一部分真有通道

宿主真正开通道的共**七项**（2026-10-31 起）：

| 能力 | 宿主给的东西 |
|---|---|
| `image.read` | 当前图片的路径 + ≤8 MP 的共享内存像素（屏上朝向那一份） |
| `image.write` | 编辑回写的接收方（同尺寸共享内存 → 宿主编辑态 → 另存 / 还原） |
| `clipboard.write` | 结果自动写系统剪贴板 |
| `settings.read` / `settings.write` | 设置区 `plugins/<id>/settings.json` 的读与合并写 |
| `open.url` | 响应里的 `open_url` 交系统默认浏览器（只认 http/https，宿主再验一次） |
| `open.file` | 响应里的 `open_image` 字节按**文件头**复核后落 `<exe>/temp/`，交系统默认程序 |

其余能力（`network`、`file.read` / `file.write`、`process.spawn`、`ui.window`、
`selection.read`、`ui.panel`）只是**告知**用户——声明了不会有宿主通道，要自己实现
（联网、读写文件、起子进程都在插件自己的进程里做）。未声明者宿主连对应的东西都不发
（能力最小化：没声明 `image.read` 的插件连 `image_path` 都收不到）。

新增通道的判据：**这份数据是不是宿主独占**（解码后的像素、视图上下文、格式路由、
用户资产如剪贴板 / 打开 URL / 写宿主 temp）。不是独占的一律不代办，免得把契约税收在刀背上。

## 5. 设置与快捷键（2026-10-31 起）

- **声明式设置**：manifest 可选 `settings` 数组（`bool` / `choice` / `int` / `text` / `hotkey`），
  声明了它，宿主就**自己渲染**设置窗（齿轮 → 宿主窗口），值存 `settings.json` 并随每次 `run` 下发。
  与 `settings_ui`（插件自画）**二选一**；插件的对话框那条老路留给没有 schema 的老插件。
- **快捷键**：`type=hotkey` 的那一项就是本插件的动态热键。值存设置区（不是清单——清单是发布物，
  用户改不了）。出厂**一律空**；用户在快捷键页或插件设置里绑。**同键抢占**：新绑的赢，
  宿主或别的插件里同一把键会被自动清空。

## 6. 生命周期

1. 宿主扫 `<exe>/plugins/<id>/manifest.json`（发现）。
2. 启停由 `rupix.ini` 的 `extensions_disabled` 管；`min_host` 不满足则入口整体关闭并标原因。
3. 每次调用新起进程、结束即退出（无常驻）。
4. 升级：整目录替换，旧目录的 `settings.json` 先读进内存再搬（可逆）。
5. 卸载：目录进回收站（可还原）。

## 7. 发布物

每个插件一个 ZIP：`<id>_<ver>.zip`，内含 `manifest.json` + 入口 exe + `README.md` /
`LICENSE` 等人读文件。清单 `plugins.json` 由 `tools/make_catalog.py` 生成，
下载地址指向发布仓的 release 资产（tag = `<id>_<version>`，资产 = 同名 zip）。
