#!/usr/bin/env python3
"""Patch a fresh Shimeji-Desktop checkout for the reference harness.

Replacements are exact. The script exits if a site is missing or if any
Math.random / new Random call remains under src/main/java afterwards.
"""

from __future__ import annotations

import sys
from pathlib import Path

SITES = [
    "src/main/java/com/group_finity/mascot/Main.java",
    "src/main/java/com/group_finity/mascot/config/Configuration.java",
    "src/main/java/com/group_finity/mascot/behavior/UserBehavior.java",
    "src/main/java/com/group_finity/mascot/action/Regist.java",
    "src/main/java/com/group_finity/mascot/action/Dragged.java",
]

EXPECTED_CALLS = 8


def die(message: str) -> None:
    print(f"apply-patches.py: {message}", file=sys.stderr)
    sys.exit(1)


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        die(f"{label}: expected 1 occurrence, found {count}")
    return text.replace(old, new, 1)


def add_import(text: str) -> str:
    needle = "import shimeji2.ref.SeededRandom;\n"
    if needle in text:
        return text
    marker = "package "
    start = text.find(marker)
    if start < 0:
        die("package declaration missing")
    line_end = text.find("\n", start)
    return text[: line_end + 1] + "\n" + needle + text[line_end + 1 :]


def patch_random(root: Path) -> int:
    total = 0
    for rel in SITES:
        path = root / rel
        text = path.read_text(encoding="utf-8")
        calls = text.count("Math.random()")
        if calls == 0:
            die(f"{rel}: no Math.random() call")
        text = text.replace("Math.random()", "SeededRandom.random()")
        text = add_import(text)
        path.write_text(text, encoding="utf-8")
        total += calls
        print(f"patched {calls} Math.random() call(s) in {rel}")
    if total != EXPECTED_CALLS:
        die(f"patched {total} calls, expected {EXPECTED_CALLS}")
    return total


def patch_native_factory(root: Path) -> None:
    path = root / "src/main/java/com/group_finity/mascot/platform/NativeFactory.java"
    text = path.read_text(encoding="utf-8")
    old = """    public static void resetInstance() {
        boolean windowedMode = Main.getInstance().getSettings().windowedMode;
"""
    new = """    public static void resetInstance() {
        // shimeji2-ref: the harness installs HeadlessNativeFactory before any platform factory.
        if ("headless".equals(System.getProperty("shimeji2.ref.factory"))) {
            instance = new HeadlessNativeFactory();
            return;
        }
        boolean windowedMode = Main.getInstance().getSettings().windowedMode;
"""
    path.write_text(replace_once(text, old, new, "NativeFactory.resetInstance"), encoding="utf-8")


def patch_main(root: Path) -> None:
    path = root / "src/main/java/com/group_finity/mascot/Main.java"
    text = path.read_text(encoding="utf-8")
    old_err = """    public static void showError(Component parentComponent, String message, Throwable exception) {
        ResourceBundle languageBundle = INSTANCE.languageBundle;
"""
    new_err = """    public static void showError(Component parentComponent, String message, Throwable exception) {
        // shimeji2-ref: a dialog would block the headless runner.
        if ("headless".equals(System.getProperty("shimeji2.ref.factory"))) {
            log.error(message, exception);
            return;
        }
        ResourceBundle languageBundle = INSTANCE.languageBundle;
"""
    text = replace_once(text, old_err, new_err, "Main.showError")
    old_exit = """    public void exit() {
        manager.disposeAll();
"""
    new_exit = """    public void exit() {
        // shimeji2-ref: System.exit would hide the stack that asked to quit.
        if ("headless".equals(System.getProperty("shimeji2.ref.factory"))) {
            throw new IllegalStateException("Main.exit() called under the reference harness");
        }
        manager.disposeAll();
"""
    text = replace_once(text, old_exit, new_exit, "Main.exit")
    path.write_text(text, encoding="utf-8")


def patch_pose(root: Path) -> None:
    path = root / "src/main/java/com/group_finity/mascot/animation/Pose.java"
    text = path.read_text(encoding="utf-8")
    old = """    public void apply(final Mascot mascot) {
        mascot.getAnchor().translate(mascot.isLookRight() ? -dx : dx, dy);
"""
    new = """    public void apply(final Mascot mascot) {
        // shimeji2-ref: remember the image-pair key so the trace can record the path.
        mascot.noteImageKey(imageKey);
        mascot.getAnchor().translate(mascot.isLookRight() ? -dx : dx, dy);
"""
    path.write_text(replace_once(text, old, new, "Pose.apply"), encoding="utf-8")


def patch_mascot(root: Path) -> None:
    path = root / "src/main/java/com/group_finity/mascot/Mascot.java"
    text = path.read_text(encoding="utf-8")
    old = """    private MascotImage image = null;

    /**
     * Whether this {@code Mascot} is facing right.
"""
    new = """    private MascotImage image = null;

    /**
     * Image-pair key of the pose most recently applied.
     * Written by the reference-harness patch in {@link com.group_finity.mascot.animation.Pose#apply}.
     * shimeji2-ref: image key
     */
    private String imageKey;

    /** @return the image-pair key last passed to {@link #noteImageKey}, or null */
    public String getImageKey() {
        return imageKey;
    }

    /** shimeji2-ref: called from the patched {@code Pose.apply}. */
    public void noteImageKey(final String imageKey) {
        this.imageKey = imageKey;
    }

    /**
     * Whether this {@code Mascot} is facing right.
"""
    path.write_text(replace_once(text, old, new, "Mascot.imageKey"), encoding="utf-8")


def patch_script(root: Path) -> None:
    path = root / "src/main/java/com/group_finity/mascot/script/Script.java"
    text = path.read_text(encoding="utf-8")
    old = """            compiled = ENGINE.compile(this.source);
"""
    new = """            // shimeji2-ref: compiled Math.random is Nashorn's builtin and ignores a later assignment.
            // The harness pins __shimeji2Random on the global before any script is compiled.
            compiled = ENGINE.compile(this.source.replace("Math.random", "__shimeji2Random"));
"""
    text = replace_once(text, old, new, "Script.compile")
    old = """            value = compiled.eval(variables);
            needsReevaluation = false;
"""
    new = """            // shimeji2-ref: compiled scripts only see this variable map, not ENGINE_SCOPE.
            if (!variables.getRawMap().containsKey("__shimeji2Random")) {
                variables.put("__shimeji2Random", shimeji2.ref.SeededRandom.JS);
            }
            value = compiled.eval(variables);
            needsReevaluation = false;
            // shimeji2-ref: verbose traces log this evaluation. No-op unless enabled.
            shimeji2.ref.EvalTrace.record(source, value, variables);
"""
    path.write_text(replace_once(text, old, new, "Script.get"), encoding="utf-8")


def assert_clean(root: Path) -> None:
    java_files = list((root / "src/main/java").rglob("*.java"))
    math_left = []
    random_left = []
    seeded = []
    for path in java_files:
        text = path.read_text(encoding="utf-8")
        rel = path.relative_to(root).as_posix()
        if "Math.random(" in text:
            math_left.append(rel)
        if "new Random(" in text or "new java.util.Random" in text:
            random_left.append(rel)
        count = text.count("SeededRandom.random()")
        if count:
            seeded.append(f"{rel}:{count}")
    if math_left or random_left:
        die("call sites remain: Math.random=" + str(math_left) + " Random=" + str(random_left))
    print("SeededRandom.random() sites: " + ", ".join(seeded))


def main() -> None:
    if len(sys.argv) != 2:
        die("usage: apply-patches.py VENDOR_DIR")
    root = Path(sys.argv[1])
    if not (root / "pom.xml").is_file():
        die(f"{root} is not a Shimeji-Desktop checkout")
    patch_random(root)
    patch_native_factory(root)
    patch_main(root)
    patch_pose(root)
    patch_mascot(root)
    patch_script(root)
    assert_clean(root)


if __name__ == "__main__":
    main()
