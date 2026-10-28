# com.rupix.doodle

编辑型涂鸦插件（`kind=editor`）：宿主把当前图片的**全分辨率 RGBA** 放进共享内存，
插件打开一个独立 Win32 窗口「图像涂鸦」，在**缩放预览**上画；点「应用」时把全部笔画
按**原图分辨率**重绘一遍，写进新建共享内存回给宿主。零第三方 UI 框架，直接 Win32 +
`windows` crate。

- 能力：`image.read`（读宿主共享内存）、`image.write`（回写编辑结果）、`ui.window`（自建窗口）
- 协议：v1（命名管道 + JSON 帧；共享内存 32 字节头 `RPXIMG01`，见 `../../protocol/`）

## 界面与操作

- 工具：铅笔 / 橡皮（橡皮 = 画回底图色）
- 固定 12 色色板 + 当前色块
- 笔宽 1..64（+/− 按钮，数字显示；笔画存像素半径 = 宽/2）
- 撤销：Ctrl+Z 或「撤销」按钮；「清空」可撤销
- 应用：按钮或 **Enter**；取消：按钮、**Esc** 或窗口 X（都按取消回帧）
- 笔画按图像坐标存 `{points, radius, color, erase}`；预览与原图重绘共用同一光栅化函数（Bresenham 圆头线段）

## 退出时序（editor 关键契约）

回帧后插件**阻塞再读管道直到宿主关闭**（EOF/错误）才退出：宿主要在插件退出前打开
返回的映射拷像素，谁先退出谁毁掉映射。因此宿主侧请给 editor 类插件配**长超时**
（会话时长 = 用户画多久）。

## 构建

由根 `build.ps1` 调用本目录 `build.ps1`：clippy `-D warnings` → cargo test →
release 构建，产物落 `<workspace>/.temp/plugin-dist/com.rupix.doodle/`，并打包成
`.build/docs/com.rupix.doodle_<version>.zip`（版本见 `manifest.json`）。

## 无头单测

- 共享内存：头序列化/校验、真实 Create/OpenFileMapping 写读往返
- 光栅化：圆头线段/橡皮/越界钳制在小缓冲上的像素断言、历史重放与清空语义
- 协议：请求 JSON 解析（含非法前缀/尺寸拒绝）、响应形状
