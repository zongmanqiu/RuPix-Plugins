#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Documentation gate: links resolve, every doc is registered, no local paths.

Five questions, all mechanical (no semantic guessing):

1. Every relative markdown link in the tracked docs points at a file that exists.
2. Every `.md` in the repo root / docs/ has a row in DOCS_MANIFEST.md.
3. No absolute local path (drive letter or /home/…) leaked into a doc.
4. The README "发布插件" table matches plugins.json row for row (id set, version,
   kind, exact bytes, release date, both mirror download URLs).
5. Every plugin directory ships a `HISTORY.md` whose first entry is that plugin's
   current version and release date — and its entries carry no release-progress
   status (that truth lives in `plugins.json` only).

Exit codes: 0 = clean; 1 = violation, or the gate could not see anything to check.

Usage:
    python tools/check_docs.py
    python tools/check_docs.py --selftest
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"

# 只查这些根目录下的 md：插件自己的 README 随包发布、格式自由，不在本门禁范围。
TRACKED_ROOTS = [ROOT]
LINK = re.compile(r"\]\(([^)\s]+)\)")
ABS_PATH = re.compile(r"(?<![A-Za-z])[A-Za-z]:\\\\|[A-Za-z]:\\\\|/home/|/Users/|/e/tmp", re.IGNORECASE)

RELEASE_SECTION = "## 发布插件"
ROW_ID = re.compile(r"^`([^`]+)`")
ROW_URL = re.compile(r"\]\((https?://[^)]+)\)")
# manifest 的 kind 是机器名，README 用中文门面词；映射写在这里，两边各只有一处定义。
KIND_LABEL = {"command": "命令型", "editor": "编辑型", "panel": "面板型"}


def tracked_docs() -> list[Path]:
    out = [p for p in ROOT.glob("*.md")]
    if DOCS.is_dir():
        out.extend(sorted(DOCS.glob("*.md")))
    return [p for p in out if p.name != "DOCS_MANIFEST.md"]


def label(doc: Path) -> str:
    """仓库内文档报相对路径；临时夹具（自检用）报文件名。"""
    try:
        return doc.relative_to(ROOT).as_posix()
    except ValueError:
        return doc.name


def check_links(docs: list[Path]) -> list[str]:
    bad = []
    for doc in docs:
        text = doc.read_text(encoding="utf-8", errors="replace")
        for target in LINK.findall(text):
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            path = target.split("#", 1)[0]
            if not path:
                continue
            resolved = (doc.parent / path).resolve()
            if not resolved.exists():
                bad.append(f"{label(doc)}: 链接指向不存在的 {target}")
    return bad


def check_registered(docs: list[Path]) -> list[str]:
    manifest = ROOT / "DOCS_MANIFEST.md"
    if not manifest.is_file():
        return ["DOCS_MANIFEST.md 不存在（本查失明）"]
    text = manifest.read_text(encoding="utf-8", errors="replace")
    bad = []
    for doc in docs:
        rel = label(doc)
        if rel not in text:
            bad.append(f"{rel}: 没在 DOCS_MANIFEST.md 登记")
    return bad


FENCE = re.compile(r"```.*?```", re.DOTALL)


def check_local_paths(docs: list[Path]) -> list[str]:
    bad = []
    for doc in docs:
        text = doc.read_text(encoding="utf-8", errors="replace")
        # 围栏代码块里是契约示例（`"C:\\images\\a.png"` 这种），不是本机泄漏。
        text = FENCE.sub("", text)
        for m in ABS_PATH.finditer(text):
            bad.append(f"{label(doc)}: 出现本机绝对路径 {m.group(0)!r}")
    return bad


def release_rows(readme: Path) -> list[list[str]]:
    """README「## 发布插件」小节里的数据行（以 `| \\`id\\`` 开头的那些）。"""
    if not readme.is_file():
        return []
    rows: list[list[str]] = []
    in_section = False
    for line in readme.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("## "):
            in_section = line.strip() == RELEASE_SECTION
            continue
        if not in_section:
            continue
        cell_line = line.strip()
        if not cell_line.startswith("| `"):
            continue
        cells = [c.strip() for c in cell_line.strip("|").split("|")]
        if len(cells) >= 7:
            rows.append(cells)
    return rows


def check_release_table(readme: Path, catalog_path: Path) -> list[str]:
    """README 的门面表与 `plugins.json` 逐格对账——数字抄进文档就会漂，那就机器盯着它。"""
    if not catalog_path.is_file():
        return [f"{catalog_path.name} 不存在（本查失明）"]
    if not readme.is_file():
        return [f"{readme.name} 不存在（本查失明）"]
    try:
        entries = json.loads(catalog_path.read_text(encoding="utf-8"))
    except Exception as e:
        return [f"{catalog_path.name}: JSON 解析失败 {e}"]
    rows = release_rows(readme)
    if not rows:
        return [f"README 的「{RELEASE_SECTION}」表一行都没扫到（本查失明，不是通过）"]

    by_id = {}
    bad: list[str] = []
    for e in entries:
        pid = e.get("id")
        if not pid:
            bad.append("清单里有一项没有 id")
            continue
        if pid in by_id:
            bad.append(f"清单里 id 重复：{pid}")
        by_id[pid] = e

    seen = set()
    for cells in rows:
        m = ROW_ID.match(cells[0])
        if not m:
            bad.append(f"README 行的第一格不以 `id` 形式开头：{cells[0]!r}")
            continue
        pid = m.group(1)
        seen.add(pid)
        entry = by_id.get(pid)
        if entry is None:
            bad.append(f"README 有 {pid}，清单里没有")
            continue
        want = {
            "版本": str(entry.get("version", "")),
            "形态": KIND_LABEL.get(entry.get("kind", ""), entry.get("kind", "")),
            "大小": f"{int(entry.get('size', -1)):,} B",
            "日期": str(entry.get("released", "")),
        }
        got = {"版本": cells[1], "形态": cells[2], "大小": cells[3], "日期": cells[4]}
        for field, value in want.items():
            if got[field] != value:
                bad.append(f"README {pid} 的{field}是 {got[field]!r}，清单是 {value!r}")
        for cell, key in ((cells[5], "gitee"), (cells[6], "github")):
            url = ROW_URL.search(cell)
            if not url:
                bad.append(f"README {pid} 的 {key} 列没有链接")
            elif url.group(1) != entry.get(key):
                bad.append(f"README {pid} 的 {key} 地址与清单不一致")

    for pid in sorted(set(by_id) - seen):
        bad.append(f"清单有 {pid}，README 表里缺行")
    return bad


RELEASE_WORDS = ("待上传", "未发布", "release tag", "发布包已生成", "等待上传")


def check_plugin_history(plugins_dir: Path) -> list[str]:
    """每枚插件目录都要有 `HISTORY.md`，且首条版本 = manifest 的 `version`。

    ★ 为什么要门禁判而不是写在文档里：这格曾经"没有理由地缺着"——发版的人要的就是这一页
    （release 说明从这里抄），少了它等于发布流程缺一块材料，而构建照样全绿。
    条目里还禁写发布进度（`待上传` / `未发布` / `release tag` …）：发布状态的真源是
    `plugins.json`，历史只记版本事实——写进历史就变成第二份状态，而且一定过期。
    """
    bad: list[str] = []
    if not plugins_dir.is_dir():
        return [f"{plugins_dir.name}/ 不存在（本查失明）"]
    seen = 0
    for src in sorted(p for p in plugins_dir.iterdir() if p.is_dir()):
        manifest = src / "manifest.json"
        if not manifest.is_file():
            continue
        seen += 1
        pid = src.name
        try:
            meta = json.loads(manifest.read_text(encoding="utf-8-sig"))
        except Exception as e:
            bad.append(f"{pid}: manifest 读不出（{e}）")
            continue
        history = src / "HISTORY.md"
        if not history.is_file():
            bad.append(f"{pid}: 缺 HISTORY.md（发 release 要抄的就是这一页）")
            continue
        text = history.read_text(encoding="utf-8", errors="replace")
        headings = [ln for ln in text.splitlines() if ln.startswith("## ")]
        if not headings:
            bad.append(f"{pid}: HISTORY.md 里没有任何 `## <版本>` 条目（本查失明）")
            continue
        want = f"## {meta.get('version')}"
        if not headings[0].startswith(want):
            bad.append(f"{pid}: HISTORY 首条是 {headings[0]!r}，manifest 版本是 {meta.get('version')!r}")
        released = meta.get("released") or ""
        if released and released not in headings[0]:
            bad.append(f"{pid}: HISTORY 首条没有发行日期 {released}（应 = manifest 的 released）")
        # 只判条目区（第一条 `## ` 之后），标题里的政策说明允许提这些词。
        body = text.split(headings[0], 1)[1]
        for word in RELEASE_WORDS:
            if word in body:
                bad.append(f"{pid}: HISTORY 条目里出现发布进度词「{word}」（真源是 plugins.json）")
    if seen == 0:
        return ["plugins/ 下一枚插件都没扫到（本查失明，不是通过）"]
    return bad


def run(docs: list[Path]) -> list[str]:
    return (
        check_links(docs)
        + check_registered(docs)
        + check_local_paths(docs)
        + check_release_table(ROOT / "README.md", ROOT / "plugins.json")
        + check_plugin_history(ROOT / "plugins")
    )


def selftest() -> int:
    """失败臂自证：四条检查各自抓得到假样本；本仓现状必须扫得到货。"""
    import tempfile

    cases: list[tuple[str, bool]] = []
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        (tmp / "DOCS_MANIFEST.md").write_text("nothing", encoding="utf-8")
        doc = tmp / "a.md"
        doc.write_text("见 [x](nope.md)，路径 E:\\\\secret\\\\x.txt", encoding="utf-8")
        cases.append(("坏链接要报", len(check_links([doc])) == 1))
        cases.append(("未登记要报", len(check_registered([doc])) == 1))
        cases.append(("本机绝对路径要报", len(check_local_paths([doc])) == 1))
        good = tmp / "b.md"
        good.write_text("见 [a](a.md)", encoding="utf-8")
        cases.append(("好链接不误报", check_links([good]) == []))

        # 第 4 查：README 门面表 ↔ plugins.json。夹具按清单字段拼出"对得上"的那一行，
        # 再逐格改坏——判据是"只判等值"，所以夹具必须与真 README 的列形状一致。
        entry = {
            "id": "alpha",
            "version": "1.2.0",
            "kind": "command",
            "size": 123456,
            "released": "2026-01-02",
            "gitee": "https://gitee.com/o/RuPix-Plugins/releases/download/alpha_1.2.0/alpha_1.2.0.zip",
            "github": "https://github.com/o/RuPix-Plugins/releases/download/alpha_1.2.0/alpha_1.2.0.zip",
        }

        def readme_of(row: str) -> str:
            return "# 插件仓库\n\n## 发布插件\n\n| 插件 | 版本 | 形态 | ZIP 大小 | 发行日期 | Gitee | GitHub |\n" \
                   "|---|---:|---:|---:|---:|---|---|\n" + row + "\n\n## 安装\n\n正文\n"

        def row_of(e: dict) -> str:
            return (
                f"| `{e['id']}`（示例） | {e['version']} | {KIND_LABEL.get(e['kind'], e['kind'])} | "
                f"{e['size']:,} B | {e['released']} | [下载]({e['gitee']}) | [下载]({e['github']}) |"
            )

        cat = tmp / "plugins.json"
        cat.write_text(json.dumps([entry], ensure_ascii=False), encoding="utf-8")
        base = readme_of(row_of(entry))
        rm = tmp / "README.md"
        rm.write_text(base, encoding="utf-8")
        cases.append(("表与清单一致不误报", check_release_table(rm, cat) == []))

        def mutated(**over) -> list[str]:
            e = dict(entry)
            e.update(over)
            rm.write_text(readme_of(row_of(e)), encoding="utf-8")
            return check_release_table(rm, cat)

        cases.append(("版本漂移要报", len(mutated(version="1.3.0")) == 1))
        cases.append(("字节数漂移要报", len(mutated(size=222)) == 1))
        cases.append(("发行日期漂移要报", len(mutated(released="2026-02-03")) == 1))
        cases.append(("下载地址换 owner 要报", len(mutated(gitee=entry["github"])) == 1))
        cases.append(("形态写错要报", len(mutated(kind="editor")) == 1))
        rm.write_text(readme_of(""), encoding="utf-8")
        cases.append(("表里没有行 = 失明要报", len(check_release_table(rm, cat)) == 1))
        # 表里放着另一枚（beta）而清单只有 alpha ⇒ 两笔账都要报：多一行 + 缺一行。
        other = dict(entry, id="beta")
        rm.write_text(readme_of(row_of(other)), encoding="utf-8")
        cases.append(("多行与缺行一起报", len(check_release_table(rm, cat)) == 2))
        rm.write_text(readme_of(row_of(entry)), encoding="utf-8")
        empty_cat = tmp / "empty.json"
        empty_cat.write_text("[]", encoding="utf-8")
        cases.append(("清单为空也要报（不当通过）", len(check_release_table(rm, empty_cat)) == 1))

    cases.append(("本仓现状要扫得到货", len(tracked_docs()) >= 5))
    live = check_release_table(ROOT / "README.md", ROOT / "plugins.json")
    cases.append(("本仓 README 表现状无违规", live == []))
    cases.append(("本仓 README 表收得到行", len(release_rows(ROOT / "README.md")) >= 1))

    # 第 5 查：每枚插件的 HISTORY.md。夹具 = 齐全的一份，再逐格破坏。
    def plugin_fixture(tmp: Path, history: str | None, version: str, released: str) -> Path:
        root = tmp / "plugins"
        one = root / "foo"
        one.mkdir(parents=True, exist_ok=True)
        (one / "manifest.json").write_text(
            json.dumps({"schema": 1, "id": "foo", "version": version, "released": released}),
            encoding="utf-8",
        )
        if history is not None:
            (one / "HISTORY.md").write_text(history, encoding="utf-8")
        return root

    head = "## 1.2.0 — 2026-01-02 — protocol 1\n\n- **新增**：第一版。\n"
    with tempfile.TemporaryDirectory() as td2:
        root = plugin_fixture(Path(td2), head, "1.2.0", "2026-01-02")
        cases.append(("HISTORY 齐全不误报", check_plugin_history(root) == []))
    with tempfile.TemporaryDirectory() as td3:
        root = plugin_fixture(Path(td3), None, "1.2.0", "2026-01-02")
        cases.append(("缺 HISTORY 要报", len(check_plugin_history(root)) == 1))
    with tempfile.TemporaryDirectory() as td4:
        root = plugin_fixture(Path(td4), "## 1.1.0 — 2026-01-02 — protocol 1\n", "1.2.0", "2026-01-02")
        cases.append(("首条版本不等于 manifest 要报", len(check_plugin_history(root)) == 1))
    with tempfile.TemporaryDirectory() as td5:
        root = plugin_fixture(Path(td5), "## 1.2.0 — 2026-01-09 — protocol 1\n", "1.2.0", "2026-01-02")
        cases.append(("条目日期不等于 released 要报", len(check_plugin_history(root)) == 1))
    with tempfile.TemporaryDirectory() as td6:
        root = plugin_fixture(
            Path(td6), f"## 1.2.0 — 2026-01-02\n\n- **状态**：待上传资产。\n", "1.2.0", "2026-01-02"
        )
        cases.append(("条目里写发布进度要报", len(check_plugin_history(root)) == 1))
    with tempfile.TemporaryDirectory() as td7:
        empty = Path(td7) / "plugins"
        empty.mkdir()
        cases.append(("扫不到插件 = 失明要报", len(check_plugin_history(empty)) == 1))
    live_history = check_plugin_history(ROOT / "plugins")
    cases.append(("本仓 HISTORY 现状无违规", live_history == []))

    failed = [n for n, ok in cases if not ok]
    for n, ok in cases:
        print(f"  {'ok  ' if ok else 'FAIL'} {n}")
    print(f"check_docs selftest: {'ok' if not failed else 'FAILED ' + str(failed)}")
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if "--selftest" in argv:
        return selftest()
    docs = tracked_docs()
    if not docs:
        print("docs: 一个 .md 都没找到（本查失明，不是通过）")
        return 1
    violations = run(docs)
    for v in violations:
        print(f" [x] {v}")
    print(f"docs: {len(docs)} files, violations: {len(violations)}")
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
