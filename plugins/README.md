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

当前提供：

| id | 用途 | 状态 |
|---|---|---|
| `checkup` | 图片体检：六段读数进结果窗（命令型，吃宿主下发的整图像素） | 已建（2026-10-31） |
| `scan` | 条码识别：一维码 / QR / DataMatrix / Aztec / PDF417（命令型，整图多码） | 已建（2026-10-31） |

`checkup` 与 `scan` 是第一批**真的用到 `image.read` 像素**的插件：命令型拿像素这条通道
由它们验证，换血（先建两枚、后删三枚）的账与顺序只写在宿主 `main/docs/待办.md` §25 一处。

新增插件步骤见根 [`开发指南.md`](../开发指南.md)。
