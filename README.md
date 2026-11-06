# RuPix 插件仓库

> RuPix 看图器的**独立进程插件**源码与发布清单。
> [**Gitee**](https://gitee.com/qiuzongman/RuPix-Plugins) | [**GitHub**](https://github.com/zongmanqiu/RuPix-Plugins)
>
> 普通用户不用碰这个仓库：宿主「设置 → 插件 → 可下载」点「下载」即可。

## 发布插件

| 插件 | 版本 | 形态 | ZIP 大小 | 发行日期 | Gitee | GitHub |
|---|---:|---:|---:|---:|---|---|
| `checkup`（图片体检） | 0.1.0 | 命令型 | 140,159 B | 2026-10-31 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/checkup_0.1.0/checkup_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/checkup_0.1.0/checkup_0.1.0.zip) |
| `scan`（条码识别） | 0.1.0 | 命令型 | 798,029 B | 2026-10-31 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/scan_0.1.0/scan_0.1.0.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/scan_0.1.0/scan_0.1.0.zip) |
| `levels`（色阶分析） | 0.1.6 | 命令型 | 125,708 B | 2026-11-06 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/levels_0.1.6/levels_0.1.6.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/levels_0.1.6/levels_0.1.6.zip) |
| `channels`（通道调色） | 0.1.1 | 编辑型 | 108,679 B | 2026-11-06 | [下载](https://gitee.com/qiuzongman/RuPix-Plugins/releases/download/channels_0.1.1/channels_0.1.1.zip) | [下载](https://github.com/zongmanqiu/RuPix-Plugins/releases/download/channels_0.1.1/channels_0.1.1.zip) |

表中数字与地址抄自 [`plugins.json`](plugins.json)（真源在清单里），由
`python tools/check_docs.py` 逐格对账。发布与上架步骤见 [`docs/releasing.md`](docs/releasing.md)。

## 仓库结构

```text
RuPix-Plugin/
├─ plugins/          每枚插件一个目录（一览见 plugins/README.md）
├─ protocol/         manifest / 请求响应 / 共享内存契约
├─ docs/             贡献者长期文档（入口 docs/README.md）
├─ tools/            四道门禁 + 清单生成器
├─ build.ps1         根构建入口
├─ plugins.json      可下载清单
└─ AGENTS.md         本仓库硬规则
```

## 文档

| 想知道 | 去哪 |
|---|---|
| 写一个插件（manifest / 协议 / 能力） | [`插件开发规范.md`](插件开发规范.md) |
| 契约逐字正本 | [`protocol/protocol-v1.md`](protocol/protocol-v1.md) |
| 新增、构建、验收、发布的顺序 | [`开发指南.md`](开发指南.md) |
| 架构 / 构建 / 测试 / 发布 / 许可 / 踩坑 | [`docs/README.md`](docs/README.md) |
| 插件一览 | [`plugins/README.md`](plugins/README.md) |
| 全部文档的登记簿 | [`DOCS_MANIFEST.md`](DOCS_MANIFEST.md) |

## 构建

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

细节见 [`docs/building.md`](docs/building.md)。

## 许可证

本仓库 **AGPL-3.0**（[`LICENSE`](LICENSE)），插件目录各自 **MIT**；
授权口径与第三方归属见 [`docs/licensing.md`](docs/licensing.md)。
