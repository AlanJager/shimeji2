#!/usr/bin/env python3
"""Run the shime scenarios against every corpus pack the reference can load.

Writes corpus/traces/<id>/<scenario>.trace (gitignored) and prints a summary.
A pack counts as deterministic when every scenario exits 0 twice and the two
traces match byte for byte, including a trace that records an abort line.
"""

import json
import os
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PACKS = ROOT / "corpus" / "packs"
TRACES = ROOT / "corpus" / "traces"
SCENARIO_DIR = ROOT / "tests" / "scenarios"
STAGE = Path("/tmp/shimeji2-stage")
TIMEOUT_S = 180


def scenarios():
    names = sorted(path.stem for path in SCENARIO_DIR.glob("shime-*.json"))
    return names


def rewrite(scenario_name, image_set, dest):
    data = json.loads((SCENARIO_DIR / f"{scenario_name}.json").read_text(encoding="utf-8"))
    data["imageSet"] = image_set
    dest.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def run_java(java, workdir, pin, classpath, scenario, out):
    cmd = [
        java,
        "-Dshimeji2.ref.factory=headless",
        f"-Dshimeji2.ref.pin={pin}",
        "-Dfile.encoding=UTF-8",
        "-cp",
        classpath,
        "shimeji2.ref.TraceRunner",
        "--scenario",
        str(scenario),
        "--out",
        str(out),
    ]
    try:
        completed = subprocess.run(
            cmd,
            cwd=workdir,
            timeout=TIMEOUT_S,
            capture_output=True,
            text=True,
        )
    except subprocess.TimeoutExpired:
        return 124, "", "timeout"
    err = completed.stderr or ""
    interesting = [
        line
        for line in err.splitlines()
        if "Exception" in line or line.startswith("abort ") or "ERROR" in line or "Failed" in line
    ]
    tail = "\n".join(interesting[-8:] or err.splitlines()[-8:])
    return completed.returncode, completed.stdout, tail


def one_pack(pack_id, java, pin, classpath, names):
    pack = PACKS / pack_id
    dest = STAGE / pack_id
    trace_dir = TRACES / pack_id
    trace_dir.mkdir(parents=True, exist_ok=True)
    stage = subprocess.run(
        [sys.executable, str(HERE / "stage-pack.py"), "--pack", str(pack), "--dest", str(dest)],
        capture_output=True,
        text=True,
    )
    if stage.returncode != 0:
        reason = (stage.stderr or stage.stdout or "stage failed").strip().splitlines()[-1]
        return {"id": pack_id, "result": "stage-fail", "reason": reason, "scenarios": {}}

    info = {}
    for line in stage.stdout.splitlines():
        if "\t" in line:
            key, value = line.split("\t", 1)
            info[key] = value
    image_set = info.get("image-set", "")
    layout = info.get("layout", "")
    results = {}
    worst = "deterministic"
    for name in names:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            scenario = tmp_path / f"{name}.json"
            rewrite(name, image_set, scenario)
            first = tmp_path / "first.trace"
            second = tmp_path / "second.trace"
            code1, _, err1 = run_java(java, dest, pin, classpath, scenario, first)
            if code1 != 0:
                results[name] = f"run-fail {code1}: {err1}"
                worst = "run-fail"
                continue
            code2, _, err2 = run_java(java, dest, pin, classpath, scenario, second)
            if code2 != 0:
                results[name] = f"run-fail {code2}: {err2}"
                worst = "run-fail"
                continue
            same = first.read_bytes() == second.read_bytes()
            if not same:
                results[name] = "not-identical"
                if worst == "deterministic":
                    worst = "not-identical"
                continue
            target = trace_dir / f"{name}.trace"
            target.write_bytes(first.read_bytes())
            abort = any(line.startswith("# abort\t") for line in first.read_text(encoding="utf-8").splitlines())
            results[name] = "abort" if abort else "ok"
            if abort and worst == "deterministic":
                worst = "deterministic-abort"
    return {
        "id": pack_id,
        "result": worst,
        "layout": layout,
        "imageSet": image_set,
        "scenarios": results,
    }


def main():
    pin = (HERE / "PIN").read_text(encoding="utf-8").strip()
    classpath = (HERE / "build" / "runtime-classpath.txt").read_text(encoding="utf-8").strip()
    if not classpath:
        print("classpath is empty; run gradle printClasspath first", file=sys.stderr)
        return 1
    java = os.environ.get("JAVA", "java")
    names = scenarios()
    if not PACKS.is_dir():
        print(f"corpus packs missing: {PACKS}", file=sys.stderr)
        return 1
    pack_ids = sorted(path.name for path in PACKS.iterdir() if path.is_dir())
    jobs = int(os.environ.get("JOBS", "3"))
    TRACES.mkdir(parents=True, exist_ok=True)
    STAGE.mkdir(parents=True, exist_ok=True)
    rows = []
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = [pool.submit(one_pack, pack_id, java, pin, classpath, names) for pack_id in pack_ids]
        for future in as_completed(futures):
            row = future.result()
            rows.append(row)
            print(f"{row['result']}\t{row['id']}", flush=True)
    rows.sort(key=lambda row: row["id"])
    summary = TRACES / "summary.json"
    summary.write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")
    counts = {}
    for row in rows:
        counts[row["result"]] = counts.get(row["result"], 0) + 1
    print("COUNTS " + json.dumps(counts, sort_keys=True))
    print(f"scenarios {len(names)} packs {len(rows)} summary {summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
