#!/usr/bin/env python3
"""Check relocated candidate app, icon, dependencies and normal >20s startup.

Requires no running mascot; uses a fresh /tmp profile and stops only its own app.
No input gestures or second-machine checks are claimed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import signal
import subprocess
import tempfile
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, default=ROOT / "target/release-candidate")
    parser.add_argument("--out", type=Path, default=ROOT / "output/release-prep-20261014")
    args = parser.parse_args()
    assert subprocess.run(["pgrep", "-x", "shimeji-macos"], capture_output=True).returncode != 0, "Quit the running mascot first"
    manifest = json.loads((args.candidate / "manifest.json").read_text())
    for name, digest in manifest["sha256"].items():
        assert hashlib.sha256((args.candidate / name).read_bytes()).hexdigest() == digest, name
    archive = next(args.candidate.glob("*-macos-*.zip"))
    with zipfile.ZipFile(archive, metadata_encoding="utf-8") as stream:
        assert stream.testzip() is None
        assert not any(".DS_Store" in n or "settings.json" in n or "running.log" in n for n in stream.namelist())
    stage = Path(tempfile.mkdtemp(prefix="shimeji2-release-", dir="/tmp"))
    subprocess.run(["ditto", "-x", "-k", str(archive.resolve()), str(stage)], check=True)
    app = stage / "shimeji2.app"
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    with (app / "Contents/Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    assert info["CFBundleIdentifier"] == "ai.shimeji2.macos"
    assert info["LSMinimumSystemVersion"] == "14.0"
    binary = app / "Contents/MacOS/shimeji-macos"
    for target in (app / "Contents/MacOS").iterdir():
        assert set(run("lipo", "-archs", str(target)).split()) == set(manifest["architectures"])
        for line in run("otool", "-L", str(target)).splitlines()[1:]:
            dependency = line.strip().split(" (", 1)[0]
            assert dependency.startswith(("/System/Library/", "/usr/lib/")), dependency
    icons = stage / "decoded.iconset"
    subprocess.run(["iconutil", "-c", "iconset", str(app / "Contents/Resources/shimeji2.icns"), "-o", str(icons)], check=True)
    assert len(list(icons.glob("*.png"))) == 10
    args.out.mkdir(parents=True, exist_ok=True)
    profile = stage / "profile"
    profile.mkdir()
    (profile / "first-run.json").write_text('{"seen":true}\n')
    stdout = args.out.resolve() / "native-startup.log"
    stderr = args.out.resolve() / "native-stderr.log"
    stdout.write_text("")
    stderr.write_text("")
    command = ["open", "-W", "-n", "--stdout", str(stdout), "--stderr", str(stderr), str(app),
               "--args", "--profile", str(profile), "--agent-home", str(profile / "agent-home"), "--seed", "1"]
    launcher = subprocess.Popen(command, env={**os.environ, "NSUnbufferedIO": "YES"})
    pid = None
    start = time.monotonic()
    try:
        while time.monotonic() - start < 30:
            assert launcher.poll() is None, "Normal application exited before30seconds"
            listing = subprocess.run(["pgrep", "-x", "shimeji-macos"], capture_output=True, text=True)
            if listing.returncode == 0:
                for candidate in listing.stdout.split():
                    command_line = run("ps", "-p", candidate, "-o", "args=")
                    if str(binary) in command_line and str(profile) in command_line:
                        pid = int(candidate)
            time.sleep(0.5)
        assert pid is not None, "Did not identify owned application process"
        text = stdout.read_text()
        loaded_path = next(line.split("=", 1)[1] for line in text.splitlines() if line.startswith("pack_source="))
        assert Path(loaded_path).resolve() == (app / "Contents/Resources/packs/shime").resolve()
        assert "decoded_images=46" in text
        assert "agent_hooks=not_installed" in text and "agent_listener=off" in text
        assert "mischief=off" in text
    finally:
        if pid is not None:
            command_line = subprocess.run(["ps", "-p", str(pid), "-o", "args="], capture_output=True, text=True).stdout
            if str(binary) in command_line and str(profile) in command_line:
                os.kill(pid, signal.SIGTERM)
        try:
            launcher.wait(timeout=10)
        except subprocess.TimeoutExpired:
            launcher.terminate()
    validation = dict(zip_hashes="PASS", zip_crc="PASS", signature="PASS", relocated_pack="PASS",
                      images=46, architectures=manifest["architectures"], system_dependencies_only="PASS",
                      icns_representations=10, normal_launch_alive_seconds=30, no_seconds_flag=True,
                      profile=str(profile), extracted_app=str(app), native_second_mac="not_tested",
                      physical_drag_menu_settings="manual_checklist", iconset=str(icons))
    (args.out / "native-validation.json").write_text(json.dumps(validation, indent=2) + "\n")
    print(json.dumps(validation, indent=2))


if __name__ == "__main__":
    main()
