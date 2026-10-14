#!/usr/bin/env python3
"""Stage a Shimeji pack into the working directory the reference engine loads.

The engine only reads conf/ and img/<imageSet>/. Corpus packs also use an
actions file at the pack root, or PNGs at the pack root beside conf/. This
script symlinks those bytes into the engine layout. It does not copy art
and it does not modify the pack.
"""

import argparse
import os
import shutil
import sys
from pathlib import Path

ACTIONS = ("actions.xml", "動作.xml", "one.xml", "1.xml")
BEHAVIORS = ("behaviors.xml", "behavior.xml", "行動.xml", "two.xml", "2.xml")
IMAGE_EXT = {".png", ".gif", ".bmp", ".jpg", ".jpeg", ".webp"}
SKIP_DIRS = {"conf", "sound", "unused"}

LOGGING = """handlers=java.util.logging.ConsoleHandler
.level=WARNING
java.util.logging.ConsoleHandler.level=WARNING
java.util.logging.ConsoleHandler.formatter=java.util.logging.SimpleFormatter
"""


def find_named(directories, names):
    for directory in directories:
        if not directory.is_dir():
            continue
        for name in names:
            candidate = directory / name
            if candidate.is_file():
                return candidate
    return None


def image_subdirs(img):
    found = []
    if not img.is_dir():
        return found
    for child in sorted(img.iterdir()):
        if child.is_dir() and child.name != "unused":
            found.append(child)
    return found


def detect(pack):
    img = pack / "img"
    subs = image_subdirs(img)
    root_actions = find_named([pack], ACTIONS)
    has_conf = (pack / "conf").is_dir()
    if subs:
        return "image-sets", subs
    if root_actions is not None:
        return "root-xml", []
    if has_conf and not img.is_dir():
        return "dropped", []
    if has_conf and img.is_dir():
        return "classic", []
    return None, []


def config_dirs(pack, layout, image_set):
    img = pack / "img"
    if layout == "image-sets":
        return [img / image_set / "conf", pack / "conf" / image_set, pack / "conf"]
    if layout == "root-xml":
        return [pack]
    if layout == "dropped":
        return [pack / "conf"]
    if layout == "classic":
        return [pack / "conf"]
    return []


def image_root(pack, layout, image_set):
    img = pack / "img"
    if layout == "image-sets":
        return img / image_set
    if layout == "root-xml":
        return img if img.is_dir() else None
    if layout == "dropped":
        return pack
    if layout == "classic":
        return img
    return None


def link_images(src, dest_root):
    count = 0
    if src is None or not src.exists():
        return count
    for dirpath, dirnames, filenames in os.walk(src, followlinks=False):
        dirnames[:] = [name for name in dirnames if name not in SKIP_DIRS]
        current = Path(dirpath)
        for filename in filenames:
            source = current / filename
            if source.suffix.lower() not in IMAGE_EXT or not source.is_file():
                continue
            relative = source.relative_to(src)
            if any(part in SKIP_DIRS for part in relative.parts):
                continue
            target = dest_root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.is_symlink() or target.exists():
                target.unlink()
            target.symlink_to(source.resolve())
            count += 1
    return count


def symlink_file(source, dest):
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_symlink() or dest.exists():
        dest.unlink()
    dest.symlink_to(source.resolve())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack", required=True, type=Path)
    parser.add_argument("--dest", required=True, type=Path)
    parser.add_argument("--image-set", default="")
    parser.add_argument("--vendor", type=Path, default=None)
    args = parser.parse_args()

    pack = args.pack.resolve()
    if not pack.is_dir():
        print(f"pack is not a directory: {pack}", file=sys.stderr)
        return 1

    here = Path(__file__).resolve().parent
    vendor_conf = (args.vendor or (here / "vendor" / "conf")).resolve()
    if not vendor_conf.is_dir():
        print(f"vendor conf is missing: {vendor_conf}", file=sys.stderr)
        return 1

    layout, subs = detect(pack)
    if layout is None:
        print(f"unrecognized pack layout: {pack}", file=sys.stderr)
        return 1

    image_set = args.image_set
    if layout == "image-sets":
        names = [path.name for path in subs]
        if not image_set:
            if len(names) != 1:
                print("multiple image sets: " + ", ".join(names), file=sys.stderr)
                return 1
            image_set = names[0]
        elif image_set not in names:
            print(
                f"image set {image_set} is not in {pack.name} ({', '.join(names)})",
                file=sys.stderr,
            )
            return 1
    elif not image_set:
        image_set = pack.name

    directories = config_dirs(pack, layout, image_set)
    actions = find_named(directories, ACTIONS)
    behaviors = find_named(directories, BEHAVIORS)
    if actions is None or behaviors is None:
        missing = []
        if actions is None:
            missing.append("actions")
        if behaviors is None:
            missing.append("behaviors")
        print(
            f"missing {' and '.join(missing)} for {pack.name} layout {layout}",
            file=sys.stderr,
        )
        return 1

    dest = args.dest
    if dest.exists() and dest.resolve() == pack:
        print("dest and pack are the same directory", file=sys.stderr)
        return 1
    for stale in (dest / "conf", dest / "img"):
        if stale.exists():
            shutil.rmtree(stale)
    conf = dest / "conf"
    images = dest / "img" / image_set
    conf.mkdir(parents=True, exist_ok=True)
    images.mkdir(parents=True, exist_ok=True)

    (conf / "logging.properties").write_text(LOGGING, encoding="utf-8")
    for language in sorted(vendor_conf.glob("language*.properties")):
        symlink_file(language, conf / language.name)
    symlink_file(actions, conf / actions.name)
    symlink_file(behaviors, conf / behaviors.name)
    info = find_named(directories, ("info.xml",))
    if info is not None:
        symlink_file(info, conf / "info.xml")

    count = link_images(image_root(pack, layout, image_set), images)
    print(f"layout\t{layout}")
    print(f"image-set\t{image_set}")
    print(f"actions\t{actions.name}")
    print(f"behaviors\t{behaviors.name}")
    print(f"images\t{count}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
