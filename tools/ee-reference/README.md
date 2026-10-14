# ee-reference

Headless driver for DalekCraft2/Shimeji-Desktop, pinned in `PIN`. The checkout is `vendor/` (gitignored). Traces follow `tests/scenarios/schema.md`.

Java 21 and Gradle are needed only to regenerate reference traces, not to build
or run the Rust app. The committed goldens are used by ordinary Cargo tests.
Reference generation can consume substantial memory; run it separately from the
app. Shell scripts inherit proxy environment variables. Configure any Gradle
proxy in your user Gradle settings; the repository contains no private proxy.
`fetch.sh` uses process-local HTTP/1.1 for compatibility. `jfa` is excluded because
JNA 5.19.1 does not publish its requested `jpms` classifier.

```
./fetch.sh
./run.sh --scenario /absolute/tests/scenarios/shime-floor-only.json --out /tmp/floor.trace
./run.sh --pack /absolute/tests/fixtures/ee-synthetic/ee-layout-root --scenario /absolute/tests/scenarios/ee-layout-root.json --out /tmp/root.trace
./run-goldens.sh
ONLY=ee-layout-root ./run-goldens.sh
./run-corpus.sh
```

`run-goldens.sh` runs every shime scenario and every `ee-*` synthetic scenario twice and copies a golden only when the two traces match. `ONLY` is a comma-separated list of scenario names. `run-corpus.sh` writes gitignored traces to `corpus/traces/<pack>/<scenario>.trace`. Regeneration, the twice-and-compare check, and what each golden pins are in `docs/testing/golden-traces.md`.

`--ticks N` truncates a run. `--verbose` adds `E` lines. Goldens are non-verbose.

The process calls `System.exit` when the trace is flushed. Swing starts a non-daemon event thread, and a normal return would leave the JVM running.

`Math.random` in pack scripts is rewritten to `__shimeji2Random` at compile time. Nashorn compiles the identifier `Math.random` into a call to `java.lang.Math.random`, so assigning `Math.random` on the global does not affect compiled scripts. The replacement function lives in the script's variable map. A bare `Math.random` (no call) converts to `NaN` and does not draw, which is what Nashorn's builtin does. `動作.xml` uses that form on the look-left branch of two walk targets (lines 443 and 533).

`./dump-vectors.sh` writes `crates/shimeji-expr/tests/vectors/java_random.json` and `nashorn.json`. Random sequences come from `java.util.Random`. Expression cases are compiled with the same `Math.random` rewrite, against a fixed stub `mascot` and seed 42. The expression list is the hand-written coercion cases plus the bundled shime XML. Corpus expressions are not dumped.
