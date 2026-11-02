# 构建（building.md）

> 一条命令：`powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1`
> （根目录执行；脚本全是 ASCII，PowerShell 5.1 按 ANSI 读无 BOM 脚本）。

## 1. 流水线

```
build.ps1
 ├─ python tools/check_manifests.py     # manifest 结构/字段/能力白名单/id=目录名
 ├─ python tools/check_licenses.py      # 供应链：每个插件的解析依赖都必须有可再分发的许可证
 ├─ python tools/check_secrets.py       # 无密钥/无隐私：私钥块 / 令牌 / URL 凭据 / 口令字面量 / 邮箱 / 含真名路径
 ├─ python tools/check_docs.py          # 文档：链接可达 + 清单登记 + 不写本机绝对路径 + README 门面表对账
 ├─ 逐个 plugins/<id>/build.ps1         # cargo test --release → cargo build --release
 │   └─ 产物 → ../.temp/plugin-dist/<id>/{<entry>.exe, manifest.json}
 └─ python tools/make_catalog.py        # 打包 ../.build/docs/<id>_<ver>.zip + plugins.json
                                        #（ZIP 内另附 README / LICENSE / 生成的 LICENSE-NOTICE.md）
```

## 2. 目录与落点（便携铁律）

| 东西 | 落点 | 说明 |
|---|---|---|
| 源码 | `plugins/<id>/` | 每枚插件一个独立 cargo 包（各自 `Cargo.lock`，无 workspace） |
| 编译中间物 | `../.temp/plugin-target/<id>/` | **不落源码目录**（各插件 `build.ps1` 用 `--target-dir` 钉死） |
| 发布暂存 | `../.temp/plugin-dist/<id>/` | 根脚本从这里取货打包 |
| 最终发布物 | `../.build/docs/<id>_<ver>.zip` + `plugins.json` | 宿主从清单读 |
| 源码仓镜像清单 | `plugins.json`（本仓根） | 与 `.build/docs/plugins.json` 内容相同，入库 |

## 3. 单插件构建

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File plugins/checkup/build.ps1
```

每枚插件的 `build.ps1` 做三件事：跑它自己的单测（`--test-threads=1`，见 testing.md）、
`cargo build --release`、把 exe 与 manifest 拷到 `../.temp/plugin-dist/<id>/`。

★ **打包是确定性的**：`make_catalog.py` 把 ZIP 条目的时间戳钉死成 `1980-01-01`，所以
**同样内容 ⇒ 同样字节**（连打两次实测一致）。这条现在只服务"可复现"——2026-11-02 起清单
与宿主都不校验哈希，同内容重打能确认"改的到底是内容还是时间戳"。
日常只改一枚插件就只重打那一枚（`plugins/<id>/build.ps1` + `make_catalog.py`），
另一枚的字节不会被动过。

## 4. 加一枚插件

1. 建 `plugins/<id>/`：`manifest.json` + 入口源码 + `build.ps1` + `LICENSE` + `README.md` + `HISTORY.md`。
2. `manifest.json` 的 `id` 必须**等于目录名**（门禁会查），入口必须是目录内相对路径的 `.exe`；
   `HISTORY.md` 的首条版本必须等于 manifest 的 `version`（`check_docs.py` 第 5 查会判）。
3. 如果声明了 `settings`，必须同时声明 `settings.read` + `settings.write`
   （`settings_ui` 那条老路与它二选一）。
4. 引入任何第三方依赖前先读 [`licensing.md`](licensing.md)：进不了白名单＝判不采用。
5. 跑根 `build.ps1` 全绿，再把 ZIP 上传到发布仓对应 tag（见 [`releasing.md`](releasing.md)）。

## 5. 常见失败

| 现象 | 原因 | 修法 |
|---|---|---|
| `check_manifests` 报 id ≠ 目录 | 改名后没同步 | 两处改成同一个 |
| `check_licenses` 报某许可证不在白名单 | 新依赖带了 GPL 系 / 无 license 字段 | 换依赖，而不是往白名单里塞 |
| 构建产物找不到 | `build.ps1` 没拷到 `../.temp/plugin-dist/<id>/` | 对照本文件 §3 |
| PowerShell 报中文乱码/语法错 | 脚本里写了非 ASCII | `.ps1` 一律 ASCII，注释写英文 |
