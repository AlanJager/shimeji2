#!/usr/bin/env python3
"""Copy the editor into cloud-functions in the form the Python builder packages.

The builder copies .py and .json only. PNG and XML are stored in blobs.py and
unpacked when the function handles a request.
"""

import base64
import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EDITOR = ROOT / "cloud-functions" / "api" / "editor"
MODULES = ("server.py", "media.py", "parts.py", "catalog.py", "dance.py")
PACK = ROOT / "assets" / "packs" / "shime"


def main():
    if EDITOR.exists():
        shutil.rmtree(EDITOR)
    EDITOR.mkdir(parents=True)
    for name in MODULES:
        shutil.copyfile(ROOT / "tools" / "editor" / name, EDITOR / name)
    catalog = EDITOR / "assets" / "catalog"
    catalog.mkdir(parents=True)
    shutil.copyfile(ROOT / "assets" / "catalog" / "sources.json", catalog / "sources.json")
    blobs = {}
    for path in sorted(PACK.rglob("*")):
        if path.is_file():
            rel = path.relative_to(ROOT).as_posix()
            blobs[rel] = base64.b64encode(path.read_bytes()).decode("ascii")
    lines = ["BLOBS = {"]
    for rel, encoded in blobs.items():
        lines.append(f"    {json.dumps(rel, ensure_ascii=False)}: {json.dumps(encoded)},")
    lines.append("}")
    lines.append("")
    (EDITOR / "blobs.py").write_text("\n".join(lines), encoding="utf-8")
    if "assets/packs/shime/conf/動作.xml" not in blobs:
        raise SystemExit("缺少 動作.xml")
    if "assets/packs/shime/img/shime1.png" not in blobs:
        raise SystemExit("缺少 shime1.png")


if __name__ == "__main__":
    main()
