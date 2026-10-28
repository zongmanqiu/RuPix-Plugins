#!/usr/bin/env python3
"""Package built plugins into <workspace>/.build/docs and write plugins.json.

Input:  plugins/<id>/manifest.json + <workspace>/.temp/plugin-dist/<id>/
Output: <workspace>/.build/docs/<id>_<version>.zip
        <workspace>/.build/docs/plugins.json
        <repo>/plugins.json (same catalog, source copy)
"""
from __future__ import annotations

import hashlib
import json
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKSPACE = ROOT.parent
PLUGINS = ROOT / "plugins"
DIST = WORKSPACE / ".temp" / "plugin-dist"
DOCS = WORKSPACE / ".build" / "docs"
GITEE = "https://gitee.com/qiuzongman/RuPix-Plugins/releases/download"
GITHUB = "https://github.com/zongmanqiu/RuPix-Plugins/releases/download"

# 随包进的"人读文件"：插件目录里放一份，用户装完能在 plugins/<id>/ 里直接看到。
# 宿主**不解析**它们（自述只有 manifest.json 一份），这里只为把文件带进去。
DOC_FILES = ("README.md", "LICENSE", "LICENSE.md", "LICENSE.txt", "NOTICE")


def fail(msg: str) -> int:
    print(f" [x] {msg}")
    return 1


def main() -> int:
    DOCS.mkdir(parents=True, exist_ok=True)
    entries = []
    for src in sorted(p for p in PLUGINS.iterdir() if p.is_dir()):
        manifest = src / "manifest.json"
        if not manifest.is_file():
            continue
        try:
            meta = json.loads(manifest.read_text(encoding="utf-8-sig"))
        except Exception as e:
            return fail(f"{manifest}: {e}")
        pid = meta.get("id")
        ver = meta.get("version")
        payload = DIST / str(pid)
        if not payload.is_dir():
            print(f"skip {pid}: no built payload at {payload}")
            continue
        if not (payload / "manifest.json").is_file():
            return fail(f"{payload}: missing manifest.json")

        file_name = f"{pid}_{ver}.zip"
        out_zip = DOCS / file_name
        with zipfile.ZipFile(out_zip, "w", zipfile.ZIP_DEFLATED) as z:
            for p in sorted(payload.rglob("*")):
                if p.is_file():
                    z.write(p, p.relative_to(payload).as_posix())
            for doc in DOC_FILES:
                f = src / doc
                if f.is_file() and not (payload / doc).is_file():
                    z.write(f, doc)
        data = out_zip.read_bytes()
        tag = f"{pid}_{ver}"
        entries.append(
            {
                "id": pid,
                "name": meta.get("name", pid),
                "version": ver,
                "protocol": meta.get("protocol", 1),
                "min_host": meta.get("min_host", ""),
                "kind": meta.get("kind", "command"),
                "description": meta.get("description", ""),
                "capabilities": meta.get("capabilities", []),
                "released": meta.get("released", ""),
                "author": meta.get("author", "RuPix"),
                "license": meta.get("license", "MIT"),
                "file": file_name,
                "size": len(data),
                "sha256": hashlib.sha256(data).hexdigest(),
                "gitee": f"{GITEE}/{tag}/{file_name}",
                "github": f"{GITHUB}/{tag}/{file_name}",
            }
        )
        print(f"packed {pid} -> {out_zip}")

    text = json.dumps(entries, ensure_ascii=False, indent=2) + "\n"
    (DOCS / "plugins.json").write_text(text, encoding="utf-8")
    (ROOT / "plugins.json").write_text(text, encoding="utf-8")
    old_index = ROOT / "index.json"
    if old_index.is_file():
        old_index.unlink()
    print(f"plugins.json: {len(entries)} entries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
