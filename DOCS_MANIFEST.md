# 文档登记簿（RuPix-Plugin）

> 文件 | 何时更新 | 谁验证

| 文件 | 何时更新 | 谁验证 |
|---|---|---|
| `README.md` | 仓库定位、**发布表**（大小 `x.xx MB`，1 MB = 1024² B）、目录树、文档索引变化（门面页，技术细则一律去 `docs/`） | `python tools/check_docs.py`（第 4 查按 `fmt_size_mb` 换算对 `plugins.json`）+ 自检 |
| `AGENTS.md` | 仓库边界、插件形态、安全底线、构建规则变化 | `python tools/check_manifests.py` + 自检 |
| `插件开发规范.md` | manifest / 能力 / 协议 / 超时 / 版本 / 安全变化 | 与 `protocol/` 交叉核对 |
| `开发指南.md` | 新增、构建、验收、发布、改协议顺序变化 | 自检 |
| `protocol/manifest.schema.json` | manifest 字段或校验规则变化 | `python tools/check_manifests.py` |
| `protocol/protocol-v1.md` | 管道帧、请求响应、共享内存布局变化 | 与宿主 `docs/扩展体系.md` 交叉核对 |
| `plugins/README.md` | 插件目录约定或清单变化 | 自检 |
| `plugins/<id>/README.md` | 该插件的当前能力、限制、构建与冒烟方式变化 | `python tools/check_docs.py`（存在性）+ 自检 |
| `plugins/<id>/HISTORY.md` | **该插件每次对外变化**（发 release 的说明文字从这里抄；版本递增同一次加一条） | `python tools/check_docs.py` 第 5 查（三份人读文件齐备、每条标题 `## <版本> — <日期> — protocol N`、首条版本/日期/protocol = manifest、条目不写发布进度） |
| `docs/README.md` | 文档索引或写作口径变化 | `python tools/check_docs.py` |
| `docs/architecture.md` | 进程模型 / 数据线 / 能力 / 设置与快捷键 / 生命周期 / 安装落点变化 | 自检 + 与 `protocol/` 交叉核对 |
| `docs/building.md` | 构建流水线或落点变化 | `python tools/check_docs.py` |
| `docs/testing.md` | 门禁、单测口径、冒烟或真机验收清单变化 | 自检 |
| `docs/releasing.md` | 版本、tag、资产、清单指向或发布检查单变化 | 自检 |
| `docs/licensing.md` | 本仓授权口径或供应链流程变化 | `python tools/check_licenses.py` |
| `docs/pitfalls.md` | 新增踩坑（编号只增） | 自检 |
| `THIRD-PARTY-NOTICES.md` | 新增/升级任何第三方依赖，或**随包**任何外部可执行文件时 | `python tools/check_licenses.py`（版本与许可证实读 `cargo metadata`，白名单对不上即红） |
| `plugins.json` | 插件目录生成规则 / 远程地址变化 | 生成器/自检 |
| `tools/check_secrets.py` | 凭据形态变化（新令牌前缀、新的口令写法） | 它自己的 `--selftest`（六条判据 + 四组不误报 + 失明臂） |
| `tools/make_catalog.py` 的 `license_notice()` | **许可口径变化**（仓库/插件的授权、镜像地址、源码位置写法） | 打包即产出：每枚 ZIP 必含 `LICENSE-NOTICE.md`（生成物，不在仓里，所以没有"忘了放"这种失败形态） |
