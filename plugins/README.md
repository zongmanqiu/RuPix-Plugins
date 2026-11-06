# plugins/

每个发布插件一个目录，目录名 = manifest 的 `id`。

```text
plugins/<id>/
├─ manifest.json        # 必须（唯一被宿主解析的文件）
├─ <entry>.exe          # command / editor 必须
├─ build.ps1            # 可选：产出 ../.temp/plugin-dist/<id>/
├─ src/                 # 可选：源码
├─ assets/              # 可选：模型/资源
├─ README.md            # 插件自己的说明（随 ZIP 走，给用户看）
├─ HISTORY.md           # 版本历史（不随 ZIP；发 release 的说明文字从这里抄）
└─ LICENSE              # 随 ZIP 走
```

当前提供：

| id | 用途 | 状态 |
|---|---|---|
| `checkup` | 图片体检：六段读数进结果窗（命令型，吃宿主下发的整图像素） | 已建（2026-10-31） |
| `levels` | 色阶分析：R / G / B / 亮度四组分布曲线 + 文本报告，回传图片由宿主开新窗 | 已建（2026-11-04） |
| `scan` | 条码识别：一维码 / QR / DataMatrix / Aztec / PDF417（命令型，整图多码） | 已建（2026-10-31） |
| `channels` | 通道调色：R / G / B 增益与偏置、声明式实时预览（**编辑型**，第一枚 `run_ui` 插件） | 已建（2026-11-06） |

四枚都声明 `image.read`；差异在能力组合：三枚命令型只**读**像素，`levels` 是第一枚
回传图片的插件，`channels` 另带 `image.write` + `settings.read` / `settings.write`
（`type=int` + `min/max` ⇒ 滑块，`type=hotkey` ⇒ 动态热键；`preview` / `presets` 走显示层变换）。
★ `channels` 的实时调色只作用在**屏上那份位图**，不吃整图 ⇒ 不受 8 MP 闸；
正式整图编辑仍走「整图发插件进程」那条老路，才受闸。
插件清单的演进记在宿主 `main/docs/待办.md` §25。

新增插件步骤见根 [`开发指南.md`](../开发指南.md)。
