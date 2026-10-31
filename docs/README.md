# 插件仓文档索引（docs/README.md）

> 面向**本仓贡献者**的长期文档。用户/使用者看根 [`README.md`](../README.md)；
> 契约正本是 [`../插件开发规范.md`](../插件开发规范.md) 与 [`../protocol/`](../protocol/)。
> 登记簿见 [`../DOCS_MANIFEST.md`](../DOCS_MANIFEST.md)。

| 我想… | 看 |
|---|---|
| 懂这个仓的结构与运行模型（进程外、管道、共享内存） | [`architecture.md`](architecture.md) |
| 从零构建一遍 | [`building.md`](building.md) |
| 改完代码怎么验（门禁 / 单测 / 冒烟） | [`testing.md`](testing.md) |
| 发一个版本（tag、ZIP、清单指向） | [`releasing.md`](releasing.md) |
| 弄清许可证与第三方归属 | [`licensing.md`](licensing.md) |
| 踩过的坑（先读再写代码） | [`pitfalls.md`](pitfalls.md) |

## 文档写作口径

1. **能机器判的写进工具**（`tools/check_*.py`），文档只写理由与流程——同一件事不写两份。
2. 版本号、大小、件数这类**会变的数字**不手抄进 `docs/`；要引用就写"用某命令读"。
   唯一例外是根 [`README.md`](../README.md) 的「发布插件」门面表（与解码器仓同一口径：表是给人
   找下载地址的入口，真源是 `plugins.json`）——它由 `tools/check_docs.py` 第 4 查逐格判等值，
   漂了就红灯。**没有守卫的第二份数字才是真源漂移，有守卫的门面表不是。**
3. 现行文档不写考古（"曾经…""上一版…"）：历史进 `README` 的更新记录或宿主 `HISTORY.md`。
4. 路径一律相对本仓或 `<宿主>` 占位，不写本机绝对路径（`tools/check_secrets.py` 会拦）。
5. 表格里抄来的数字旁边**写清它抄自哪里**（如"字节数抄自 `plugins.json`"），读者才知道该信谁。
