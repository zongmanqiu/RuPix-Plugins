#!/usr/bin/env python3
"""Validate RuPix plugin manifests under plugins/*/manifest.json.

Exit codes:
  0 = all valid (or no plugins)
  1 = at least one violation
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLUGINS = ROOT / "plugins"

ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{2,63}$")
VER_RE = re.compile(r"^[0-9]+(?:\.[0-9]+)*$")
CAPS = {
    "image.read",
    "image.write",
    "selection.read",
    "clipboard.write",
    "file.read",
    "file.write",
    "network",
    "ui.window",
    "ui.panel",
    "settings.read",
    "settings.write",
    "process.spawn",
    "open.url",
    "open.file",
}
KINDS = {"command", "editor", "panel"}


def fail(errors: list[str], path: Path, msg: str) -> None:
    errors.append(f"{path.relative_to(ROOT).as_posix()}: {msg}")


def check_manifest(path: Path, dir_name: str, seen: set[str], errors: list[str]) -> None:
    try:
        data = json.loads(path.read_text(encoding="utf-8-sig"))
    except Exception as e:
        fail(errors, path, f"JSON parse failed: {e}")
        return
    if not isinstance(data, dict):
        fail(errors, path, "manifest must be a JSON object")
        return

    def req_str(key: str) -> str | None:
        v = data.get(key)
        if not isinstance(v, str) or not v:
            fail(errors, path, f"missing/non-string field {key}")
            return None
        return v

    if data.get("schema") != 1:
        fail(errors, path, "schema must be 1")
    # 宿主支持的协议区间（与宿主 core/ext.rs 的 MIN..=MAX 对齐；只增不改，下界永不抬）。
    protocol_min, protocol_max = 1, 1
    protocol = data.get("protocol")
    if not isinstance(protocol, int) or not (protocol_min <= protocol <= protocol_max):
        fail(errors, path, f"protocol must be within {protocol_min}..={protocol_max}")

    pid = req_str("id")
    if pid:
        if not ID_RE.match(pid):
            fail(errors, path, f"invalid id {pid!r}")
        if pid != dir_name:
            fail(errors, path, f"id {pid!r} != directory {dir_name!r}")
        if pid in seen:
            fail(errors, path, f"duplicate id {pid!r}")
        seen.add(pid)

    req_str("name")
    for key in ("version", "min_host"):
        v = req_str(key)
        if v and not VER_RE.match(v):
            fail(errors, path, f"{key} must be dotted numeric version")

    kind = req_str("kind")
    if kind and kind not in KINDS:
        fail(errors, path, f"unknown kind {kind!r}")

    entry = req_str("entry")
    if entry:
        if re.match(r"^[A-Za-z]:", entry) or entry.startswith(("/", "\\")):
            fail(errors, path, "entry must be relative to plugin directory")
        parts = entry.replace("\\", "/").split("/")
        if ".." in parts:
            fail(errors, path, "entry must not contain ..")
        if kind in ("command", "editor") and not entry.lower().endswith(".exe"):
            fail(errors, path, "entry must be .exe for command/editor")

    args = data.get("args", [])
    if not isinstance(args, list) or any(not isinstance(a, str) for a in args):
        fail(errors, path, "args must be a string array")

    caps = data.get("capabilities", [])
    if not isinstance(caps, list) or any(not isinstance(c, str) for c in caps):
        fail(errors, path, "capabilities must be a string array")
    else:
        unknown = sorted(set(caps) - CAPS)
        if unknown:
            fail(errors, path, f"unknown capabilities: {', '.join(unknown)}")

    platforms = data.get("platforms")
    if not isinstance(platforms, list) or any(not isinstance(p, str) for p in platforms):
        fail(errors, path, "platforms must be a string array")
    elif "windows-x64" not in platforms:
        fail(errors, path, "platforms must contain windows-x64")

    # `settings_ui` = 插件自带设置窗口（宿主在插件卡上给一颗齿轮）。
    # 必须同时声明 `settings.write`：只有读权限的话用户改完存不下来，
    # 那比没有入口更糟——宿主解析期会拒，这里提前拦，省一次下载。
    if "settings_ui" in data:
        if not isinstance(data["settings_ui"], bool):
            fail(errors, path, "settings_ui must be a boolean")
        elif data["settings_ui"] and "settings.write" not in caps:
            fail(errors, path, "settings_ui=true requires capability settings.write")

    # `run_ui` = host renders the parameter dialog at RUN time (additive field).
    settings = data.get("settings", [])
    if "run_ui" in data:
        if not isinstance(data["run_ui"], bool):
            fail(errors, path, "run_ui must be a boolean")
        elif data["run_ui"] and not (isinstance(settings, list) and settings):
            fail(errors, path, "run_ui=true requires a non-empty settings array")
    if isinstance(settings, list):
        for i, item in enumerate(settings):
            if not isinstance(item, dict):
                continue
            lo, hi = item.get("min"), item.get("max")
            if (lo is None) != (hi is None):
                fail(errors, path, f"settings[{i}]: min/max must come in pairs")
            elif lo is not None:
                ints = all(
                    isinstance(v, int) and not isinstance(v, bool) for v in (lo, hi)
                )
                if not ints or lo >= hi:
                    fail(
                        errors,
                        path,
                        f"settings[{i}]: min/max must be integers with min < max",
                    )

    # `preview` = declarative live preview (additive). Its keys must point at
    # slider-backed int settings: the host reads 0..1 slider signals, not text.
    if "preview" in data:
        prev = data["preview"]
        if not isinstance(prev, dict):
            fail(errors, path, "preview must be an object")
        else:
            if prev.get("op") != "channel-linear":
                fail(errors, path, "preview.op must be channel-linear")
            keys = {}
            if isinstance(settings, list):
                keys = {
                    it["key"]: it
                    for it in settings
                    if isinstance(it, dict) and isinstance(it.get("key"), str)
                }
            for field in ("gain", "bias"):
                arr = prev.get(field)
                if not isinstance(arr, list) or len(arr) != 3 or not all(
                    isinstance(k, str) for k in arr
                ):
                    fail(errors, path, f"preview.{field} must be exactly 3 key names")
                    continue
                for k in arr:
                    it = keys.get(k)
                    if it is None:
                        fail(errors, path, f"preview.{field}: {k!r} is not in settings")
                    elif it.get("type") != "int" or "min" not in it or "max" not in it:
                        fail(errors, path, f"preview.{field}: {k!r} must be an int with min/max")

    # `presets` = declarative filter table (additive). The host writes the six
    # slider values when a preset is chosen, so numbers must land inside the
    # declared slider ranges and every name must exist in a choice setting.
    if "presets" in data:
        table = data["presets"]
        if not isinstance(table, dict):
            fail(errors, path, "presets must be an object")
        else:
            choice_values = None
            if isinstance(settings, list):
                for it in settings:
                    if isinstance(it, dict) and it.get("type") == "choice":
                        choice_values = it.get("choices")
                        break
            if not isinstance(choice_values, list):
                fail(errors, path, "presets requires a choice setting")
            else:
                if len(table) >= len(choice_values):
                    fail(
                        errors,
                        path,
                        "presets must leave one choice value as the custom sentinel",
                    )
            key_map = {}
            if isinstance(settings, list):
                key_map = {
                    it["key"]: it
                    for it in settings
                    if isinstance(it, dict) and isinstance(it.get("key"), str)
                }
            prev = data.get("preview")
            for name, body in table.items():
                if isinstance(choice_values, list) and name not in choice_values:
                    fail(errors, path, f"presets: {name!r} is not in choice.choices")
                if name == "custom":
                    fail(errors, path, "presets: 'custom' is the sentinel, no numbers")
                if not isinstance(body, dict):
                    fail(errors, path, f"presets.{name} must be an object")
                    continue
                for field in ("gain", "bias"):
                    arr = body.get(field)
                    if not isinstance(arr, list) or len(arr) != 3 or not all(
                        isinstance(v, int) and not isinstance(v, bool) for v in arr
                    ):
                        fail(errors, path, f"presets.{name}.{field} must be 3 integers")
                        continue
                    names = prev.get(field) if isinstance(prev, dict) else None
                    for i, v in enumerate(arr):
                        key = names[i] if isinstance(names, list) and len(names) == 3 else None
                        it = key_map.get(key) if isinstance(key, str) else None
                        if it is None:
                            continue  # preview gate already owns this failure
                        lo, hi = it.get("min"), it.get("max")
                        if isinstance(lo, int) and isinstance(hi, int) and not (lo <= v <= hi):
                            fail(
                                errors,
                                path,
                                f"presets.{name}.{field}[{i}]={v} outside {lo}..{hi}",
                            )

    for key in ("description", "author"):
        if key in data and not isinstance(data[key], str):
            fail(errors, path, f"{key} must be a string")


def main() -> int:
    errors: list[str] = []
    if not PLUGINS.is_dir():
        print("manifests: plugins/ missing")
        return 1

    seen: set[str] = set()
    count = 0
    for d in sorted(p for p in PLUGINS.iterdir() if p.is_dir()):
        manifest = d / "manifest.json"
        if not manifest.is_file():
            fail(errors, d, "missing manifest.json")
            continue
        count += 1
        check_manifest(manifest, d.name, seen, errors)

    if errors:
        for e in errors:
            print(f" [x] {e}")
        print(f"manifest violations: {len(errors)}")
        return 1
    print(f"manifests: {count} ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
