# 发布（releasing.md）

> 目标：**宿主不需要为某枚插件重发版**。插件的一切（版本、能力、设置项、快捷键）
> 都写进它自己的 `manifest.json`，宿主只读清单。

## 1. 一次发布 = 五步

1. **改版本**：`plugins/<id>/manifest.json` 的 `version` 递增（只增不回退；
   语义化不强制，但同一版本号不许换内容——已发布资产按 tag 冻结）。
2. **构建**：根 `build.ps1` 全绿（三条门禁 + 各插件单测）。
3. **产出**：`../.build/docs/<id>_<ver>.zip` 与 `plugins.json`（两份清单会同时更新：
   `.build/docs/` 里给宿主用，仓根那份入库给源码读者看）。ZIP 内除 `manifest.json` + 入口外，
   还带插件自己的 `README.md` / `LICENSE`、生成的 `LICENSE-NOTICE.md`（许可与源码位置）与
   生成的 `THIRD-PARTY-NOTICES.md`（这枚插件的逐包许可表，见 [`licensing.md`](licensing.md) §2）
   ——**这四样缺一样就是发布包不完整**。
4. **打 tag 并上传资产**：tag 名 = `<id>_<version>`，资产 = 同名 ZIP。
   - 上传动作由维护者自己做（本仓不代传）。**两个镜像都要传**——清单里有 `gitee` / `github`
     两列地址，只传一列等于另一半用户 404。
   - **清单先指新 tag ⇒ 新 tag 上线前下载 404**：顺序是"先上传资产，再让清单指过去"，
     或者接受新版本条目短时间内 404（宿主下载失败会如实报，不会静默）。
   - ★ **文档要在这一步之前定稿**：ZIP 里塞了插件自己的 `README.md` / `LICENSE`，
     改一个字 ⇒ ZIP 字节变 ⇒ `size` / `sha256` 变 ⇒ 已上传的资产与清单失配（见
     [`pitfalls.md`](pitfalls.md) P9）。
5. **复核**：宿主「插件 → 可下载」卡应看到新版本并可「升级」。

## 2. 版本与兼容

- 插件协议 `protocol` 落在宿主支持的区间（当前 `1..=1`）。**下界永不抬**：
  旧插件在宿主升级后继续可跑。
- `min_host`：本插件需要的最低宿主版本。**能力是加法式的**——老宿主不认识的新字段会被忽略，
  所以新增 `settings` / 新能力位不需要抬 `min_host`，除非插件**依赖**宿主的新行为。
- 宿主侧永远向后兼容：新增的能力只走"新可选字段/新响应字段"，旧插件收不到就用缺省。

## 3. 不许做的事

- **不撤已发布的 tag 与附件**：老用户按清单取包，撤了就是对外破坏。
  插件确实坏了就发新版本，旧版本留在那里。
- **不把源码塞进 ZIP**：ZIP 是运行产物（exe + manifest + 人读文件）。
- **不改已发布版本的 manifest**：要改就升版本号。

## 4. 清单（`plugins.json`）的字段与校验

顶层**就是一个数组，一项一枚插件**。生成器 `tools/make_catalog.py` 从
`plugins/<id>/manifest.json` 抄语义字段、从 ZIP 算 `size` / `sha256`，两条下载地址自己拼。

| 字段 | 必填 | 谁写 | 宿主怎么用 |
|---|---|---|---|
| `id` | 是 | manifest | 安装目录名（`<exe>/plugins/<id>/`）；解析期过 `validate_id` |
| `name` | 是 | manifest | 界面上的显示名（列表行、信息窗、菜单项） |
| `version` | 是 | manifest | 与已装版本比大小 ⇒ 决定「可下载 / 升级 / 已安装 / 本机更新」 |
| `protocol` | 是 | manifest | 握手用；落在宿主区间 `1..=1` 之外 ⇒ 拒载 |
| `kind` | 是 | manifest | `command` / `editor` / `panel` ⇒ 决定走哪条调用路径 |
| `file` | 是 | 生成器（`<id>_<version>.zip`） | ZIP 文件名；**含 `/`、`\` 或 `..` 直接判清单坏** |
| `min_host` | 否 | manifest | 宿主版本低于它 ⇒ 入口整体关闭（置灰标原因）；缺省空串当"不限" |
| `size` | 否 | 生成器（ZIP 字节数） | 下载校验之一，挡截断 / 多收；缺省 0 |
| `sha256` | 否 | 生成器（ZIP 摘要） | 下载校验之二，挡**同字节数的替换**；缺省空串 |
| `description` | 否 | manifest | 「!」信息窗那一行 |
| `capabilities` | 否 | manifest | 信息窗列能力；宿主按 manifest（已装的那份）判定给不给通道 |
| `released` | 否 | manifest | 信息窗的发行日期 |
| `author` / `license` | 否 | manifest | 信息窗的署名与许可 |
| `gitee` / `github` | 是（生成器） | 生成器 | 两条下载地址；点「下载」按选项页「下载源」选哪一列 |

- **`size` 与 `sha256` 并用不是重复**：只看字节数，一枚同样长的恶意包就过了；只看 sha，
  截断到一个不同长度时报警太晚。宿主两者都核（`extensions::install`），不匹配就**不落盘**。
- **远程优先、本地兜底**：点「下载」先按所选来源联网；联网失败而 `docs/<file>` 里有同一枚包
  ⇒ 用本地那份装（404 不该把用户卡死）。两条都没有才报"这一枚在当前来源还没有发布地址"。
- **单枚 ZIP 上限 512 MiB**（宿主 `extensions::install::MAX_BYTES`）：超了直接拒收，
  不是"慢一点"。今天两枚都在 1 MiB 量级。
- **`gitee` 是主源、`github` 由它推导**：两个镜像的 **owner 不同**（Gitee `qiuzongman` /
  GitHub `zongmanqiu`），推导时**主机名与 owner 一起换**。`released` 与 `gitee` 由人填。
- **仓库名是复数 `RuPix-Plugins`，本地目录是单数 `RuPix-Plugin/`**；默认分支 `main`。
  写地址前先探一次（见 [`pitfalls.md`](pitfalls.md) P7）。
- 宿主内置的默认刷新地址（用户可在 ini 里改 `plugin_index_gitee` / `plugin_index_github`）：
  - Gitee：`https://raw.giteeusercontent.com/qiuzongman/RuPix-Plugins/raw/main/plugins.json`
  - GitHub：`https://raw.githubusercontent.com/zongmanqiu/RuPix-Plugins/refs/heads/main/plugins.json`
- **清单只收有构建产物的插件**：`tools/make_catalog.py` 按 `../.temp/plugin-dist/<id>/`
  有没有货来收，缺产物打印 `skip` 而不是静默略过 ⇒ "少了一枚"这件事在构建输出里就看得见。
- **根 `plugins.json` 与 `.build/docs/plugins.json` 是同一次生成的两份**：仓根那份入库给源码
  读者看，`.build/docs/` 那份随宿主分发（离线兜底）。★ 清单里数字的真源只有 `plugins.json`；
  根 README 的「发布插件」表是**门面**，由 `python tools/check_docs.py` 第 4 查逐格判等值
  （版本 / 形态 / 字节数 / 日期 / 两条地址），漂了就红灯——**不许再往别处抄第三份**。

## 5. 发布检查单

- [ ] 各插件 `version` 已递增，`id` 与目录名一致
- [ ] `build.ps1` 全绿（manifests / licenses / docs / 单测）
- [ ] 冒烟两腿通过（见 [`testing.md`](testing.md) §3）
- [ ] 真机四条通过（§4）
- [ ] ZIP 已上传到 `<id>_<version>` tag
- [ ] `plugins.json` 里的下载地址可达（点一次 head 请求）
- [ ] 本仓 `README` 的「发布插件」表与清单一致（`tools/check_docs.py` 第 4 查自动判）
- [ ] 两镜像都传了（只传 Gitee ⇒ 选 GitHub 源的用户 404）
