# plugins/

每个发布插件一个目录，目录名 = manifest 的 `id`。

```text
plugins/<id>/
├─ manifest.json        # 必须
├─ <entry>.exe          # command / editor 必须
├─ build.ps1            # 可选：产出 ../.temp/plugin-dist/<id>/
├─ src/                 # 可选：源码
├─ assets/              # 可选：模型/资源
└─ README.md            # 插件自己的说明
```

当前规划：

| id | 用途 | 状态 |
|---|---|---|---|
| `com.rupix.echo` | 诊断：回显宿主请求 | 已建 |
| `com.rupix.ocr` | Windows 本地 OCR（>4MP 先软件降采样） | 已建 |
| `com.rupix.doodle` | 独立窗口涂鸦（编辑型） | 已建 |

新增插件步骤见根 [`开发指南.md`](../开发指南.md)。
