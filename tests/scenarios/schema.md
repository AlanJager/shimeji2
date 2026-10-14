# Scenario and trace schema

The reference harness (`tools/ee-reference`) and the Rust runner share this format.
Header lines and unknown `#` lines are metadata. Data lines are what a parser must read.

## Scenario JSON

UTF-8 JSON object. Numbers are integers. There is no interpolation of booleans.

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Scenario name. Copied into the trace header. |
| `seed` | yes | Seed for the single `java.util.Random` stream. |
| `ticks` | yes | Tick count. Positive. The runner may override it with `--ticks` for a smoke run. Goldens use the file value. |
| `imageSet` | no | Image set passed to `createMascot`. Default `shime`. |
| `screens` | yes | Non-empty array of screens. |
| `activeWindow` | no | Keyframes of the active window. Empty or omitted means no window. |
| `cursor` | no | Keyframes of the cursor. Empty or omitted means `(0, 0)` for every tick. |
| `mouse` | no | Events injected at tick boundaries. |

A screen:

| Field | Meaning |
| --- | --- |
| `id` | Stable id. Also the map key passed to `ComplexArea.set`. |
| `x`, `y`, `width`, `height` | Screen rectangle. |
| `work` | Optional `{x, y, width, height}`. Omitted means the work area equals the screen. |

An `activeWindow` keyframe:

| Field | Meaning |
| --- | --- |
| `tick` | Tick at which this sample applies. |
| `visible` | Default `true`. |
| `x`, `y`, `width`, `height` | Required when `visible` is true. |
| `title` | Optional window title. |

A `cursor` keyframe: `tick`, `x`, `y`.

A `mouse` event:

| Field | Meaning |
| --- | --- |
| `tick` | Injected before this tick's environment update. |
| `type` | `press` or `release`. |
| `button` | `left` or `right`. `right` is a popup trigger. |
| `x`, `y` | Optional event coordinates. Default `0`. Drag physics uses the cursor timeline, not these coordinates. |
| `mascot` | Optional id (`mascot1`). Omitted targets the first live mascot. |

Events whose target is not yet in the manager's live list are dropped. A press on tick 0, before the spawned mascot is admitted, does nothing.

### Sampling

Keyframes are sorted by `tick`. At an exact keyframe tick, that frame is used. Before the first frame, the first frame is used. After the last, the last is held.

Between two frames, integer coordinates use

```
start + (end - start) * (tick - tickStart) / (tickEnd - tickStart)
```

with the multiplication in 64-bit arithmetic before the division. `visible` does not blend: if the two frames disagree, the previous frame is held until the next keyframe tick.

The cursor's `dx` and `dy` are not stored. The harness places the tick-0 cursor with raw coordinates and zero velocity, then every later sample goes through `Location.set`, which smooths with integer `(oldDelta + newPosition - oldPosition) / 2`.

Mouse events for a tick are delivered before `Environment.tick` for that tick.

## Trace

UTF-8, LF line endings, no BOM. One file per scenario.

### Header

Lines start with `#`. A parser skips them. The harness writes:

```
# scenario\tNAME
# seed\tSEED
# ticks\tTICKS
# engine\tDalekCraft2/Shimeji-Desktop
# commit\tCOMMIT
# script-engine\tCLASS
# nashorn\tVERSION
# jdk\tVERSION
# rng-counter\tshared-after-tick
```

`CLASS` is the Nashorn engine class actually used. This reference is Nashorn, not Rhino.

### Tick lines

One `T` line per live mascot after each completed `Manager.tick`, in manager-list order. Newborns queued during the tick appear on the next tick, which is when the engine admits them.

```
T\tTICK\tMASCOT\tBEHAVIOR\tACTION\tANCHOR_X\tANCHOR_Y\tIMAGE\tLOOK_RIGHT\tRNG
```

| Column | Meaning |
| --- | --- |
| `TICK` | 0-based index of the tick that just finished. |
| `MASCOT` | `Mascot.toString()`, for example `mascot1`. |
| `BEHAVIOR` | `UserBehavior.getName()`. The plain behavior name, not an evaluated script. |
| `ACTION` | The `名前` attribute of the innermost current action when that attribute is a constant. Scripted names are not evaluated (that would consume RNG). The fallback is the action's simple class name. |
| `ANCHOR_X`, `ANCHOR_Y` | Anchor after the tick's `apply`. |
| `IMAGE` | XML image path, such as `/shime1.png`. `-` when no pose has been applied. |
| `LOOK_RIGHT` | `true` or `false`. |
| `RNG` | Cumulative `nextDouble` draws from the shared stream after the whole tick (environment, every mascot `tick`, every `apply`). Every `T` line of one tick carries the same value. |

Text fields escape `\`, tab, LF, and CR as `\\`, `\t`, `\n`, `\r`. A null field is `-`.

### Verbose evaluation lines

`--verbose` inserts `E` lines as evaluations happen, before the `T` lines of that tick. Goldens are produced without it.

```
E\tTICK\tMASCOT\tSOURCE\tRESULT
```

`SOURCE` is the script source. `RESULT` is `null`, `undefined`, `true`/`false`, a number's `toString`, a quoted string, a mascot's `toString`, or `obj:` plus the runtime class name. Identity hashes are not used. `MASCOT` is the `mascot` binding when it is a constant, otherwise `-`.

The same escape rules apply. `SOURCE` and `RESULT` are escaped, so a line still has five tab-separated fields.

### Abort line

If an exception escapes `Manager.tick`, the harness writes one metadata line and stops. The process still exits 0: the exception is the oracle for that scenario. Ticks before the abort have their `T` lines. The abort tick does not.

```
# abort\tTICK\tCLASS\tMESSAGE
```

`CLASS` is the exception class name. `MESSAGE` is `getMessage()`, or `-` when that is null or empty. Both are escaped with the same rules as trace fields. A run that finishes every tick has no abort line.
