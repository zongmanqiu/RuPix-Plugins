"""本仓库的「无密钥 / 无隐私」门禁：`violations: 0` 才算过。

判据与解码器仓 `tools/check_secrets.py` **同源**（两仓各自自包含，各自扫自己的树）。
本仓是**公开**源码仓：一旦令牌、口令、私人邮箱、含真名的本机路径混进来，删文件不算解决
——它已经进了 push 历史。所以这条门禁的全部价值在**push 之前**拦住。

判据（只有这六条，宁可少报也不误报到没人看）：
  1. 私钥块            `-----BEGIN ... PRIVATE KEY-----`
  2. 令牌前缀          ghp_ / github_pat_ / glpat- / xox?- / AKIA… / sk-…（带够长的实体才算）
  3. URL 内嵌凭据      `scheme://user:pass@host`
  4. 口令字面量        键名像凭据、值又是**带引号的字面量**；变量赋值不算
  5. 邮箱              除署名/许可证文件外一律报
  6. 本机用户目录      `C:\\Users\\<名字>`（占位符豁免）

豁免：`.git/`、构建中间物、二进制（exe/dll/pdb/图片）、`Cargo.lock`（只有哈希），
以及**本文件自身**——判据的正则原样写在这里，不跳就等于永远自报违规。代价如实说：
往这个文件里塞密钥扫不出来，而它是最显眼的一个文件，评审看它比看门禁靠谱。

用法（从仓库根）：
    python tools/check_secrets.py            # 0 = 干净；1 = 有违规（逐条打印 文件:行）
    python tools/check_secrets.py --list     # 只列被扫的文件数，不判
    python tools/check_secrets.py --selftest # 六条判据各自抓得到假样本 + 扫 0 判失败
"""
from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

SKIP_DIRS = {".git", "target", "dist", "__pycache__", ".cargo", "node_modules"}
SKIP_SUFFIX = {".dll", ".pdb", ".exe", ".lib", ".obj", ".dylib", ".so", ".a",
               ".zip", ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".tif", ".tiff",
               ".webp", ".ico", ".psd", ".hdr", ".exr"}
SKIP_NAMES = {"Cargo.lock"}
SELF = pathlib.Path(__file__).resolve()
# 允许出现邮箱的文件（三方署名 / 许可证正文）。加进来的每一条都要有理由。
EMAIL_OK_FILES = {"THIRD-PARTY-NOTICES.md", "LICENSE", "LICENSE.md", "COPYING"}

TEXT_SUFFIX = {".rs", ".toml", ".md", ".json", ".ps1", ".bat", ".py", ".c", ".h",
               ".txt", ".yml", ".yaml", ".gitignore", ".gitattributes", ".editorconfig", ""}

PRIVATE_KEY = re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")
# 前缀必须带够长的实体才算：文档里只写 `glpat-` 这个前缀不该被判成令牌（真实令牌 ≥20 位）。
TOKEN_PREFIX = re.compile(
    r"\b(?:ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|ghu_[A-Za-z0-9]{20,}"
    r"|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{16,}|xox[baprs]-[A-Za-z0-9-]{10,}"
    r"|AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{20,})")
URL_CREDS = re.compile(r"\b[a-z][a-z0-9+.-]*://[^\s/@:]+:[^\s/@]+@")
CRED_ASSIGN = re.compile(
    r"(?i)\b(?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key)\b"
    r"\s*[:=]\s*[\"'][^\"']{8,}[\"']")
EMAIL = re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
USER_PATH = re.compile(r"[A-Za-z]:\\+Users\\+([A-Za-z0-9._-]+)")
USER_PATH_OK = {"Someone", "someone", "user", "User", "username", "USERNAME", "Public", "Default"}


def iter_files(root: pathlib.Path = ROOT, self_path: pathlib.Path = SELF):
    for p in sorted(root.rglob("*")):
        if not p.is_file():
            continue
        if p.resolve() == self_path.resolve():
            continue
        if any(part in SKIP_DIRS for part in p.parts):
            continue
        if p.name in SKIP_NAMES:
            continue
        if p.suffix.lower() in SKIP_SUFFIX:
            continue
        if p.suffix.lower() not in TEXT_SUFFIX and p.name not in (".gitignore", ".gitattributes"):
            continue
        yield p


def scan_file(p: pathlib.Path, root: pathlib.Path) -> list[tuple[str, int, str]]:
    rel = p.relative_to(root).as_posix() if root in p.parents or root == p.parent else p.name
    bad: list[tuple[str, int, str]] = []
    try:
        text = p.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        return [(rel, 0, f"读不到：{e}")]
    for i, line in enumerate(text.splitlines(), 1):
        if PRIVATE_KEY.search(line):
            bad.append((rel, i, "私钥块"))
        if TOKEN_PREFIX.search(line):
            bad.append((rel, i, "访问令牌"))
        if URL_CREDS.search(line):
            bad.append((rel, i, "URL 内嵌凭据"))
        if CRED_ASSIGN.search(line):
            bad.append((rel, i, "口令字面量"))
        if p.name not in EMAIL_OK_FILES:
            m = EMAIL.search(line)
            if m:
                bad.append((rel, i, f"邮箱：{m.group(0)}"))
        m = USER_PATH.search(line)
        if m and m.group(1) not in USER_PATH_OK:
            bad.append((rel, i, f"含真名的本机路径：{m.group(0)}"))
    return bad


def scan(root: pathlib.Path = ROOT, self_path: pathlib.Path = SELF):
    bad: list[tuple[str, int, str]] = []
    scanned = 0
    for p in iter_files(root, self_path):
        scanned += 1
        bad.extend(scan_file(p, root))
    return scanned, bad


def selftest() -> int:
    """六条判据各自抓得到；四组"不该报"的样本不误报；扫 0 个判失败（不算通过）。"""
    import tempfile

    positive = {
        "私钥块": "-----BEGIN RSA PRIVATE KEY-----\n",
        "访问令牌": "token = ghp_" + "A" * 36 + "\n",
        "URL 内嵌凭据": "remote = https://alice:hunter2@host/x\n",
        "口令字面量": 'password = "hunter2x"\n',
        "邮箱": "contact me at qzm@example.com\n",
        "含真名的本机路径": "copy from C:\\Users\\qiuzongman\\Desktop\n",
    }
    # 这些**不该**报：只写前缀的文档、变量赋值、占位用户名——判据窄是刻意的，
    # 一条天天误报的门禁最后一定被人加白名单加到失效。
    negative = {
        "只写前缀不算令牌": "形如 glpat- / ghp_ 的长串都不许进仓\n",
        "变量赋值不算口令": "password: self.config.password\n",
        "占位用户名不算泄漏": "C:\\Users\\<名字>\\ 这种写法只是占位\n",
        "文档里的示例路径": "落点是 <exe>/plugins/<id>/settings.json\n",
    }

    cases: list[tuple[str, bool]] = []
    with tempfile.TemporaryDirectory() as td:
        tmp = pathlib.Path(td)
        probe = tmp / "probe.txt"
        for want, body in positive.items():
            probe.write_text(body, encoding="utf-8")
            hits = scan_file(probe, tmp)
            cases.append((f"{want}要报", len(hits) == 1 and hits[0][2].startswith(want)))
        for want, body in negative.items():
            probe.write_text(body, encoding="utf-8")
            cases.append((f"{want}不误报", scan_file(probe, tmp) == []))
        signed = tmp / "LICENSE"
        signed.write_text("Upstream author: someone <someone@example.com>\n", encoding="utf-8")
        cases.append(("署名文件的邮箱豁免", scan_file(signed, tmp) == []))
        blind = tmp / "empty"
        blind.mkdir()
        n, _ = scan(blind, blind / "absent.py")
        cases.append(("扫 0 个 = 失明，不许当通过", n == 0))

    live_scanned, live_bad = scan()
    cases.append(("本仓现状要扫得到货", live_scanned >= 20))
    cases.append(("本仓现状零违规", live_bad == []))

    failed = [n for n, ok in cases if not ok]
    for n, ok in cases:
        print(f"  {'ok  ' if ok else 'FAIL'} {n}")
    print(f"check_secrets selftest: {'ok' if not failed else 'FAILED ' + str(failed)}")
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if "--selftest" in argv:
        return selftest()
    scanned, bad = scan()
    if "--list" in argv:
        print(f"secrets: files scanned {scanned}")
        return 0
    if scanned == 0:
        print("secrets: 一个文件都没扫到（本查失明，不是通过）")
        return 1
    for rel, i, why in bad:
        print(f" [x] {rel}:{i} -- {why}")
    print(f"secrets: files scanned {scanned}, violations: {len(bad)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
