# Golden traces

Reference traces for the headless Shimeji-Desktop harness in `tools/ee-reference`. The trace format, including the abort line, is `tests/scenarios/schema.md`. Goldens are non-verbose: header lines, `T` lines, and at most one `# abort` line. Log timestamps on stderr are not part of the oracle.

## Regenerate

From the repo root, with Java21 and Gradle available. Configure a proxy in your environment and user Gradle settings if required:

```
./tools/ee-reference/run-goldens.sh
```

That prepares the shime run directory, stages each synthetic pack, and runs every scenario below. `ONLY=name1,name2` limits the set:

```
ONLY=ee-layout-root,shime-window-drag-up ./tools/ee-reference/run-goldens.sh
```

Shime scenarios read `assets/packs/shime` through `run/shime`. Synthetic scenarios read `tests/fixtures/ee-synthetic/<name>`. The output paths are `tests/golden/shime/<name>.trace` and `tests/golden/ee-synthetic/<name>.trace`.

A single scenario, without the twice-and-compare check:

```
./tools/ee-reference/run.sh --scenario /absolute/path/tests/scenarios/shime-floor-only.json --out /tmp/floor.trace
./tools/ee-reference/run.sh --pack tests/fixtures/ee-synthetic/ee-layout-root --scenario /absolute/path/tests/scenarios/ee-layout-root.json --out /tmp/root.trace
```

`--ticks N` truncates a run. `--verbose` adds `E` lines. The working directory of the JVM must be a staged pack that contains `conf/logging.properties`.

## Twice-and-compare

`run-goldens.sh` runs each scenario twice into temporary files. `cmp -s` must succeed before the script copies either file onto the golden. A mismatch prints `NOT IDENTICAL` and exits 1, and the previous golden is left in place. A match prints `IDENTICAL <name> -> tests/golden/...`.

The same check is what `run-corpus.sh` uses before it writes a corpus trace.

The harness calls `System.exit` after flushing the trace. An exception that escapes `Manager.tick` is written as an abort line and the process still exits 0, so the twice-and-compare check covers that oracle. An exception during configuration load exits 1 and fails the check.

## Staging

The Java engine reads `conf/` plus `img/<imageSet>/`. Three fixture layouts stay on disk in the shapes real packs use, and `tools/ee-reference/stage-pack.py` symlinks them into that working directory. It does not copy art and it does not modify the pack.

| On-disk layout | Fixture | Scenario `imageSet` | Staged images |
| --- | --- | --- | --- |
| `actions.xml` at the pack root, PNGs in `img/` | `ee-layout-root` | `dot` | `img/dot/` |
| PNGs at the pack root, XML in `conf/` | `ee-layout-dropped` | `dot` | `img/dot/` |
| XML in `conf/`, PNGs in `img/<Set>/` | `ee-layout-set` | `Breed` | `img/Breed/` |

`run-corpus.sh` uses the same stager. For a corpus pack whose `img/` has one image-set subdirectory, that directory name is the image set. Otherwise the image set name is the pack id. The scenario JSON is rewritten only in a temp copy, so `imageSet` in `tests/scenarios/` stays `shime` for the shime goldens.

## Scenarios

Each line is what the committed golden pins. Seed is 1. Tick count is 3000 unless the mascot is gone or the tick aborts. The header `ticks` field stays 3000 in every file.

- `shime-floor-only` — no window and no input; the mascot lands on the work-area floor at y 745 and walks; `引っこ抜く` later spawns two more mascots.
- `shime-moving-window` — the mascot rides the active window's top edge while that window moves from y 500 to y 260.
- `shime-two-screens` — spawn and landing use the right screen's work bottom (y 740).
- `shime-drag-throw` — press at tick 400, drag through tick 459, release into `投げられる` at tick 460, then fall to y 745.
- `shime-breed-limit` — breeding is on and `maxCount` is 50; the run ends at 3 mascots and never selects `分裂する`. The cap is not reached. `ee-layout-set` is the scenario that reaches a cap.
- `shime-offscreen-teleport` — a drag past the right edge trips the off-screen check; tick 312 teleports to y -256 and starts `落下する`.
- `shime-window-drag-up` — the mascot sits on the window top (`ワークエリアの下辺の右の端っこで座る` / `座る`) from tick 450. The window then moves up by 10 px per tick for 15 ticks (y 500 down to 350), then by 21 px (y 329), then by 81 px (y 248). Anchor y matches the window through the 10 px steps and the 21 px step. On the 81 px step the anchor stays at 329, the behavior becomes `落下する`, and the mascot falls away from the window. `FloorCeiling.move` drops the rider when `newY - location.y > 20` or `newY - location.y < -80` or the horizontal shift is at least 80. In top-left coordinates the 20 px cutoff is the downward direction. An upward step of 21 px still rides. An upward step of 81 px does not.
- `ee-layout-root` — root-xml layout in the English schema. Spawn is off the floor, so the mascot Falls for 55 ticks and Stands at (525, 745) for the rest.
- `ee-layout-dropped` — dropped-set layout. `Dragged` is `Hidden="true"` and still runs for the whole press (ticks 40–119): `Hidden` only affects the context menu. The drag action writes `FootX` and the animation condition `#{FootX < mascot.environment.cursor.x}` selects `/sway.png` while the cursor leads (53 ticks) and `/stand.png` once `FootX` catches the cursor. Release at tick 120 is `Thrown`, then `Stand` on the floor at (660, 745).
- `ee-layout-set` — `conf/` + `img/Breed/` layout. `Multiply` is an embedded `Breed` with `BornX="40"` `BornY="16"` `BornBehavior="Born"`, gated by `mascot.totalCount < maxCount` where `maxCount` is 5. One parent breeds. Children are born at ticks 57, 59, 61, and 63 at (565, 761), the parent's floor anchor (525, 745) plus those offsets while looking left. Population goes 1 → 2 → 3 → 4 → 5 and stays at 5 for 2937 ticks. After the cap the parent's condition fails, it teleports, and it Falls. The four children stay in `Born` at (565, 761) because that action is a `Stay` with no duration.
- `ee-error-condition` — behavior `Boom` has `Condition="#{null.foo}"` and frequency 100, beside `Stand` at frequency 1. Evaluating the condition throws `VariableException` (Nashorn `TypeError` on `null.foo`). The reference logs it, skips that candidate, and selects `Stand`. All 3000 ticks are `Stand` at (-4000, -4000). The wide screen keeps that anchor on-screen, so the off-screen check does not rename the behavior to `Fall`.
- `ee-error-action` — `Stand` lasts 3 ticks, then the next behavior `Bad` has `Duration="${null.foo}"`. The parameter throws while the action initializes. `Mascot.tick` catches it and disposes the mascot. Tick 3 is still present, named `Bad`/`Bad`, because disposal is applied on the next tick. There is no tick 4. The process exits 0 and writes no abort line.
- `ee-error-nonboolean` — after a one-tick `Stand`, the next-list condition `#{1}` is evaluated. The reference casts the result to `Boolean` and the integer 1 throws `ClassCastException` out of `Manager.tick`. The golden has the tick-0 `T` line and `# abort	1	java.lang.ClassCastException	class java.lang.Integer cannot be cast to class java.lang.Boolean (java.lang.Integer and java.lang.Boolean are in module java.base of loader 'bootstrap')`. The process exits 0.

The three error packs are the three sites in the plan's engine-error decision. The condition-throw and action-throw goldens match that decision (false, and dispose). The non-boolean golden records the reference's uncaught cast. stderr for the first two packs contains `java.util.logging` warnings with timestamps; those lines are not in the files.

## Corpus reference traces

`./tools/ee-reference/run-corpus.sh` stages each `corpus/packs/<id>` the reference can load and runs every `tests/scenarios/shime-*.json` twice. A pack is `deterministic` when every scenario exits 0 twice and the files match. `deterministic-abort` means the same, and at least one trace contains an `# abort` line. `not-identical` means the two runs differed. `run-fail` means the JVM exited non-zero or timed out at 180 seconds. `stage-fail` means `stage-pack.py` rejected the pack. Matching traces are written to `corpus/traces/<id>/<scenario>.trace`. `corpus/traces/summary.json` has one row per pack. Both are gitignored.

Job count is `JOBS` (default 3). Each JVM is killed at 180 seconds.

Run on 2026-10-08, JDK 21.0.9, reference `54f71e8f`, 50 packs, 7 scenarios, `JOBS=3`. Elapsed about 281 seconds. Exit 0.

35 packs produced deterministic traces for every scenario: 245 files under `corpus/traces/<id>/`. The two runs matched byte for byte, and none of those traces contains an abort line. Every layout the stager knows is in that set: root-xml 29, dropped-set 3, image-sets 2 (`wispy-touhou-reimu` as `Reimu`, `wispy-vocaloid-miku` as `Miku`), classic 1 (`groupfinity-shime`).

15 packs failed inside the reference while it loaded the configuration. The failure is the same on all 7 scenarios (exit 1). No trace was written. Nothing was nondeterministic, and nothing failed staging.

| Pack | Why the reference rejected it |
| --- | --- |
| `hololive-amelia` | `ClassNotFoundException: com.group_finity.mascot.action.Breeder` |
| `hololive-gura` | `ClassNotFoundException: com.group_finity.mascot.action.AnnoyingJump`. The pack also names `DraggedA`, which this tree does not contain. Load stops at `AnnoyingJump`. |
| `neurolings-neuron` | Duplicate behaviour `NoticeFallingTutel` |
| `shimejis-apex-legends-octane-by-the-kittle-kat` | Duplicate behaviour `SitAndSpinHead` |
| `shimejis-my-hero-academia-katsuki-bakugo-kacchan-by-superevey` | Duplicate behaviour `SitAndSpinHead` |
| `shimejis-doki-doki-literature-club-monika-by-childish-n` | Duplicate behaviour `JumpFromBottomOfIE` |
| `shimejis-megaman-megaman-by-nicola-maria-young` | Duplicate behaviour `PullUpShimeji` |
| `shimejis-bendy-and-the-ink-machine-bendy-by-niuniu-nuko` | `Regist` action `Wiggle` is `Type="Embedded"` and has child actions. The reference says that type does not support child actions. |
| `shimejis-bts-bangtan-boys-jimin-by-wyta-wolf` | Missing pose image `/shime18.png` |
| `shimejis-dream-smp-bad-boy-halo-by-taro-tayo` | Missing pose image `/shime2.png` |
| `shimejis-mario-yoshi-by-cachomon` | Missing pose image `/shimeX.png` |
| `shimejis-pokemon-charmander-by-cachomon` | Missing pose image `/shimeY.png` |
| `shimejis-pokemon-pikachu-by-stormgale` | Missing pose image `/shimeX.png` |
| `shimejis-undertale-sans` | Missing pose image `/shime51.png` |
| `shimejis-vocaloid-ia` | Missing pose image `/shime1-2.png` |

`corpus/traces/summary.json` has the per-scenario message text. It is gitignored with the traces.

## Conformance (Unit 7)

The Rust engine is `shimeji_core::testing::run`. `first_divergence` compares `T` and `# abort` lines. Header lines are ignored. The committed-golden gate is `crates/shimeji-packs/tests/golden.rs`. The corpus gate is `crates/shimeji-packs/tests/corpus_golden.rs` (`#[ignore]`; needs this machine's `corpus/traces/summary.json`).

### Committed goldens

Seed 1, header `ticks` 3000. Twelve traces are byte-identical on every data line. `ee-error-nonboolean` is the documented engine-error exception: the reference aborts the tick, this engine treats a non-Boolean as false and continues. The gate pins tick 0, the abort line, and the engine's tick-1 `T` line so that exception cannot widen.

| Trace | Result | First divergence |
| --- | --- | --- |
| `shime-floor-only` | match | |
| `shime-drag-throw` | match | |
| `shime-breed-limit` | match | |
| `shime-moving-window` | match | |
| `shime-offscreen-teleport` | match | |
| `shime-two-screens` | match | |
| `shime-window-drag-up` | match | |
| `ee-error-action` | match | |
| `ee-error-condition` | match | |
| `ee-error-nonboolean` | exception | tick 1: engine `T	1	mascot1	Fall	Fall	-3254	-256	/body.png	false	3`; golden `# abort	1	java.lang.ClassCastException	… Integer cannot be cast to … Boolean` |
| `ee-layout-dropped` | match | |
| `ee-layout-root` | match | |
| `ee-layout-set` | match | |

Golden runs recorded zero evaluation-budget trips.

### Corpus

Command:

```
cargo test -p shimeji-packs --test corpus_golden -- --ignored --nocapture
```

35 packs in `summary.json` with result `deterministic`. `imageSet` is rewritten the way `tools/ee-reference/run-corpus.py` does: the summary's `imageSet` field, which is the image-set subdirectory when the pack has exactly one, otherwise the pack id. Each pack is loaded with `shimeji_packs::load_dir` and run on the seven `shime-*.json` scenarios.

**MVP-tier** means the loader emitted no `FastFollowClass` diagnostic. Of those packs, the share that is trace-identical on all seven scenarios is **32/33 = 96%** (target ≥ 90%). The image-size fix round reconfirmed the 12 committed goldens above; `ee-error-nonboolean` is still the pinned abort-vs-continue exception.

| Pack | Tier | Result |
| --- | --- | --- |
| `groupfinity-shime` | mvp | identical |
| `hololive-calli` | mvp | diverges at tick 690 (`shime-window-drag-up`): engine `ClimbAlongWall` / `ClimbWall` `(0, 0)` draws 25 / golden `StareAtMouse` / `SitAndLookAtMouse` `(0, 0)` draws 26 |
| `hololive-ina` | mvp | identical |
| `hololive-kiara` | mvp | identical |
| `neurolings-cerber` | mvp | identical |
| `neurolings-eviling` | fast-follow | diverges at tick 299 (`shime-breed-limit`): engine `NoticeFallingTutel` / `RunToThrow` `(279, 570)` / golden `WalkRightAndSit` / `Run` `(319, 570)` |
| `neurolings-tuteling` | fast-follow | diverges at tick 1926 (`shime-breed-limit`): same `GrabWorkAreaBottomLeftWall` / `ClimbWall` `(0, 565)` draws 14; image `/shime12.png` vs `/shime12_l.png` |
| `shimejis-adventure-time-lumpy-space-princess` | mvp | identical |
| `shimejis-attack-on-titan-annie-leonhardt` | mvp | identical |
| `shimejis-bbc-sherlock-john-watson` | mvp | identical |
| `shimejis-black-butler-grell` | mvp | identical |
| `shimejis-bleach-aizen` | mvp | identical |
| `shimejis-blobs-blob-by-reitanna` | mvp | identical |
| `shimejis-creepypasta-jeff-the-killer-by-comickit` | mvp | identical |
| `shimejis-digimon-gatomon` | mvp | identical |
| `shimejis-dragon-ball-z-goku-by-aki-1-iniriki` | mvp | identical |
| `shimejis-durarara-hachimenroppi-orihara-02` | mvp | identical |
| `shimejis-eddsworld-future-edd-by-stanxiety` | mvp | identical |
| `shimejis-fairy-tail-jackal` | mvp | identical |
| `shimejis-five-nights-at-freddys-chica-by-axel-878` | mvp | identical |
| `shimejis-genshin-impact-aether-by-tenkotenmeo` | mvp | identical |
| `shimejis-gravity-falls-mabel-pines-01` | mvp | identical |
| `shimejis-hetalia-belarus-by-uncut-adventure` | mvp | identical |
| `shimejis-homestuck-dave-strider` | mvp | identical |
| `shimejis-hunter-x-hunter-killua` | mvp | identical |
| `shimejis-it-2017-pennywise-by-spider-toast` | mvp | identical |
| `shimejis-kingdom-hearts-sora` | mvp | identical |
| `shimejis-my-chemical-romance-bob-bryar-by-floorsucker` | mvp | identical |
| `shimejis-mystic-messenger-707-by-ember-cl` | mvp | identical |
| `shimejis-pokemon-eevee-by-cachomon` | mvp | identical |
| `shimejis-pusheen-pusheen-the-cat-by-crystal-clear-cc` | mvp | identical |
| `shimejis-sonic-sonic-by-myly14` | mvp | identical |
| `shimejis-vocaloid-hatsune-miku` | mvp | identical |
| `wispy-touhou-reimu` | mvp | identical |
| `wispy-vocaloid-miku` | mvp | identical |

All 35 deterministic packs loaded. 0 load errors. 15 packs the reference rejected still have no traces.

### Triage

The 0×0 off-screen box is fixed. `off_screen` now uses the previous image's IHDR size, the same rectangle as `Mascot.getBounds` plus the test in `UserBehavior.next`. The 22 MVP misses from the first corpus run are gone, including calli's tick-250 drag teleport and the 128 px re-teleport at anchor x ≤ 64. `groupfinity-shime` stays identical, which is the check that the bundled shime golden did not move.

One MVP pack still diverges.

- **`hololive-calli` — engine bug, not patched.** First miss is now tick 690 of `shime-window-drag-up`. Ticks through 689 match, including draw count 24: `Callicopter` / `Jumping` at `(6, 0)`. Tick 690 both land at `(0, 0)` on `/panic2.png` (the jump target snap; the new action has not applied a pose). The reference selects `StareAtMouse` and draws twice (selection, then `SitAndLookAtMouse`'s `Duration="${10+Math.random()*10}"`). The engine selects `ClimbAlongWall` and draws once. With the anchor at `(0, 0)`, the engine's sticky work area is the screen (`0,0,1280×800`), so the wall and ceiling conditions are true (total frequency 700). The 25th `nextDouble` times 700 is 520.79, which lands on `ClimbAlongWall`. `StareAtMouse` is what you get when those borders are off and it is the only frequency>0 candidate (the inset work area's top is y 25, so `(0, 0)` is on neither its ceiling nor its left wall). The promotion happens in `MascotEnvironment::sync`: multiscreen is on for every trace, so each tick reselects, and the tick that starts at `(18, 16)` replaces the inset work area with the screen. `Jump` does not query the work area during the flight. A literal reading of `getWorkArea` would also adopt the screen once it is called at `(0, 0)`, so the promotion was not removed: `shime-two-screens` depends on reselection, and a guess at the cache would not be a one-line port of that method.

- **`neurolings-eviling` — documented exception.** Fast-follow. Tick 299 of `shime-breed-limit` is still `NoticeFallingTutel` / `RunToThrow` against `WalkRightAndSit` / `Run`. `ScanMove` / `Broadcast` remain Stay stand-ins (R6). Outside the MVP denominator.

- **`neurolings-tuteling` — loader difference.** Fast-follow, outside the MVP denominator. The first reported line moved after the off-screen fix. Tick 1926 is the same behavior, anchor, and draw count; the image is `/shime12.png` in the engine and `/shime12_l.png` in the reference. The trace image path is the left/pair key. A `_l` suffix is a left-facing filename the loader does not switch to. The earlier ScanMove/Broadcast miss is no longer the first line this harness reports; the Stay stand-in is unchanged as a known gap.

`getBounds` elsewhere in the vendored tree: `Mascot.apply` only places the window (the shell already sizes panels from decoded images). `Hotspot.contains` flips the click x by `bounds.width` when `lookRight` is set; `UserBehavior.next` in this engine does not scan hotspots, so that width was not added. The remaining hits are platform window and screen rectangles (`WindowsEnvironment`, `X11Environment`, `AbstractEnvironment`), not the mascot.

No MVP divergence was left unexplained. None of the 32 identical packs regressed.

The 15 packs with no reference traces are unchanged: unknown classes (`Breeder`, `AnnoyingJump`), duplicate behaviour names, an `Embedded` action with children, and missing pose images.
