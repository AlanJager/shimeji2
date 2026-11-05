#!/usr/bin/env python3
"""Package a built x64 EXE with only the licensed bundled shime and notices."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=ROOT / "target/release/shimeji2.exe")
    parser.add_argument("--out", type=Path, default=ROOT / "target/windows-bundle")
    args = parser.parse_args()
    executable = args.exe.resolve()
    data = executable.read_bytes()
    if data[:2] != b"MZ" or len(data) < 64:
        raise ValueError("Expected a Windows PE executable")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0" or struct.unpack_from("<H", data, pe + 4)[0] != 0x8664:
        raise ValueError("Expected an x86-64 PE executable")
    pack = ROOT / "assets/packs/shime"
    checksums = (pack / "CHECKSUMS.sha256").read_text(encoding="utf-8").splitlines()
    assert len(checksums) == 48, "Unexpected bundled pack inventory"
    for line in checksums:
        digest, name = line.split("  ", 1)
        if sha256(pack / name) != digest:
            raise ValueError(f"Bundled source changed: {name}")
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=True)
    name = "shimeji2-0.1.0-windows-x64-preview"
    staging = tempfile.TemporaryDirectory(prefix=".windows-stage-", dir=output)
    folder = Path(staging.name) / name
    folder.mkdir()
    shutil.copy2(executable, folder / "shimeji2.exe")
    for source, destination in [
        ("LICENSE", "LICENSE"), ("NOTICE.md", "NOTICE.md"),
        ("packaging/windows/README.txt", "先读我-README.txt"),
        ("packaging/windows/THIRD-PARTY-NOTICES.md", "WINDOWS-THIRD-PARTY-NOTICES.md"),
        ("packaging/THIRD-PARTY-NOTICES.md", "THIRD-PARTY-NOTICES.md"),
    ]:
        shutil.copy2(ROOT / source, folder / destination)
    shutil.copytree(ROOT / "packaging/windows/licenses", folder / "licenses")
    for line in checksums:
        _, relative = line.split("  ", 1)
        target = folder / "packs/shime" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(pack / relative, target)
    for relative in ["CHECKSUMS.sha256", "LICENSE.txt", "ATTRIBUTION.md"]:
        shutil.copy2(pack / relative, folder / "packs/shime" / relative)
    for line in checksums:
        digest, relative = line.split("  ", 1)
        assert sha256(folder / "packs/shime" / relative) == digest
    files = sorted(path for path in folder.rglob("*") if path.is_file())
    manifest = {path.relative_to(folder).as_posix(): sha256(path) for path in files}
    (folder / "MANIFEST.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    archive = Path(staging.name) / f"{name}.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zipped:
        for path in sorted(folder.rglob("*")):
            if path.is_file():
                zipped.write(path, path.relative_to(folder.parent).as_posix())
    with zipfile.ZipFile(archive) as zipped:
        assert zipped.testzip() is None
        for relative, digest in manifest.items():
            assert hashlib.sha256(zipped.read(f"{name}/{relative}")).hexdigest() == digest
    checksum = sha256(archive)
    final_archive = output / archive.name
    archive.replace(final_archive)
    (output / "SHA256SUMS.txt").write_text(f"{checksum}  {archive.name}\n", encoding="utf-8")
    staging.cleanup()
    print(f"Windows preview: {final_archive}\nSHA-256: {checksum}")


if __name__ == "__main__":
    main()
