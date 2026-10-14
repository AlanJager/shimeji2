#!/usr/bin/env python3
"""Prepare reviewable archives from a local bundle; never upload or alter Git."""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "target/bundle"
DEST = ROOT / "target/release-candidate"


def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def public_sources():
    inventory = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT
    ).decode().split("\0")
    trees = ("crates/", "scripts/", "packaging/", "tests/", "integrations/", ".github/",
             "assets/packs/shime/", "tools/ee-reference/")
    singles = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "README.md", "LICENSE",
               "NOTICE.md", "AGENTS.md", ".gitignore", ".gitattributes", "corpus/README.md",
               "docs/release.md", "docs/testing/golden-traces.md",
               "docs/testing/public-beta-checklist.md"}
    paths = []
    for name in sorted(set(inventory)):
        if not (name in singles or name.startswith(trees) or name.startswith("docs/releases/")):
            continue
        path = ROOT / name
        if path.is_symlink():
            raise ValueError(f"Source snapshot refuses symlinks: {name}")
        if not path.is_file():
            continue
        data = path.read_bytes()
        # Detect known machine-private strings without embedding them in this script.
        if re.search(rb"/Users/(?!example/)[A-Za-z0-9_.-]+/", data) or re.search(rb"\bproxy\.[A-Za-z0-9.-]+:[0-9]+", data):
            raise ValueError(f"Machine-private content in public source: {name}")
        if re.search(rb"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----", data):
            raise ValueError(f"Private key in public source: {name}")
        paths.append((name, path))
    required = ["Cargo.toml", "Cargo.lock", "packaging/icons/shimeji2.png", "README.md", "LICENSE"]
    assert set(required) <= {name for name, _ in paths}, "Incomplete source snapshot"
    return paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", default="v0.1.0-beta.1")
    args = parser.parse_args()
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", args.tag):
        parser.error("tag must be a version such as v0.1.0-beta.1")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if args.tag.removeprefix("v").split("-")[0] != version:
        parser.error("tag core version must match Cargo.toml")
    app = BUNDLE / "shimeji2.app"
    with (app / "Contents/Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    assert info["CFBundleShortVersionString"] == version
    assert info["CFBundleIconFile"] == "shimeji2.icns"
    resources = app / "Contents/Resources"
    for name in ["LICENSE", "NOTICE.md", "THIRD-PARTY-NOTICES.md", "READ-ME-FIRST.md", "shimeji2.icns"]:
        assert (resources / name).is_file(), name
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    subprocess.run([str(ROOT / "scripts/check-entitlements.sh"), str(app)], check=True,
                   stdout=subprocess.DEVNULL)
    signature = subprocess.run(["codesign", "-dv", "--verbose=4", str(app)], check=True,
                               capture_output=True, text=True).stderr
    signing = "ad_hoc" if "Signature=adhoc" in signature else "developer_id_or_other"
    for binary in (app / "Contents/MacOS").iterdir():
        assert not re.search(rb"/Users/[A-Za-z0-9_.-]+/", binary.read_bytes()), "Private build path in app binary"
    architectures = run("lipo", "-archs", str(app / "Contents/MacOS/shimeji-macos")).split()
    label = "universal" if len(architectures) > 1 else architectures[0]
    assert set(architectures) <= {"arm64", "x86_64"}, architectures
    expected_pack = ROOT / "assets/packs/shime"
    actual_pack = resources / "packs/shime"
    assert {p.relative_to(actual_pack) for p in actual_pack.rglob("*") if p.is_file()} == {
        p.relative_to(expected_pack) for p in expected_pack.rglob("*") if p.is_file()
    }
    for path in expected_pack.rglob("*"):
        if path.is_file():
            assert path.read_bytes() == (actual_pack / path.relative_to(expected_pack)).read_bytes()
    sources = public_sources()
    DEST.mkdir(parents=True, exist_ok=True)
    stem = f"shimeji2-{args.tag.removeprefix('v')}"
    artifacts = []
    for extension in ["zip", "dmg"]:
        output = DEST / f"{stem}-macos-{label}.{extension}"
        shutil.copy2(BUNDLE / f"shimeji2.{extension}", output)
        artifacts.append(output)
    source_zip = DEST / f"{stem}-source.zip"
    with zipfile.ZipFile(source_zip, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, path in sources:
            archive.write(path, f"{stem}/{name}")
    artifacts.append(source_zip)
    for archive in [artifacts[0], source_zip]:
        with zipfile.ZipFile(archive, metadata_encoding="utf-8") as stream:
            assert stream.testzip() is None, archive
            assert not any(Path(name).is_absolute() or ".." in Path(name).parts for name in stream.namelist())
    shutil.copy2(ROOT / "packaging/READ-ME-FIRST.md", DEST / "READ-ME-FIRST.md")
    shutil.copy2(ROOT / "docs/releases" / f"{args.tag}.md", DEST / "RELEASE-NOTES.md")
    artifacts += [DEST / "READ-ME-FIRST.md", DEST / "RELEASE-NOTES.md"]
    hashes = {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in artifacts}
    (DEST / "SHA256SUMS.txt").write_text("".join(f"{digest}  {name}\n" for name, digest in hashes.items()))
    repository = (ROOT / "packaging/repository.txt").read_text().strip().removesuffix("/").removesuffix(".git")
    manifest = dict(tag=args.tag, app_version=version, architectures=architectures,
                    min_macos=info["LSMinimumSystemVersion"], repository=repository or None,
                    signing=signing, notarization="not_performed_by_local_builder",
                    releases_url=f"{repository}/releases" if repository else None,
                    publication="not_uploaded", source_files=len(sources),
                    source_revision=run("git", "rev-parse", "HEAD"),
                    source_snapshot="current working tree including uncommitted changes",
                    source_manifest={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in sources},
                    sha256=hashes, bundled_character="shime", images=46,
                    custom_character_art=False, second_mac_first_open="not_tested",
                    checks=dict(zip_crc="PASS", signatures="PASS", entitlements="PASS",
                                bundled_shime_byte_identity="PASS", public_source_filter="PASS"))
    (DEST / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Prepared {args.tag}: {len(sources)} public source files, {label}, {DEST}")


if __name__ == "__main__":
    main()
