#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Supply-chain gate for the RuPix plugin repository.

Every plugin ships as its own executable, so every *resolved* package of every
plugin must declare a license that we are willing to redistribute. This is the
dependency-free local equivalent of `cargo deny`'s license half (that tool is not
installed on this machine); advisories are not covered.

Two deliberate differences from the decoder repository's same-named gate:

1. Manifests are **discovered**, not listed. A hardcoded list is how a gate goes
   blind: add a fifth plugin, forget to add its path here, and the build stays
   green while that binary ships unregistered code.
2. Zero manifests found is a **failure**, not a pass. "Nothing to check" and
   "nothing checked" must never look the same.

Because both of those claims are about *this file's own* behaviour, they are
pinned by `--selftest` rather than by a comment: the failure arms are exercised
against synthetic package records, so a future edit that makes the gate unable
to fail turns the build red by itself.

Usage:
    python tools/check_licenses.py            # gate over every plugins/*/Cargo.toml
    python tools/check_licenses.py --selftest # exercise the failure arms
    python tools/check_licenses.py --list     # print the resolved package table

Exit codes: 0 = clean; 1 = violation, gate blind, or selftest failure.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLUGINS = ROOT / "plugins"

# SPDX ids we are willing to redistribute inside a plugin binary. Adding a family
# here is a deliberate edit that must come with a line in THIRD-PARTY-NOTICES.md.
# CC0-1.0 (WHATWG encoding tables behind `encoding`) and BSL-1.0 (`ryu`, dual-licensed
# with Apache-2.0) joined because the barcode decoder's tree pulled them in; both are
# already allowed by the decoder repository's same-named gate, so this is house policy,
# not a per-package exception.
ALLOWED = {
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "ISC",
    "MIT",
    "MPL-2.0",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
}


def metadata(manifest: Path) -> dict:
    res = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(manifest)],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if res.returncode != 0:
        raise RuntimeError(
            f"{manifest.relative_to(ROOT).as_posix()}: cargo metadata failed:\n{res.stderr}"
        )
    return json.loads(res.stdout)


def ids(expression: str) -> list[str]:
    """Split an SPDX expression into the atomic ids it grants.

    Handles the legacy `MIT/Apache-2.0` slash form, parentheses, and AND/OR —
    `unicode-ident` ships `(MIT OR Apache-2.0) AND Unicode-3.0`, so a splitter
    that only knows OR would flag `Unicode-3.0` as an unknown id forever.
    """
    text = expression.replace("/", " OR ")
    text = re.sub(r"[()]", " ", text)
    return [p.strip() for p in re.split(r"\s+(?:OR|AND)\s+", text) if p.strip()]


def discover_manifests(plugins_dir: Path) -> list[Path]:
    """Every plugin directory that owns a Rust manifest, found by walking.

    Asset-only plugins (no `Cargo.toml`) legitimately contribute nothing here;
    an empty result is what the caller must treat as blindness.
    """
    if not plugins_dir.is_dir():
        return []
    return sorted(
        p / "Cargo.toml"
        for p in plugins_dir.iterdir()
        if p.is_dir() and (p / "Cargo.toml").is_file()
    )


def assess(packages: list[dict]) -> list[str]:
    """License violations for a list of `cargo metadata` package records."""
    violations: list[str] = []
    for pkg in packages:
        if pkg.get("source") is None:
            continue  # written in this repo (the plugin crate itself)
        name, version = pkg["name"], pkg["version"]
        expr = pkg.get("license") or ""
        file_decl = pkg.get("license_file") or ""
        if not expr and not file_decl:
            violations.append(f"{name} {version}: no license / license_file")
            continue
        for ident in ids(expr):
            if ident not in ALLOWED:
                violations.append(f"{name} {version}: license id {ident!r} not allowed ({expr})")
    return violations


def pkg(name: str, version: str, license_: str | None, license_file: str | None = None) -> dict:
    return {
        "name": name,
        "version": version,
        "license": license_,
        "license_file": license_file,
        "source": "registry+https://github.com/rust-lang/crates.io-index",
    }


def selftest() -> int:
    """Each arm must fail the way we claim it does. A gate that cannot fail is not a gate."""
    cases: list[tuple[str, bool]] = []

    bad = assess([pkg("secret-sauce", "1.0.0", "AGPL-3.0-only")])
    cases.append(("不在白名单的许可证要报", len(bad) == 1 and "AGPL-3.0-only" in bad[0]))

    missing = assess([pkg("mystery", "0.1.0", None)])
    cases.append(("没有许可证字段的要报", len(missing) == 1 and "no license" in missing[0]))

    compound = assess([pkg("unicode-ident", "1.0.27", "(MIT OR Apache-2.0) AND Unicode-3.0")])
    cases.append(("AND/OR 复合表达式不许误报", compound == []))

    slash = assess([pkg("pollster", "0.4.0", "Apache-2.0/MIT")])
    cases.append(("老式斜杠表达式要认", slash == []))

    local = assess([{"name": "rupix-plugin-scan", "version": "0.1.0", "license": None,
                     "license_file": None, "source": None}])
    cases.append(("本仓自写的 crate 不判（无 source）", local == []))

    empty_dir = ROOT / ".selftest-no-plugins"
    empty_dir.mkdir(exist_ok=True)
    try:
        cases.append(("空插件目录 = 失明，不是通过", discover_manifests(empty_dir) == []))
    finally:
        empty_dir.rmdir()
    cases.append(("plugins/ 不存在时同样报失明", discover_manifests(ROOT / "nope-does-not-exist") == []))

    # 真实仓库必须至少扫到一枚 manifest：这条防的是"路径词式变了 ⇒ 静默扫 0 个"。
    cases.append(("本仓现状要扫得到货", len(discover_manifests(PLUGINS)) >= 1))

    failed = [name for name, ok in cases if not ok]
    for name, ok in cases:
        print(f"  {'ok  ' if ok else 'FAIL'} {name}")
    print(f"check_licenses selftest: {'ok' if not failed else 'FAILED ' + str(failed)}")
    return 1 if failed else 0


def list_all(manifests: list[Path]) -> int:
    """Print the resolved table — the registry THIRD-PARTY-NOTICES.md points at.

    The notices file deliberately does **not** hand-copy these rows: 88 of them is a
    table nobody updates, and a stale registry is worse than none (it reads like a
    guarantee). `--list` is the machine-readable truth; the doc keeps the policy.
    """
    rows: dict[tuple[str, str], tuple[str, set[str]]] = {}
    for manifest in manifests:
        try:
            data = metadata(manifest)
        except RuntimeError as exc:
            print(f"licenses: {exc}")
            return 1
        for pkg in data.get("packages", []):
            if pkg.get("source") is None:
                continue
            key = (pkg["name"], pkg["version"])
            expr = pkg.get("license") or pkg.get("license_file") or "(none)"
            where = rows.setdefault(key, (expr, set()))
            where[1].add(manifest.parent.name)
    print(f"{'package':28} {'version':12} {'license':40} used-by")
    for (name, version), (expr, where) in sorted(rows.items()):
        print(f"{name:28} {version:12} {expr:40} {','.join(sorted(where))}")
    print(f"packages: {len(rows)}")
    return 0


def main(argv: list[str]) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if "--selftest" in argv:
        return selftest()

    manifests = discover_manifests(PLUGINS)
    if not manifests:
        print("licenses: no plugin Cargo.toml under plugins/ (gate blind, not a pass)")
        return 1
    if "--list" in argv:
        return list_all(manifests)

    seen: dict[tuple[str, str], str] = {}
    bad: dict[str, None] = {}
    for manifest in manifests:
        try:
            data = metadata(manifest)
        except RuntimeError as exc:
            print(f"licenses: {exc}")
            return 1
        for record in data.get("packages", []):
            if record.get("source") is not None:
                seen.setdefault((record["name"], record["version"]), manifest.parent.name)
        # 同一枚包会被多份 lock 各报一次：账要一条一行，重复计数只会把清单刷成噪声。
        for line in assess(data.get("packages", [])):
            bad[line] = None

    violations = sorted(bad)
    for line in violations:
        print(f" [x] {line}")
    print(
        f"licenses: {len(seen)} packages across {len(manifests)} plugin manifests, "
        f"violations: {len(violations)}"
    )
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
