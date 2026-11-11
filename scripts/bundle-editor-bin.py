#!/usr/bin/env python3
"""Pack shimeji-editor with the page, bundled shime, and catalog. No Python inside."""

import argparse
import hashlib
import stat
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EDITOR_SKIP = {".py"}
EDITOR_SKIP_NAMES = {
    "package.json",
    "package-lock.json",
    "build-vrm-vendor.mjs",
}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def editor_files():
    root = ROOT / "tools/editor"
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        if path.suffix in EDITOR_SKIP or path.name in EDITOR_SKIP_NAMES:
            continue
        if path.name.endswith(".test.mjs"):
            continue
        yield path, Path("editor") / path.relative_to(root)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--platform", choices=("windows", "macos"), required=True)
    parser.add_argument("--out", type=Path, default=ROOT / "target/editor-bundle")
    args = parser.parse_args()
    executable = args.exe.resolve()
    data = executable.read_bytes()
    if args.platform == "windows":
        if data[:2] != b"MZ":
            raise ValueError("Expected a Windows executable")
        binary_name = "shimeji-editor.exe"
        archive_name = "shimeji2-editor-windows-x64.zip"
        launcher_name = "启动编辑器.bat"
        launcher = "@echo off\r\ncd /d \"%~dp0\"\r\nshimeji-editor.exe --open\r\n"
    else:
        if data[:4] != b"\xcf\xfa\xed\xfe" and data[:4] != b"\xfe\xed\xfa\xcf":
            raise ValueError("Expected a Mach-O executable")
        binary_name = "shimeji-editor"
        archive_name = "shimeji2-editor-macos.zip"
        launcher_name = "启动编辑器.command"
        launcher = "#!/bin/sh\ncd \"$(dirname \"$0\")\"\nexec ./shimeji-editor --open\n"
    pack = ROOT / "assets/packs/shime"
    checksums = (pack / "CHECKSUMS.sha256").read_text(encoding="utf-8").splitlines()
    if len(checksums) != 48:
        raise ValueError("Unexpected bundled pack inventory")
    for line in checksums:
        digest, name = line.split("  ", 1)
        if sha256(pack / name) != digest:
            raise ValueError(f"Bundled source changed: {name}")
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=True)
    folder_name = archive_name.removesuffix(".zip")
    staging = tempfile.TemporaryDirectory(prefix=".editor-stage-", dir=output)
    folder = Path(staging.name) / folder_name
    folder.mkdir()
    (folder / binary_name).write_bytes(data)
    (folder / binary_name).chmod(0o755)
    (folder / launcher_name).write_bytes(launcher.encode("utf-8"))
    if launcher_name.endswith(".command"):
        (folder / launcher_name).chmod(0o755)
    notice = (
        "这是不需要安装 Python 的动作编辑器。\n"
        "双击「启动编辑器」，浏览器会打开 http://127.0.0.1:4317 。\n"
        "关闭黑色窗口即退出。工程保存在浏览器里，请自行另存 JSON。\n"
        "画角色请用 Aseprite 或其他像素工具，再把图导入这里编排动作。\n"
    )
    (folder / "先读我.txt").write_bytes(notice.encode("utf-8"))
    for source, relative in editor_files():
        target = folder / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(source.read_bytes())
    for line in checksums:
        _, relative = line.split("  ", 1)
        target = folder / "assets/packs/shime" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((pack / relative).read_bytes())
    catalog = ROOT / "assets/catalog/sources.json"
    (folder / "assets/catalog").mkdir(parents=True)
    (folder / "assets/catalog/sources.json").write_bytes(catalog.read_bytes())
    archive = Path(staging.name) / archive_name
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zipped:
        for path in sorted(folder.rglob("*")):
            if not path.is_file():
                continue
            info = zipfile.ZipInfo(path.relative_to(folder.parent).as_posix())
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = path.stat().st_mode
            info.external_attr = (mode & 0xFFFF) << 16
            if path.name == binary_name or path.suffix == ".command":
                info.external_attr = (stat.S_IFREG | 0o755) << 16
            zipped.writestr(info, path.read_bytes())
    with zipfile.ZipFile(archive) as zipped:
        if zipped.testzip() is not None:
            raise ValueError("Editor archive failed its own integrity check")
    final_archive = output / archive.name
    archive.replace(final_archive)
    digest = sha256(final_archive)
    (output / f"{archive_name}.sha256").write_text(
        f"{digest}  {archive_name}\n", encoding="utf-8"
    )
    staging.cleanup()
    print(f"{digest}  {final_archive}")


if __name__ == "__main__":
    main()
