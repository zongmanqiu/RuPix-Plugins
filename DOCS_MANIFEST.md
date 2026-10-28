# 文档登记簿（RuPix-Plugin）

> 文件 | 何时更新 | 谁验证

| 文件 | 何时更新 | 谁验证 |
|---|---|---|
| `README.md` | 仓库定位、目录、构建入口、文档地图变化 | 自检 |
| `AGENTS.md` | 仓库边界、插件形态、安全底线、构建规则变化 | `python tools/check_manifests.py` + 自检 |
| `插件开发规范.md` | manifest / 能力 / 协议 / 超时 / 版本 / 安全变化 | 与 `protocol/` 交叉核对 |
| `开发指南.md` | 新增、构建、验收、发布、改协议顺序变化 | 自检 |
| `protocol/manifest.schema.json` | manifest 字段或校验规则变化 | `python tools/check_manifests.py` |
| `protocol/protocol-v1.md` | 管道帧、请求响应、共享内存布局变化 | 与宿主 `docs/扩展体系.md` 交叉核对 |
| `plugins/README.md` | 插件目录约定或清单变化 | 自检 |
| `plugins.json` | 插件目录生成规则 / 远程地址变化 | 生成器/自检 |
