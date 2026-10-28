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
--rupix-protocol 1
```

插件必须解析这两个参数；未知参数忽略。

## 3. 请求

```json
{
  "protocol": 1,
  "id": "com.example.ocr",
  "action": "run",
  "image_path": "C:\\images\\a.png",
  "selection": {"x": 0, "y": 0, "w": 100, "h": 50},
  "args": ["--lang=chi_sim"]
}
```

- `image_path` 可选；路径是宿主当前图片。
- `image` 可选：共享内存描述：
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

```json
{"protocol": 1, "ok": false, "error": "..."}
```

```json
{"protocol": 1, "ok": true, "settings": {"echo.upper": "1"}}
```

- `protocol` 必须等于 1。
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

- 宿主默认 30 秒；超时即终止插件进程。
- 插件应尽快返回；长任务应在宿主侧配置更长超时。
- 用户取消 = 管道关闭 / 进程终止，无额外取消消息。

## 7. 编辑型插件（kind=editor）的输出映射

编辑型插件回 `ok=true` + `image` 时，`image` 指向**插件新建**的共享内存（不是请求里
那块）：

- 名字：`Local\rupix-ext-<插件段>-<pid>-<毫秒时间戳>`（如 `Local\rupix-ext-doodle-1234-1730000000000`）；
- 布局与 §5 完全相同（32 字节头 + RGBA8）；插件以 `CreateFileMappingW(PAGE_READWRITE)` 创建；
- **时序铁律**：宿主必须在插件退出前打开该映射并拷走像素；插件回帧后阻塞再读管道，
  直到宿主关闭（EOF/错误）才退出。谁先退出谁毁掉映射；
- 因此宿主给 editor 类插件配长超时（会话时长 = 用户编辑时长）。

## 8. 错误码

v1 不定义数字错误码；`error` 是给人看的中文/英文短句，日志可附细节。
宿主只保证：非空、单行、长度受限（具体上限由宿主界面决定）。
