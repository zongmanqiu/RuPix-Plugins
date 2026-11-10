# 许可证与供应链（licensing.md）

## 1. 本仓自身的授权（实读，不是政策）

| 位置 | 声明 |
|---|---|
| 仓库根 `LICENSE` | **AGPL-3.0** 全文 |
| 每枚插件 `plugins/<id>/LICENSE` | **MIT**（`Copyright (c) 2026 RuPix`） |
| 每枚 `manifest.json` 的 `license` | `MIT` |

⇒ 现状是"仓库 AGPL、插件目录各自 MIT"，同一著作权人分别授权（插件是可单独取用的产物）。

★ **每枚 ZIP 里放一份 `LICENSE-NOTICE.md`**（2026-11-01 定稿）：只说两个事实——这个产物的许可
（MIT，整仓 AGPL-3.0）、对应源码在哪个 tag 的哪个目录（两个镜像各一条链接）。内容由
`tools/make_catalog.py::license_notice()` **在打包时生成**，不是仓里的一份文件——所以它不会和
清单失配，也不需要人工记得放。它是许可与源码位置的说明，不是法律意见。
★ **不做"三年内书面提供源码"那种要约**（同日定稿）：我们是开源项目，源码公开在网上，
没有需要"另行提供"的东西；那张要约卡是物理分发时代的遗留。**解码器仓同日也撤掉了它自己的
`SOURCE-OFFER.md`**：那边的 `raw` 静态链接 LGPL 的 `rawler`，"重链接材料"是真义务，现在写在包内
`README.md` 的「许可与源码」一节里，不再单立一枚文件。

## 2. 第三方组件

**政策与家族账的唯一正本在 [`../THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md)**：登记当前各枚插件的直接依赖、
许可证家族账、以及"加依赖的规矩"。逐包全集**不手抄**，用：

```
python tools/check_licenses.py --list
```

★ **每枚 ZIP 里另带一份 `THIRD-PARTY-NOTICES.md`——那枚插件自己的逐包表**（包名 / 版本 /
SPDX 表达式）。理由：crate 是**静态编进 `<id>.exe`** 的，宽松许可（MIT / Apache-2.0 / BSD …）
要的就是"许可识别随作品走到收件人手里"；只指回仓库根是这件事的弱形态。这份表由
`tools/make_catalog.py::third_party_notices()` 从 `cargo metadata` **现生成**，用的就是
`tools/check_licenses.py` 那同一份解析 ⇒ 表与门禁不可能各说一套，也不存在"忘了更新"。
纯资源插件（没有 `Cargo.toml`）不带这份表，打包时会打印一句原因。

## 3. 门禁（`tools/check_licenses.py`）

- 自己扫 `plugins/*/Cargo.toml` 的解析结果（含传递依赖）；
- 没 `license` 字段、或许可证 id 不在 `ALLOWED` 里 ⇒ **红灯**；
- **扫不到任何 manifest 判失败**（"没东西可查"与"没查到东西"不许长得一样）；
- 失败臂由 `--selftest` 自证（不在白名单 / 无许可证 / AND-OR 复合表达式 / 老式斜杠 /
  本仓自写 crate 不判 / 空目录算失明 / 目录不存在算失明 / 本仓现状要扫得到货）。

## 4. 随包外部可执行文件

要在插件目录里放一枚**不是本仓写的**可执行文件（解码/识别工具之类），必须先在本仓
`THIRD-PARTY-NOTICES.md` 给它立一节（来源 / 版本 / 许可证 / 获取方式）——
**那一节不存在 ⇒ 不随包**（AGENTS §三.7）。纯 Rust crate 依赖由门禁自动覆盖，不需要手工登记。

现状：**只有 `ocr` 是例外**——它随包带 `engine/` 子目录（一枚外部 CLI 与配套模型/字典），
已在 [`../THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md) 的「随包外部可执行文件与模型」一节
立节登记。其余插件的产物仍只有各自的 `<id>.exe` + `manifest.json`，没有随包外部可执行文件
（条码解码是编进 `scan.exe` 的 `rxing`）。

## 5. 加依赖的判定顺序

1. 能进 `ALLOWED` 吗？（MIT / Apache-2.0 / BSD-3-Clause / ISC / MPL-2.0 / Unlicense /
   Unicode-3.0 / Zlib / CC0-1.0 / BSL-1.0）
2. 体积与传递依赖可接受吗？（插件是独立进程，不影响宿主体积，但影响下载与冷启动）
3. 装得进插件自己的进程吗？（不解码文件、不联网除非声明）

三条有一票否决：进不了白名单的**换依赖**，不是"加一行白名单"。
