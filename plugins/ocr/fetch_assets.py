#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Stage the offline OCR engine + models for plugins/ocr.

No binary lives in the source tree. Assets are cached in
<workspace>/.temp/ocr-assets/ and copied into
<workspace>/.temp/plugin-dist/ocr/engine/. Every shipped file is pinned by
SHA256: a missing file, a failed download or a hash mismatch is a hard error,
so a bad asset can never be packed silently.

Only ONE recognition model is shipped. The ch dictionary contains 6270 CJK
unified ideographs, so Simplified Chinese, Traditional Chinese, English and
common symbols are recognised in the same pass; a second language-specific
rec model would force a user-facing switch and degrade mixed-script images.
"""
from __future__ import annotations

import hashlib
import shutil
import sys
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # RuPix-Plugin/
WORKSPACE = ROOT.parent
CACHE = WORKSPACE / ".temp" / "ocr-assets"
DEST = WORKSPACE / ".temp" / "plugin-dist" / "ocr" / "engine"

PACK_NAME = "OCR_0.3.0.zip"
PACK_URL = (
    "https://gitee.com/qiuzongman/CurSeen-Scripts/releases/download/"
    "OCR_0.3.0/OCR_0.3.0.zip"
)
PACK_SHA = "17674a462fe162ed2363444359678600cc06c761539a85cf316fdee9688dd4aa"

# Files taken from the released engine archive, member prefix OCR/ocr_tool/.
PACK_MEMBERS = [
    "CurSeenOcr.exe",
    "ch_PP-OCRv4_det_mobile.onnx",
    "ch_ppocr_mobile_v2.0_cls_mobile.onnx",
    "ch_PP-OCRv4_rec_mobile.onnx",
    "ppocr_keys_v1.txt",
]

# name -> SHA256. Model hashes are the upstream RapidOCR values; the engine
# binary is pinned from the bytes we verified locally. Never edit a hash
# without re-verifying the source.
PINNED = {
    "CurSeenOcr.exe": "93a647539174950bb33f17c2002102fcc22116c1c0f9fe2616ce039c0567e5a2",
    "ch_PP-OCRv4_det_mobile.onnx": "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
    "ch_ppocr_mobile_v2.0_cls_mobile.onnx": "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
    "ch_PP-OCRv4_rec_mobile.onnx": "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
    "ppocr_keys_v1.txt": "28b2362ad4ab2dc38769aa72feb535e3a9ddb3fd2a7585a05920e6393b1dc7f7",
}


def fail(msg: str) -> int:
    print("[fail] " + msg)
    return 1


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def download(url: str, dest: Path) -> None:
    if dest.is_file():
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(url, headers={"User-Agent": "RuPix-Plugin-Build/1.0"})
    tmp = dest.with_suffix(dest.suffix + ".part")
    with urllib.request.urlopen(req, timeout=300) as r, tmp.open("wb") as f:
        shutil.copyfileobj(r, f)
    tmp.replace(dest)
    print("[get] " + dest.name)


def clean_dest() -> None:
    """dist/engine is packed wholesale by rglob: anything not pinned must go.

    This also removes the CLI's `*-result.jpg/.txt` leftovers and the two
    Traditional-Chinese assets staged by an earlier build, so shrinking the
    model set cannot silently leave megabytes inside the ZIP.
    """
    for p in DEST.iterdir():
        if p.is_file() and p.name not in PINNED:
            p.unlink()
            print("[clean] removed stale " + p.name)


def main() -> int:
    DEST.mkdir(parents=True, exist_ok=True)
    CACHE.mkdir(parents=True, exist_ok=True)
    clean_dest()

    def staged_ok() -> bool:
        return all(
            (DEST / name).is_file() and sha256(DEST / name) == digest
            for name, digest in PINNED.items()
        )

    if staged_ok():
        print("[ok] engine assets already staged and verified: " + str(DEST))
        return 0

    # A stale/corrupt cache entry must be re-fetched, not silently reused.
    for name, digest in PINNED.items():
        cached = CACHE / name
        if cached.is_file() and sha256(cached) != digest:
            cached.unlink()

    pack = CACHE / PACK_NAME
    if pack.is_file() and sha256(pack) != PACK_SHA:
        pack.unlink()
    try:
        download(PACK_URL, pack)
    except Exception as e:  # network / redirect / timeout
        return fail("engine archive download failed: " + str(e))
    if sha256(pack) != PACK_SHA:
        return fail("engine archive SHA256 mismatch: " + str(pack))

    try:
        with zipfile.ZipFile(pack) as z:
            for name in PACK_MEMBERS:
                target = CACHE / name
                if not target.is_file():
                    target.write_bytes(z.read("OCR/ocr_tool/" + name))
    except Exception as e:
        return fail("engine archive is unusable: " + str(e))

    for name, digest in PINNED.items():
        src = CACHE / name
        if not src.is_file():
            return fail("missing asset: " + name)
        if sha256(src) != digest:
            return fail("SHA256 mismatch: " + name)
        shutil.copy2(src, DEST / name)

    if not staged_ok():
        return fail("staged assets did not verify")
    total = sum((DEST / n).stat().st_size for n in PINNED)
    print("[ok] staged %d assets, %.2f MiB -> %s" % (len(PINNED), total / 1048576, DEST))
    return 0


if __name__ == "__main__":
    sys.exit(main())
