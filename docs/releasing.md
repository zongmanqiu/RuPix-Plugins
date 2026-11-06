# 发布（releasing.md）

> 目标：**宿主不需要为某枚插件重发版**。插件的一切（版本、能力、设置项、快捷键）
> 都写进它自己的 `manifest.json`，宿主只读清单。

## 1. 一次发布 = 五步

1. **改版本**：`plugins/<id>/manifest.json` 的 `version` 递增（只增不回退；
   语义化不强制，但同一版本号不许换内容——已发布资产按 tag 冻结），
   **同一次**在 `plugins/<id>/HISTORY.md` 顶部加一条 `## <新版本> — <日期> — protocol N`，
   写这一版对外变了什么（发 release 时那段说明就从这里抄）。漏加、标题格式不对、
   首条三字段与 manifest 不符、或 `README.md` / `HISTORY.md` / `LICENSE` 少一份，
   都由 [`check_docs.py`](testing.md) 第 5 查判红（开发指南 §一 第 3/4 步）。
2. **构建**：根 `build.ps1` 全绿（四条门禁 + 各插件单测）。
3. **产出**：`../.build/docs/<id>_<ver>.zip` 与 `plugins.json`（两份清单会同时更新：
   `.build/docs/` 里给宿主用，仓根那份入库给源码读者看）。ZIP 内除 `manifest.json` + 入口外，
   还带插件自己的 `README.md` / `LICENSE`、生成的 `LICENSE-NOTICE.md`（许可与源码位置）与
   生成的 `THIRD-PARTY-NOTICES.md`（这枚插件的逐包许可表，见 [`licensing.md`](licensing.md) §2）
   ——**这四样缺一样就是发布包不完整**。
4. **打 tag 并上传资产**：tag 名 = `<id>_<version>`，资产 = 同名 ZIP。
   - 上传动作由维护者自己做（本仓不代传）。**两个镜像都要传**——清单里有 `gitee` / `github`
     两列地址，只传一列等于另一半用户 404。
   - ★ 源码、清单、tag **只推 Gitee**，GitHub 是自动从 Gitee 复制并推送的镜像；
     但镜像不复制 release 对象，所以「两个镜像都要传」专指 ZIP 附件与说明正文，
     GitHub 侧在镜像过来的 tag 上新建或编辑 release，不另建 tag。见根
     [`开发指南.md`](../开发指南.md) §四。
   - **清单先指新 tag ⇒ 新 tag 上线前下载 404**：顺序是"先上传资产，再让清单指过去"，
     或者接受新版本条目短时间内 404（宿主下载失败会如实报，不会静默）。
   - ★ **文档要在这一步之前定稿**：ZIP 里塞了插件自己的 `README.md` / `LICENSE`，
     改一个字 ⇒ ZIP 字节变 ⇒ 已上传的资产换了（清单只记 `size` 与地址，不会再失配，
     所以这一步从"必须同步清单"降级成"记得重传资产"——见 [`pitfalls.md`](pitfalls.md) P9）。
5. **复核**：宿主「插件 → 可下载」卡应看到新版本并可「升级」。

★ **发布 ZIP 一律由 7-Zip 生成**（2026-11-04 起）：上面第 3 步的 `make_catalog.py` 调
7-Zip 命令行（`7z` / `7za` / `7zz`，PATH 里找不到时可用 `RUPIX_7Z` 指到可执行文件），
参数是 `a -tzip -mx=9 -mtm=off -mta=off -mtc=off`。产物仍是**标准 ZIP / Deflate**、文件名
仍是 `<id>_<version>.zip`，不是 `.7z`，宿主解压链路一字未改。换工具只为压缩率：同样一份
内容，最高级 Deflate 实测比原先默认级别小约 4%（851 KB DLL：7-Zip 371,876 B，Python 默认
387,583 B）。找不到 7-Zip 时脚本**报错退出，不静默回退**；前置依赖见
[`building.md`](building.md)。

★ **打包仍是确定性的**：`pack_zip()` 在打包前把暂存目录（含目录自身）的 mtime 压到
`1980-01-01`，再关掉高精度时间戳字段；同内容连打两次逐字节一致（实测）。
**2026-11-04 已全量重打并上架（Gitee）**：当时 `../.build/docs/` 的三枚 ZIP 均由 7-Zip 生成，
`plugins.json` 的 `size` 与根 README 门面表已同步到新字节数（`check_docs.py` 第 4 查判
等值），并经 Gitee OpenAPI 传到各自的 `<id>_<version>` tag：已有的替换同名附件，
`levels_0.1.5` 新建 tag；**release 说明统一取该插件 `HISTORY.md` 里对应版本那一条**。
宿主按 `id` + `version` 识别，不会因压缩器不同而拒收。上面第 4 步说的**两个镜像都要传**
仍是规则：**2026-11-04 晚 GitHub 镜像也已同步到位**——当时的三枚 ZIP 换成同一批 7-Zip 产物，
已有的 tag 覆盖同名附件，`levels_0.1.5` 在已存在的 tag 上新建 release；说明同样取该插件
`HISTORY.md` 里对应版本那一条。两侧资产与说明按 tag 逐一对齐。

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
`plugins/<id>/manifest.json` 抄语义字段、从 ZIP 取 `size`，两条下载地址自己拼。

| 字段 | 必填 | 谁写 | 宿主怎么用 |
|---|---|---|---|
| `id` | 是 | manifest | 安装目录名（`<exe>/plugins/<id>/`）；解析期过 `validate_id` |
| `name` | 是 | manifest | 界面上的显示名（列表行、信息窗、菜单项） |
| `version` | 是 | manifest | 与已装版本比大小 ⇒ 决定「可下载 / 升级 / 已安装 / 本机更新」 |
| `protocol` | 是 | manifest | 握手用；落在宿主区间 `1..=1` 之外 ⇒ 拒载 |
| `kind` | 是 | manifest | `command` / `editor` / `panel` ⇒ 决定走哪条调用路径 |
| `file` | 是 | 生成器（`<id>_<version>.zip`） | ZIP 文件名；**含 `/`、`\` 或 `..` 直接判清单坏** |
| `min_host` | 否 | manifest | 宿主版本低于它 ⇒ 入口整体关闭（置灰标原因）；缺省空串当"不限" |
| `size` | 否 | 生成器（ZIP 字节数） | **只作显示**；2026-11-02 起不参与任何判定 |
| `description` | 否 | manifest | 「!」信息窗那一行 |
| `capabilities` | 否 | manifest | 信息窗列能力；宿主按 manifest（已装的那份）判定给不给通道 |
| `released` | 否 | manifest | 信息窗的发行日期 |
| `author` / `license` | 否 | manifest | 信息窗的署名与许可 |
| `gitee` / `github` | 是（生成器） | 生成器 | 两条下载地址；点「下载」按选项页「下载源」选哪一列 |

- **清单里没有哈希**（2026-11-02 起）：插件包全部由本仓 `build.ps1` 产出、release 只有
  作者能发，指纹挡的"第三方替换/篡改"在这条链路上没有入口；而 MSVC 每次链接都换 PDB GUID，
  同源码重编字节必变，清单与资产会天天失配。识别一枚插件只认 `id` + `version`，宿主侧
  `extensions::install` 已同步删掉 `size` / `sha256` 校验（包身份由 ZIP 里的 `manifest.json`
  核 `id` 兜住）。解码器仓同日同口径。
- **远程优先、本地兜底**：点「下载」先按所选来源联网；联网失败而 `docs/<file>` 里有同一枚包
  ⇒ 用本地那份装（404 不该把用户卡死）。两条都没有才报"这一枚在当前来源还没有发布地址"。
- **单枚 ZIP 上限 512 MiB**（宿主 `extensions::install::MAX_BYTES`）：超了直接拒收，
  不是"慢一点"。今天四枚都在 1 MiB 量级。
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
- [ ] 每枚插件三份人读文件齐备、`HISTORY.md` 顶部已加本次版本那一条
      （标题 `## <版本> — <日期> — protocol N`；首条版本 / 日期 / protocol = manifest）
- [ ] `build.ps1` 全绿（manifests / licenses / **secrets** / docs / 单测）
- [ ] 冒烟两腿通过（见 [`testing.md`](testing.md) §3）
- [ ] 真机四条通过（§4）
- [ ] ZIP 已上传到 `<id>_<version>` tag
- [ ] `plugins.json` 里的下载地址可达（点一次 head 请求）
- [ ] 本仓 `README` 的「发布插件」表与清单一致（`tools/check_docs.py` 第 4 查自动判）
- [ ] 两镜像都传了（只传 Gitee ⇒ 选 GitHub 源的用户 404）
