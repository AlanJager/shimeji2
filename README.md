# shimeji2

**Shimeji desktop pets for macOS, rewritten in Rust.** shimeji2 runs classic Shimeji character packs as a native menu-bar app. It needs no Java.

English · [简体中文](README.zh-CN.md)

<p align="center"><img src="packaging/icons/shimeji2.png" width="160" alt="shimeji2 application icon"></p>

> [!NOTE]
> **Public beta: v0.1.0-beta.1.** The app itself reports version 0.1.0.
> - **Platform.** Apple Silicon Macs running macOS 14 or later. So far it has been built and run only on the maintainer's Mac (macOS 15). Intel Macs are not a target of this beta.
> - **Not notarized.** The first launch needs a one-time approval in System Settings; see [Install](#install).
> - **Language.** The Settings window is in Chinese only. The menus are in English.

## What it is

Shimeji are small characters that live on your desktop. They walk along the bottom of the screen, climb the screen edges and the window in front, hang from the ceiling, sit on top of that window and multiply. You can pick one up and throw it. The original Shimeji and its popular Shimeji-ee fork are Java apps.

shimeji2 is a Rust rewrite of [shimeji4mac](https://github.com/AlanJager/shimeji4mac) (Shimeji for Mac). It has no Java underneath:

- **Native.** It is a menu-bar app with no Dock icon. Each pet is a transparent, borderless window. Clicks on the empty pixels around a pet pass through to the app underneath.
- **Behaves like Shimeji-ee.** The engine is checked tick by tick against the Java reference; see [How faithful is it?](#how-faithful-is-it).
- **Supported pack formats.** It loads Classic (Japanese XML) and Shimeji-ee (English XML) packs in the common folder layouts, and Codex pet sprite sheets. Some Shimeji-ee extensions are not implemented yet; see the [compatibility notes](#compatibility-notes).
- **Import limits.** Packs are decoded in pure Rust under size, file-count and image-size caps. Links and special files are rejected. Each import is checked in a staging folder first, so a broken pack cannot replace a working one.
- **Stops when not needed.** The clock stops while pets are paused or hidden, when the display sleeps or the screen is locked, and when Reduce Motion is on.

It ships with one character, the original white **shime**. Its 46 images and two behavior files are byte-for-byte copies of the shimeji4mac files.

## Install

1. Download the DMG or ZIP from [Releases](https://github.com/AlanJager/shimeji2/releases). Check it against `SHA256SUMS.txt` from the same release, for example with `shasum -a 256 shimeji2-*.dmg`. If no build has been published yet, [build it from source](#build-from-source).
2. Move `shimeji2.app` to **Applications**.
3. Open it. Because the beta is not notarized, macOS will refuse the first time. Go to **System Settings → Privacy & Security**, click **Open Anyway**, then confirm **Open** and enter your password. Do not turn off Gatekeeper. [Apple's guide](https://support.apple.com/en-us/102445) explains this step. The `READ-ME-FIRST.md` file next to the app in the DMG or ZIP covers it too, in Chinese.

No special permissions are needed for desktop pets. Accessibility access is requested only if you turn on [window mischief](#window-mischief).

**Uninstall**

1. If you installed agent hooks, remove them first with **卸载… (Uninstall…)** in **Settings → 智能体 (Agents)**. Those entries live in `~/.claude/settings.json` and `~/.codex/config.toml` and point at the app.
2. If you turned on Launch at login, remove shimeji2 under **System Settings → General → Login Items** (**Login Items & Extensions** on macOS 15).
3. Choose **Quit** from the menu bar and delete the app.
4. To also remove settings, imported packs and the mischief journal, delete `~/Library/Application Support/shimeji2`.

## Using it

At launch, one shime drops onto your main display.

| To… | Do this |
|---|---|
| Move a pet | Hold the left mouse button on it and drag. |
| Throw a pet | Let go while moving the mouse. It flies off at roughly the mouse's speed. |
| Open a pet's menu | Right-click or Control-click it. See [Pet menu](#pet-menu). |
| Hide or show all pets | Press **Control + Shift + S**. |
| Everything else | Use the **し** item in the menu bar. |

### Multiplying

Pets multiply by default, up to 50. To stop it, turn off breeding (自动繁殖) or lower the pet limit (宠物数量上限) in **Settings → 外观与动作 (Appearance & Behavior)**. To clear extras, use **Remove Newest** or **Dismiss All**.

### Reduce Motion

If **Reduce Motion** is on in macOS, the pet stands still at the bottom of the screen. To let it move anyway, turn on **减少动态效果时继续动画 (Keep animating with Reduce Motion)** in **Settings → 高级 (Advanced)**.

### Pet menu

Right-click or Control-click a pet:

| Item | What it does |
|---|---|
| **Dismiss** | Removes this pet. |
| **Dismiss All** | Removes every pet of this character. |
| **Follow Cursor** | That character's pets chase the pointer. |
| **Restore Moved Windows** | Greyed out; not active yet. |
| **Behaviors** | Toggles the pack's optional behaviors. Greyed out for the bundled shime, which has none. |
| **Pause/Resume** | Pauses or resumes all pets. |

### Menu bar

The **し** menu contains:

| Item | What it does |
|---|---|
| **Characters** | Lists each installed character, with **Add** and **Remove Newest**. |
| **Pause/Resume** | Pauses or resumes all pets. |
| **Hide All/Show All** | Hides or shows all pets. |
| **Dismiss All** | Removes every pet. |
| **Mischief** | Turns [window mischief](#window-mischief) on or off. |
| **Settings…** | Opens the Settings window. |
| **Check for Updates…** | Opens the Releases page in your browser. It is greyed out in builds made without a repository URL, such as `cargo run`. The app never checks for updates on its own. |
| **Quit** | Quits the app. |

On first launch, a small popover under the menu bar item shows the shortcut and offers **Launch at login**.

While pets are paused, a dragged pet is set down where you let go instead of being thrown.

### Settings

Changes save automatically.

| Page | What it contains |
|---|---|
| 宠物 (Pets) | Your installed packs with their characters and behaviors.<br>Characters, and behaviors the pack marks as optional, have enable checkboxes.<br>A pack with import warnings is marked **!**; select it to see the warnings.<br>[Import](#adding-characters) is here too. |
| 外观与动作 (Appearance & Behavior) | Pet size (25%–800%), scaling filter, opacity.<br>Keep on screen, move across displays.<br>Pet limit (default 50, at most 100), breeding, temporary clones.<br>**允许抛掷 (Allow throwing)**, see below.<br>A greyed-out character-transformation switch. |
| 窗口互动 (Window interaction) | [Window mischief](#window-mischief) and its window rules. |
| 智能体 (Agents) | [Coding-agent reactions](#coding-agent-reactions). |
| 高级 (Advanced) | The hide/show shortcut: type a chord that includes Control or Command, such as `ctrl+shift+s`, then press Return.<br>Pack sounds.<br>**减少动态效果时继续动画 (Keep animating with Reduce Motion)**.<br>A diagnostics panel. |

Notes on these settings:

- **Allow throwing.** Despite its hint text, **允许抛掷 (Allow throwing)** controls whether pets may carry and throw *windows* when mischief is on. You can always fling pets.
- **Pet limit.** The menu's **Add** counts all pets. Breeding is limited per character, though, so several breeding characters together can go past the limit.
- **Sounds.** Sounds are off by default. When turned on, only uncompressed (integer PCM) WAV files included in a pack are played. The bundled shime has none.

## Adding characters

Open **Settings → 宠物 (Pets)** and click **导入素材… (Import assets…)**, or drag ZIP files or folders onto the list. Then choose **Characters → *name* → Add** from the menu bar to bring a pet out. Imported characters are not placed on screen automatically at launch; only the bundled shime is.

> [!IMPORTANT]
> The pack's files (`conf/`, `img/`, the XML, `pet.json` or `pack.json`) must sit at the top level of the ZIP or folder. A ZIP whose contents are inside one extra folder, which is what Finder's **Compress** makes, fails with "no actions file found". Unzip it and import the inner folder instead.

| Format | What it looks like |
|---|---|
| **Classic** Shimeji | XML with Japanese tags (root `<マスコット>`), usually `conf/動作.xml` and `conf/行動.xml` |
| **Shimeji-ee** | XML with English tags (root `<Mascot>`), usually `conf/actions.xml` and `conf/behaviors.xml` |
| **Codex pets** | `pet.json` plus one PNG sprite sheet, 1536×1872 (v1) or 1536×2288 (v2) |
| **Native** | shimeji2's own `pack.json` with a fixed 8×6 sprite atlas (see [`native/manifest.rs`](crates/shimeji-packs/src/native/manifest.rs)) |

Classic and Shimeji-ee packs can use any of the usual layouts:

- `conf/` and `img/`
- image-set folders under `img/<name>/`
- XML in the pack root
- `conf/` next to loose PNGs

Either dialect may use any of the accepted file names: `actions.xml`, `動作.xml`, `one.xml` or `1.xml` for actions, and `behaviors.xml`, `behavior.xml`, `行動.xml`, `two.xml` or `2.xml` for behaviors.

**Import rules**

- **Same name.** If a pack with the same name (the folder or ZIP name) is already installed, you can **Replace** it, **Keep Both** (the new copy gets a `-2` suffix) or **Cancel**.
- **Size.** An import may hold at most 200 MB of files and 5,000 files and folders, nested at most 16 levels deep.
- **Images.** Each image may be at most 4096 px on a side. A pack's images and sounds together may take at most 512 MB once decoded.
- **ZIP format.** ZIPs must use ordinary (stored or Deflate) compression and cannot be password-protected.
- **Links.** A symlink or other special file anywhere in the pack rejects the import.
- **Problems.** Going over any limit rejects the whole import. Smaller problems still let the pack load, and its warnings appear on the Pets page.

Settings can turn off characters but cannot delete a pack. To delete an imported pack, quit the app and remove its folder under `~/Library/Application Support/shimeji2/packs/`. The bundled shime is reinstalled there at every launch, so turn its character off instead.

Apart from the bundled shime, no characters are included in this repository or in the app. Check each pack's terms before importing or sharing it.

### Compatibility notes

Most packs that the Java reference can run behave the same way here. Known gaps:

- **Unimplemented Shimeji-ee actions.** Seventeen extension actions are recognised but run as a plain "stay": the pet stays where it is and plays that action's animation, if it has one. Most of them set no duration, so a pet can stay in one until it is dragged or loses its footing. They are:
  - the `Broadcast*`, `Scan*` and `Complex*` families
  - `BreedJump`, `BreedMove`, `Interact`, `Transform`, `SelfDestruct`, `Mute`, `Turn` and `MoveWithTurn`
- **Unknown action classes.** Classes the engine does not know at all also become "stay", with a warning, so the pack still loads. The Java reference refuses to load such packs.
- **Ignored attributes.** Click hotspots are read but not used. `BornMascot` on breeding is ignored, so children are always the parent's character.

## Optional features

These are **experimental and off by default**. The core pets do not need them.

### Window mischief

Pets pick up the frontmost window, carry it and throw it, as the original Shimeji did. Turn it on from **Mischief** in the menu bar or in **Settings → 窗口互动 (Window interaction)**. It needs Accessibility permission. Because the beta is ad-hoc signed, you have to grant that permission again after every update.

- **What moves.** Only the current frontmost window is moved, and only its position. Windows are never resized.
- **Protected windows.** macOS's own login and authorization prompts and System Settings are never touched. 1Password, Bitwarden, Keychain Access and Terminal are protected by default. Sign-in windows of other apps are not covered unless you block them.
- **Window rules.** You can add allow or block rules that match part of an app name or window title (case-sensitive). Adding any allow rule turns the list into an allowlist: after that, only matching windows can be moved.
- **Restoring windows.** Every carry is logged first to a journal in the profile folder, which never stores window titles. Moved windows go back where they were when you turn mischief off or quit, unless you have moved or closed them yourself since. After a crash, they go back the next time mischief is on with Accessibility granted, within 24 hours.
- **App support.** Whether a window can move at all depends on the app that owns it.

> [!WARNING]
> Protection matches app names in English. On a Mac set to another language, such as Chinese, the built-in protection for System Settings, Terminal and Keychain Access may not apply. Add block rules with the translated names (for example 系统设置, 终端 and 钥匙串访问), or leave mischief off.

Walking on windows is a separate feature. It works without mischief and without any permission.

### Coding-agent reactions

shimeji2 can react to what [Claude Code](https://claude.com/claude-code) or [Codex](https://github.com/openai/codex) is doing:

- When the agent starts working, the pet sits down.
- When the agent needs your input, the pet swings its legs.
- On a failure, the pet trips.
- When the agent finishes, the pet jumps. With the bundled shime, the jump currently heads for the top-left corner of the main display.

Each pose plays once when the state changes. Then the pet goes back to its usual routine. A small badge above the pet keeps showing the state; click it to bring the terminal forward.

> [!CAUTION]
> **Do not install the hooks in v0.1.0-beta.1.** The installer points them at the app's own executable instead of the bundled `shimeji-hook`. The app exits with an error on the hook's argument, and Claude Code can treat that as a blocking error for every prompt and tool call. If you already installed them, use **卸载… (Uninstall…)** in **Settings → 智能体 (Agents)**.

How it is meant to work, once the installer is fixed:

- **Two steps.** The switch in **Settings → 智能体 (Agents)** starts the app's listener. Hooks are installed separately with **安装… (Install…)**, which works only while `shimeji2.app` runs from an Applications folder.
- **Review first.** The installer shows the before-and-after of the `hooks` entries in `~/.claude/settings.json` and the `notify` line in `~/.codex/config.toml`. Nothing is written until you click **应用更改 (Apply changes)**.
- **What changes.** Applying rewrites `settings.json` with its keys sorted alphabetically. The previous files are kept as `*.shimeji2-bak.1` to `.5`.
- **Existing Codex setting.** If `~/.codex/config.toml` already sets `notify`, the install is refused for both agents.
- **What is sent.** `shimeji-hook` always exits successfully and prints nothing. It sends the agent name, session id, state and the name of your terminal app to shimeji2 over a private local socket. It never sends prompt text, tool input or file paths.
- **Limits.** Codex reports only "done", and code-review events are not wired up yet.

### forge (source checkout only)

The `shimeji-forge` crate builds a command-line tool named `forge`. It helps make a new character with a coding agent's own image tool:

1. It prepares the generation jobs.
2. It extracts and places the frames against the 46-pose shime layout, and runs quality checks.
3. It writes contact sheets and GIF previews.
4. `forge install` asks the running app for confirmation before importing anything. It can reach the app only while agent reactions are on. Otherwise it prints the folder for you to drag onto the Pets page.

Current gaps:

- Forge is not part of the app bundle. Run it with `cargo run -p shimeji-forge -- <command>`.
- It does not write the pack's behavior XML yet, so copy a `conf/` folder in yourself; the bundled shime's matches the 46-pose layout.
- Online generation providers are not implemented.

See [`integrations/skills/shimeji-forge/SKILL.md`](integrations/skills/shimeji-forge/SKILL.md).

## How faithful is it?

The reference behavior is [DalekCraft2/Shimeji-Desktop](https://github.com/DalekCraft2/Shimeji-Desktop), a Shimeji-ee fork, at the commit in [`tools/ee-reference/PIN`](tools/ee-reference/PIN). A headless Java harness runs scripted scenarios through it with a fixed seed. These cover walking, riding a moving window, dragging and throwing, breeding limits and two screens. For every tick it records the behavior, action, position, image, facing direction and the running count of random draws. `cargo test` replays the same scenarios in Rust and compares the traces. No Java is needed for that step.

Pack scripts run in a small sandboxed JavaScript-subset evaluator instead of Java's Nashorn. A bit-exact port of `java.util.Random` makes the same random choices in the same order.

- **Committed traces.** There are 13, from the bundled shime and synthetic packs. Twelve match the reference on every tick line. The thirteenth differs on purpose: a non-boolean condition aborts the Java tick, while shimeji2 treats it as false and keeps going.
- **Third-party packs.** A local corpus of 50 packs was run through the reference, which refused to load 15 of them.
  - Of the other 35, 33 use no unimplemented extensions.
  - 32 of those 33 produce identical traces in all seven scenarios.

  Corpus art and traces stay on the maintainer's machine; see [`docs/testing/golden-traces.md`](docs/testing/golden-traces.md).
- **Scripts.** The expression test vectors take their expected results from the reference's own Nashorn and are never edited by hand. They are hand-picked coercion cases plus every expression in the bundled shime.

Most deliberate differences are safety limits:

- Each tick has an evaluation budget.
- Chains of behavior hand-offs are capped.
- Expressions have step, depth and size limits.

Where the Java app would crash or recurse forever, shimeji2 cuts the evaluation short instead: the condition counts as false, or that one pet is removed, and a diagnostic is recorded. The other deliberate differences are the error-handling choices above: non-boolean conditions and unknown action classes.

## Known issues and limitations

These issues are confirmed in v0.1.0-beta.1 and tracked for a fix:

- **Packs with several image sets.** In packs with several image sets (`img/<name>/`) that reuse file names, a character can draw another set's art and hit area. A pose stored in a subfolder of its set can be invisible.
- **Several displays.** With more than one display, pets can jump or fall when keyboard focus moves to a window on another display.
- **Pet size.** At sizes other than 100%, pets stand slightly above or below the floor and can be clipped at the edges of their window.
- **Pack sounds.** Sounds repeat for as long as a pose lasts. Shimeji-ee `Volume` values are read differently: `0` and negative values are silent. The app can crash when a pack mixes mono and stereo WAVs, or when the audio output device changes. Keep **播放动作声音 (Play action sounds)** off for now.
- **Mischief restore.** If Accessibility is revoked while windows are being carried, or a carried window is minimized or on another Space at quit, that window is not put back.
- **Import.** Dropping a non-ZIP file whose name ends in a non-ASCII character, such as `写真`, crashes the app. A pack you import under the name `shime` is replaced by the bundled one at the next launch.
- **Pet limit.** Type the pet limit with half-width digits. Full-width digits from an input method set it to 1.
- **Agent hooks.** Installing them breaks Claude Code; see [Coding-agent reactions](#coding-agent-reactions).

Other limitations:

- **Testing coverage.** Only the maintainer's Apple Silicon Mac on macOS 15 has been tested. Still unverified:
  - a first launch on another Mac
  - multi-display setups
  - sleep and wake
  - long runs

  See [`docs/testing/public-beta-checklist.md`](docs/testing/public-beta-checklist.md) and [`docs/release.md`](docs/release.md).
- **Intel.** A universal build is possible from source but untested.
- **Unavailable settings.** The character-transformation setting and the HQX scaling filter are shown but not available yet.

## Contributing

### Build from source

The Rust toolchain is pinned in `rust-toolchain.toml`. Normal builds and tests need no Java and no Gradle.

```sh
cargo build --workspace
cargo run -p shimeji-macos -- --profile /tmp/shimeji2-dev   # macOS only
```

`cargo run` loads the shime from `assets/packs/shime`. Without `--profile`, it uses the same `~/Library/Application Support/shimeji2` folder as the installed app.

Before sending a change, all three must pass:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

To build the `.app`, DMG and ZIP on a Mac (needs the Xcode Command Line Tools):

```sh
./scripts/bundle.sh --dev      # add --offline once dependencies are cached
```

- **Output.** The output goes to `target/bundle/`.
- **`--dev`.** This gives the build its own bundle id (`ai.shimeji2.macos.dev`), and so its own Accessibility entry. Leave it out only for release builds.
- **Architectures.** The script builds a universal app when both `aarch64-apple-darwin` and `x86_64-apple-darwin` are installed. Otherwise it builds for the host.
- **Releases.** [`docs/release.md`](docs/release.md) covers the full release flow.

To try a bundled build without touching your everyday setup, quit your everyday copy first. Then give the trial a throwaway profile and agent home:

```sh
open -W -n target/bundle/shimeji2.app --args --seconds 30 \
  --profile /tmp/shimeji2-try --agent-home /tmp/shimeji2-try/agent-home
```

`--seconds N` makes it quit on its own. Without it, the app runs until you choose Quit. Installing agent hooks always edits your real `~/.claude` and `~/.codex` configuration, whatever profile is in use.

### Project layout

| Crate | Role |
|---|---|
| `shimeji-expr` | Sandboxed expression evaluator and bit-exact `JavaRandom`. The library has no dependencies. |
| `shimeji-core` | Pack model, compiler and the behavior engine. |
| `shimeji-packs` | Pack loading, safe import and export. Pure-Rust decoders only. |
| `shimeji-app` | Settings, character library, window rules, mischief journal, agent state, fixed-step clock. |
| `shimeji-macos` | The app itself: AppKit windows, menus, settings, input. The only crate that touches Apple frameworks. |
| `shimeji-proto` | The versioned local-socket messages. |
| `shimeji-hook` | The small program coding agents run on each event. |
| `shimeji-forge` | The character-generation CLI. |

Every crate except `shimeji-macos` is portable Rust that builds and tests on any OS. Dependencies point one way: `expr → core → packs → app → macos`.

The engine counts time in 40 ms ticks (25 per second). The macOS shell only supplies timestamps. The fixed-step clock in `shimeji-app` turns them into ticks, so it is tested without AppKit.

The full rules are in [`AGENTS.md`](AGENTS.md). In short:

- Never commit third-party pack art or corpus data.
- Keep the bundled shime byte-identical. `cargo test` checks its 46 PNGs and two XML files against `assets/packs/shime/CHECKSUMS.sha256`.
- When engine behavior is in question, the pinned reference source wins. Fetch it into `tools/ee-reference/vendor/` with `tools/ee-reference/fetch.sh`.

## License and credits

shimeji2 is released under the [zlib/libpng license](LICENSE). It is an **altered version** of Shimeji, not the original software. Please don't present it as Shimeji, Shimeji for Mac or shimeji4mac.

- Shimeji © 2009–2011 group-finity (Yuki Yamada)
- Shimeji for Mac © 2011–2012 Yusaku Hashimoto
- shimeji4mac © 2016 AlanJager
- Behavior reference: [Shimeji-Desktop](https://github.com/DalekCraft2/Shimeji-Desktop), based on Shimeji-ee (© Shimeji-ee Group; Kilkakon). Its license is in [`tools/ee-reference/UPSTREAM-LICENSE.txt`](tools/ee-reference/UPSTREAM-LICENSE.txt).

Other notices:

- **The bundled shime.** It is the upstream art, unchanged. The upstream license texts cover "this software" without saying whether the drawings are included. [`assets/packs/shime/ATTRIBUTION.md`](assets/packs/shime/ATTRIBUTION.md) records that uncertainty.
- **The app icon.** It is original artwork made with AI assistance for this project; see [`packaging/icons/README.md`](packaging/icons/README.md).
- **Rust packages.** Notices for the 65 Rust packages used by the app and the hook are in [`packaging/THIRD-PARTY-NOTICES.md`](packaging/THIRD-PARTY-NOTICES.md).

See also [`NOTICE.md`](NOTICE.md).

## Feedback

Please open an issue on [GitHub](https://github.com/AlanJager/shimeji2/issues) and include:

- the release you downloaded (for example v0.1.0-beta.1; Finder's **Get Info** shows 0.1.0)
- your Mac chip and macOS version
- where the character pack came from
- the steps that reproduce the problem
- whether it happened on the desktop or over an app window

A screenshot or screen recording helps. Leave out personal configuration files and private window contents.
