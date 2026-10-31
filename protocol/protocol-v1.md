# 插件协议 v1

> 传输契约正本。manifest 字段见 [`manifest.schema.json`](manifest.schema.json)；
> 中文说明见 [`../插件开发规范.md`](../插件开发规范.md)。

## 1. 命名管道

- 宿主创建：`\\.\pipe\rupix-ext-<pid>-<id>-<seq>`
- 插件连接：`CreateFileW` 打开同名管道。
- 帧：`u32 little-endian length` + `body`。
- 单帧上限：16 MiB。
- 一次调用 = 一条请求帧 + 一条响应帧。

## 2. 启动参数

宿主在 manifest `args` 之后追加：

```text
--rupix-pipe <pipe-name>
--rupix-protocol <插件声明的 protocol>（当前 1）
```

插件必须解析这两个参数；未知参数忽略。

## 3. 请求

```json
{
  "protocol": 1,
  "id": "example-ocr",
  "action": "run",
  "image_path": "C:\\images\\a.png",
  "selection": {"x": 0, "y": 0, "w": 100, "h": 50},
  "args": ["--lang=chi_sim"]
}
```

- `image_path` 可选；路径是宿主当前图片。**只有声明 `image.read` 的插件才拿到**
  （没声明 ⇒ 路径与像素都不发，宿主不该让一个纯诊断插件看见用户在看什么）。
- `image` 可选：共享内存描述。**命令型也能收到**（2026-10-31 起，声明 `image.read` 就给）：
  两样**可能同时存在**，而拿不到整图时（切片态 / 超 8 MP 闸）**只剩 `image_path`**——
  插件必须能退化并如实报，不许把"没收到像素"当成宿主坏了。
  ```json
  {"name":"Local\\rupix-ext-...","width":800,"height":600,"stride":3200}
  ```
- `selection` 可选，原图像素坐标。
- `args` 是 manifest `args` 的副本。
- `settings` 可选：宿主已存的插件设置（对象，值只能是字符串/数字/布尔）。
  只有声明 `settings.read` 的插件才会收到。
- `action`：`run`（缺省语义）/ `settings`。`settings` **只发给** manifest
  `settings_ui=true` 的插件——这是协议只加不改的做法：旧插件永远收不到未知 action。
  插件不认识某个 action 必须回 `{"ok":false,"error":"..."}`，不许静默当 `run` 处理。

## 4. 响应

```json
{"protocol": 1, "ok": true, "text": "..."}
```

```json
{"protocol": 1, "ok": true,
 "image": {"name":"Local\\rupix-ext-...","width":800,"height":600,"stride":3200}}
```

命令型还可以带两个**可选**的"打开"动作（加法式，2026-10-31 新增）：

```json
{"protocol": 1, "ok": true, "text": "...",
 "open_url": "https://example.invalid/a",
 "open_image": {"ext": "PNG", "base64": "iVBORw0KGgo..."}}
```

- 只有声明了 `open.url` / `open.file` 的插件，宿主才会采纳（没声明 = 这两个字段被忽略）。
- `open_url` 必须是 `http` / `https`，否则宿主不打开（只记日志）。
- `open_image` 的字节由宿主按**文件头**复核（认不出就不打开），落地到
  `<exe>/temp/barcode-<毫秒>.<ext>` 后交系统默认程序；`ext` 只是提示。
- 打开失败不影响结果窗：文本照常显示，失败只 toast。

```json
{"protocol": 1, "ok": false, "error": "..."}
```

```json
{"protocol": 1, "ok": true, "settings": {"open_url": "1"}}
```

- `protocol` 必须落在宿主支持区间（当前 `1..=1`）。宿主按**插件声明的版本**握手；
  旧版插件在区间内继续可跑（宿主升级后不要求插件重编）。
- `ok=false` 必须有 `error`。
- `text` / `image` 可同时存在。
- `settings` 可选：只有声明 `settings.write` 的插件回传才会被宿主落盘
  （合并进 `plugins/<id>/settings.json`）；值只能是字符串/数字/布尔。

## 5. 共享内存

- 名称前缀：`Local\rupix-ext-`
- 头 32 字节：

  | 偏移 | 类型 | 含义 |
  |---:|---|---|
  | 0 | `u8[8]` | magic `RPXIMG01` |
  | 8 | `u32 LE` | width |
  | 12 | `u32 LE` | height |
  | 16 | `u32 LE` | stride（字节） |
  | 20 | `u32 LE` | len（= stride × height） |
  | 24 | `u8[8]` | reserved（0） |
  | 32 | `u8[]` | RGBA8 像素 |

- 宿主创建可读写映射；插件只读打开。
- `stride >= width*4`；`len == stride*height`。
- 插件不得自行枚举或打开请求之外的共享内存。

## 6. 超时

- 宿主默认 **30 秒**；超时即终止插件进程。
- **长任务由插件自己在 manifest 里声明 `timeout_ms`**（毫秒，上限 30 分钟；超限按坏清单拒载）
  ——宿主不替插件猜它要多久：猜短了腰斩，猜长了用户干等。用户侧不需要任何配置。
- 编辑型（`kind=editor`）与设置型（`action=settings`）是**交互任务**，不受这个超时约束，
  由用户点「取消」兜底。
- 插件应尽快返回；不要假设"宿主一定会等"。
- 用户取消 = 管道关闭 / 进程终止，无额外取消消息。

## 7. 编辑型插件（kind=editor）的输出映射

编辑型插件回 `ok=true` + `image` 时，`image` 指向**插件新建**的共享内存（不是请求里
那块）：

- 名字：`Local\rupix-ext-<插件段>-<pid>-<毫秒时间戳>`（如 `Local\rupix-ext-example-edit-1234-1730000000000`）；
- 布局与 §5 完全相同（32 字节头 + RGBA8）；插件以 `CreateFileMappingW(PAGE_READWRITE)` 创建；
- **时序铁律**：宿主必须在插件退出前打开该映射并拷走像素；插件回帧后阻塞再读管道，
  直到宿主关闭（EOF/错误）才退出。谁先退出谁毁掉映射；
- 因此宿主给 editor 类插件配长超时（会话时长 = 用户编辑时长）。

## 8. 错误码

v1 不定义数字错误码；`error` 是给人看的中文/英文短句，日志可附细节。
宿主只保证：非空、单行、长度受限（具体上限由宿主界面决定）。
