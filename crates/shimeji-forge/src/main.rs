//! `forge extract|place|qa|derive|review|provenance|install|agent-jobs`.
//!
//! Agent mode writes `jobs.json` and can wait for image files. It does not
//! open a socket. OpenAI and Gemini are not called.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use shimeji_forge::jobs::CharacterSheet;
use shimeji_forge::provider::AgentMode;
use shimeji_forge::qa::{QaFrame, QaInput, QaReport};
use shimeji_forge::templates::{self};
use shimeji_forge::{
    CHARACTER_SHEETS, SheetFrame, decide_export, extract_grid, group, linked_crates,
    load_templates, measure_dir, place, qa, save_png,
};

fn main() -> ExitCode {
    let (packs, proto) = linked_crates();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{}", usage(packs, proto));
        return ExitCode::from(2);
    }
    let cmd = args.remove(0);
    let result = match cmd.as_str() {
        "extract" => cmd_extract(&args),
        "place" => cmd_place(&args),
        "qa" => cmd_qa(&args),
        "derive" => cmd_derive(&args),
        "review" => cmd_review(&args),
        "provenance" => cmd_provenance(&args),
        "install" => cmd_install(&args),
        "agent-jobs" => cmd_agent(&args),
        "templates" => cmd_templates(&args),
        "guides" => cmd_guides(&args),
        "help" | "--help" | "-h" => {
            println!("{}", usage(packs, proto));
            Ok(0)
        }
        other => Err(format!("unknown command {other}\n{}", usage(packs, proto))),
    };
    match result {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("forge: {err}");
            ExitCode::from(2)
        }
    }
}

fn usage(packs: &str, proto: &str) -> String {
    format!(
        "\
forge — pose extraction, placement, and QA ({packs}, {proto})

  forge extract <sheet> --group G2 --out <dir>
  forge extract <sheet> --slots N [--rows R] --out <dir>
  forge place --sheet <sheet> --group G2 --out <dir>
  forge qa --sheet <sheet> (--group G2 | --slots N) --out <dir> [--override]
  forge qa --raw <dir> --out <dir>
  forge derive <pack-dir>
  forge review <pack-dir> --out <dir> [--open]
  forge provenance --out <file> --model MODEL [--date YYYY-MM-DD] [--override] [--prompt TEXT] [--share-prompt]
  forge install <pack-dir>
  forge agent-jobs --out <dir> [--character NAME] [--wait SECONDS]
  forge templates --out <file>
  forge guides --out <dir>"
    )
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn positional(args: &[String]) -> Option<&str> {
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
}

fn cmd_extract(args: &[String]) -> Result<u8, String> {
    let sheet = positional(args).ok_or("extract needs a sheet path")?;
    let out = flag(args, "--out").ok_or("--out is required")?;
    let (rows, cols, _frames) = slots_of(args)?;
    let image = shimeji_forge::decode_png(Path::new(sheet))?;
    let extracted = extract_grid(&image, rows, cols)?;
    let dir = Path::new(out);
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    for frame in &extracted.frames {
        if frame.image.opaque_count() == 0 {
            continue;
        }
        let path = dir.join(format!("slot-{:02}.png", frame.slot + 1));
        save_png(&path, &frame.image)?;
    }
    let key = extracted.key_rgb();
    let body = serde_json::json!({
        "key": key,
        "fringe_ratio": extracted.fringe_ratio,
        "slots": extracted.frames.len(),
        "nonempty": extracted.frames.iter().filter(|f| f.image.opaque_count() > 0).count(),
    });
    fs::write(
        dir.join("extract.json"),
        serde_json::to_string_pretty(&body).unwrap(),
    )
    .map_err(|err| err.to_string())?;
    println!(
        "extracted {} slots, key #{:02X}{:02X}{:02X}, fringe {:.4}",
        extracted.frames.len(),
        key[0],
        key[1],
        key[2],
        extracted.fringe_ratio
    );
    Ok(0)
}

fn cmd_place(args: &[String]) -> Result<u8, String> {
    let sheet = flag(args, "--sheet").ok_or("--sheet is required")?;
    let out = flag(args, "--out").ok_or("--out is required")?;
    let (rows, cols, indices) = slots_of(args)?;
    let image = shimeji_forge::decode_png(Path::new(sheet))?;
    let extracted = extract_grid(&image, rows, cols)?;
    let templates = load_templates(&templates::templates_path())?;
    let mut sheet_frames = Vec::new();
    for frame in &extracted.frames {
        let Some(index) = indices.get(frame.slot).copied().flatten() else {
            continue;
        };
        if !(1..=46).contains(&index) {
            continue;
        }
        if frame.image.opaque_count() == 0 {
            continue;
        }
        sheet_frames.push(SheetFrame {
            index,
            image: frame.image.clone(),
        });
    }
    if sheet_frames.is_empty() {
        return Err("no classic frames to place".to_string());
    }
    let placed = place(&sheet_frames, &templates)?;
    let dir = Path::new(out);
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    for frame in &placed.frames {
        save_png(&dir.join(format!("shime{}.png", frame.index)), &frame.image)?;
    }
    let body = serde_json::json!({
        "scale": placed.scale,
        "method": placed.method.as_str(),
        "reference": placed.reference,
        "warnings": placed.warnings,
    });
    fs::write(
        dir.join("place.json"),
        serde_json::to_string_pretty(&body).unwrap(),
    )
    .map_err(|err| err.to_string())?;
    println!(
        "placed {} frames, scale {:.4} ({}), reference {}",
        placed.frames.len(),
        placed.scale,
        placed.method.as_str(),
        placed.reference
    );
    Ok(0)
}

fn cmd_qa(args: &[String]) -> Result<u8, String> {
    let out = flag(args, "--out").ok_or("--out is required")?;
    if let Some(raw) = flag(args, "--raw") {
        return qa_raw(Path::new(raw), Path::new(out));
    }
    let sheet = flag(args, "--sheet")
        .or_else(|| positional(args))
        .ok_or("qa needs --sheet or --raw")?;
    let override_flag = has(args, "--override");
    let (rows, cols, indices) = slots_of(args)?;
    let report_path = qa_one(
        Path::new(sheet),
        rows,
        cols,
        &indices,
        Path::new(out),
        None,
        override_flag,
    )?;
    let text = fs::read_to_string(&report_path).map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    print_gate(&value)
}

fn print_gate(value: &serde_json::Value) -> Result<u8, String> {
    let allowed = value
        .pointer("/export/allowed")
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    let failed = value
        .pointer("/export/failed")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let recorded = value
        .pointer("/export/override_recorded")
        .is_some_and(|item| !item.is_null());
    if allowed && recorded {
        println!("qa failed ({failed}); override recorded");
        Ok(0)
    } else if allowed {
        println!("qa passed");
        Ok(0)
    } else {
        println!("qa failed ({failed}); export blocked");
        Ok(1)
    }
}

fn qa_raw(raw: &Path, out: &Path) -> Result<u8, String> {
    fs::create_dir_all(out).map_err(|err| err.to_string())?;
    let mut summary = Vec::new();
    for spec in CHARACTER_SHEETS {
        let path = raw.join(spec.file);
        if !path.is_file() {
            summary.push(serde_json::json!({
                "file": spec.file,
                "error": "missing",
            }));
            continue;
        }
        let indices = sheet_indices(spec);
        match qa_one(
            &path,
            spec.rows as usize,
            spec.cols as usize,
            &indices,
            out,
            Some(spec.file),
            false,
        ) {
            Ok(report) => {
                let text = fs::read_to_string(&report).unwrap_or_default();
                let value: serde_json::Value =
                    serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
                summary.push(serde_json::json!({
                    "file": spec.file,
                    "passed": value.pointer("/extract/passed"),
                    "failed": value.pointer("/extract/failed"),
                    "key": value.get("key"),
                    "fringe_ratio": value.pointer("/extract/fringe_ratio"),
                    "residue_ratio": value.pointer("/extract/residue_ratio"),
                    "empty_slots": value.pointer("/extract/empty_slots"),
                    "size_outliers": value.pointer("/extract/size_outliers"),
                    "duplicate_pairs": value.pointer("/extract/duplicate_pairs"),
                    "scale_method": value.pointer("/placement/method"),
                    "scale": value.pointer("/placement/scale"),
                    "baseline_errors": value.pointer("/placement/baseline_errors"),
                    "export_allowed": value.pointer("/export/allowed"),
                    "export_failed": value.pointer("/export/failed"),
                }));
                println!(
                    "{} {}",
                    spec.file,
                    value
                        .pointer("/export/failed")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "ok".to_string())
                );
            }
            Err(err) => {
                summary.push(serde_json::json!({"file": spec.file, "error": err}));
                println!("{} error: {err}", spec.file);
            }
        }
    }
    fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .map_err(|err| err.to_string())?;
    Ok(0)
}

fn qa_one(
    sheet: &Path,
    rows: usize,
    cols: usize,
    indices: &[Option<u16>],
    out: &Path,
    name: Option<&str>,
    override_flag: bool,
) -> Result<PathBuf, String> {
    let image = shimeji_forge::decode_png(sheet)?;
    let extracted = extract_grid(&image, rows, cols)?;
    let frames = extracted
        .frames
        .iter()
        .map(|frame| {
            let cell = &extracted.cells[frame.slot];
            let index = indices.get(frame.slot).copied().flatten();
            QaFrame {
                slot: frame.slot,
                index,
                image: frame.image.clone(),
                column_x0: cell.x,
                column_y0: cell.y,
                column_x1: cell.x + cell.width,
                column_y1: cell.y + cell.height,
                origin_x: frame.origin_x,
                origin_y: frame.origin_y,
                pad_x: 0,
                pad_y: 0,
            }
        })
        .collect();
    let report = qa(&QaInput {
        key: extracted.key_rgb(),
        expected_slots: rows * cols,
        frames,
        templates: None,
    });
    let templates = load_templates(&templates::templates_path()).ok();
    let placement = if let Some(templates) = templates {
        let mut sheet_frames = Vec::new();
        for frame in &extracted.frames {
            let Some(index) = indices.get(frame.slot).copied().flatten() else {
                continue;
            };
            if !(1..=46).contains(&index) || frame.image.opaque_count() == 0 {
                continue;
            }
            sheet_frames.push(SheetFrame {
                index,
                image: frame.image.clone(),
            });
        }
        if sheet_frames.is_empty() {
            None
        } else {
            match place(&sheet_frames, &templates) {
                Ok(placed) => {
                    let placed_frames = placed
                        .frames
                        .iter()
                        .enumerate()
                        .map(|(slot, frame)| {
                            let mut placed_frame =
                                QaFrame::lone(slot, Some(frame.index), frame.image.clone());
                            if let Some(template) = templates.get(frame.index) {
                                placed_frame.pad_x = frame.anchor_x - template.anchor_x;
                                placed_frame.pad_y = frame.anchor_y - template.anchor_y;
                            }
                            placed_frame
                        })
                        .collect();
                    let placed_qa = qa(&QaInput {
                        key: extracted.key_rgb(),
                        expected_slots: placed.frames.len(),
                        frames: placed_frames,
                        templates: Some(templates),
                    });
                    Some(serde_json::json!({
                        "scale": placed.scale,
                        "method": placed.method.as_str(),
                        "reference": placed.reference,
                        "warnings": placed.warnings,
                        "baseline_pass": placed_qa.baseline_pass,
                        "baseline_errors": placed_qa.baseline_errors,
                        "heights": placed.frames.iter().map(|frame| {
                            serde_json::json!({
                                "index": frame.index,
                                "height": frame.content_height,
                                "anchor": [frame.anchor_x, frame.anchor_y],
                                "feet": frame.image.feet_y(),
                                "top": frame.image.top_y(),
                            })
                        }).collect::<Vec<_>>(),
                    }))
                }
                Err(err) => Some(serde_json::json!({"error": err})),
            }
        }
    } else {
        None
    };
    let baseline_failed = placement
        .as_ref()
        .and_then(|item| item.get("baseline_pass"))
        .and_then(|item| item.as_bool())
        == Some(false);
    let gate = gate_report(&report, baseline_failed);
    let decision = decide_export(&gate, override_flag);
    let file_name = name
        .map(|n| n.trim_end_matches(".png").to_string())
        .unwrap_or_else(|| {
            sheet
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "qa".to_string())
        });
    fs::create_dir_all(out).map_err(|err| err.to_string())?;
    let path = out.join(format!("{file_name}.json"));
    let body = serde_json::json!({
        "file": sheet,
        "rows": rows,
        "cols": cols,
        "key": extracted.key_rgb(),
        "key_kind": match extracted.key {
            shimeji_forge::KeyKind::NativeAlpha => "alpha",
            shimeji_forge::KeyKind::Chroma(_) => "chroma",
        },
        "extract": report,
        "placement": placement,
        "export": {
            "allowed": decision.allowed,
            "failed": gate.failed,
            "override_recorded": decision.override_recorded,
        },
    });
    fs::write(&path, serde_json::to_string_pretty(&body).unwrap())
        .map_err(|err| err.to_string())?;
    Ok(path)
}

/// Baseline is measured after placement. Fold that result into the extract
/// report so one `decide_export` call sees every blocking check.
fn gate_report(extract: &QaReport, baseline_failed: bool) -> QaReport {
    let mut report = extract.clone();
    if baseline_failed && !report.failed.iter().any(|name| name == "baseline") {
        report.baseline_pass = false;
        report.failed.push("baseline".to_string());
        report.passed = false;
    }
    report
}

fn sheet_indices(spec: &CharacterSheet) -> Vec<Option<u16>> {
    let n = (spec.rows as usize) * (spec.cols as usize);
    (0..n)
        .map(|i| spec.frames.get(i).copied().filter(|index| *index > 0))
        .collect()
}

fn slots_of(args: &[String]) -> Result<(usize, usize, Vec<Option<u16>>), String> {
    if let Some(id) = flag(args, "--group") {
        let group = group(id).ok_or_else(|| format!("unknown group {id}"))?;
        let indices = group.frames.iter().copied().map(Some).collect();
        return Ok((1, group.frames.len(), indices));
    }
    if let Some(slots) = flag(args, "--slots") {
        let cols: usize = slots.parse().map_err(|_| format!("bad --slots {slots}"))?;
        let rows: usize = flag(args, "--rows")
            .map(|s| s.parse().map_err(|_| format!("bad --rows {s}")))
            .transpose()?
            .unwrap_or(1);
        let indices = vec![None; rows * cols];
        return Ok((rows, cols, indices));
    }
    Err("--group or --slots is required".to_string())
}

fn cmd_agent(args: &[String]) -> Result<u8, String> {
    let out = flag(args, "--out").ok_or("--out is required")?;
    let character = flag(args, "--character").unwrap_or("character");
    let wait: u64 = flag(args, "--wait")
        .map(|s| s.parse().map_err(|_| format!("bad --wait {s}")))
        .transpose()?
        .unwrap_or(0);
    let mode = AgentMode {
        dir: PathBuf::from(out),
        timeout: Duration::from_secs(wait),
    };
    let path = mode.write_jobs(character)?;
    println!("wrote {}", path.display());
    if has(args, "--wait") {
        mode.wait_for_outputs()?;
        println!("outputs are in");
    }
    Ok(0)
}

fn cmd_templates(args: &[String]) -> Result<u8, String> {
    let out = flag(args, "--out").ok_or("--out is required")?;
    let set = measure_dir(&templates::bundled_shime_img())?;
    let text = templates::render_toml(&set);
    if let Some(parent) = Path::new(out).parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(out, text).map_err(|err| err.to_string())?;
    println!("measured 46 templates into {out}");
    Ok(0)
}

fn cmd_guides(args: &[String]) -> Result<u8, String> {
    let out = flag(args, "--out").ok_or("--out is required")?;
    templates::write_pose_guides(&templates::bundled_shime_img(), Path::new(out))?;
    println!("wrote 46 pose guides into {out}");
    Ok(0)
}

fn cmd_derive(args: &[String]) -> Result<u8, String> {
    let pack = positional(args).ok_or("derive needs a pack directory")?;
    let report = shimeji_forge::derive_pack(Path::new(pack))?;
    for row in &report.offsets {
        println!("split shime{}.png dx={}", row.frame, row.dx);
    }
    for row in &report.clips {
        println!("clip shime{}.png x<{} y<{}", row.frame, row.x, row.y);
    }
    Ok(0)
}

fn cmd_review(args: &[String]) -> Result<u8, String> {
    let pack = positional(args).ok_or("review needs a pack directory")?;
    let out = flag(args, "--out").ok_or("--out is required")?;
    let written = shimeji_forge::review(Path::new(pack), Path::new(out))?;
    if has(args, "--open") {
        let status = std::process::Command::new("open")
            .arg(&written.sheet)
            .status()
            .map_err(|err| err.to_string())?;
        if !status.success() {
            return Err(format!("open exited with {status}"));
        }
    }
    println!(
        "wrote {} and {} gif{}",
        written.sheet.display(),
        written.gifs.len(),
        if written.gifs.len() == 1 { "" } else { "s" }
    );
    Ok(0)
}

fn cmd_provenance(args: &[String]) -> Result<u8, String> {
    let out = flag(args, "--out").ok_or("--out is required")?;
    let model = flag(args, "--model").ok_or("--model is required")?;
    let date = flag(args, "--date")
        .map(str::to_string)
        .unwrap_or_else(shimeji_forge::today_utc);
    let prompt = if has(args, "--share-prompt") {
        Some(flag(args, "--prompt").unwrap_or("").to_string())
    } else {
        None
    };
    let spec = shimeji_forge::Provenance {
        model: model.to_string(),
        date,
        forge_version: env!("CARGO_PKG_VERSION").to_string(),
        override_used: has(args, "--override"),
        prompt,
    };
    shimeji_forge::write_provenance(Path::new(out), &spec)?;
    println!("wrote {out}");
    Ok(0)
}

fn cmd_install(args: &[String]) -> Result<u8, String> {
    let pack = positional(args).ok_or("install needs a pack directory")?;
    let report = match shimeji_proto::home_dir() {
        Ok(home) => shimeji_forge::install_with_home(Path::new(pack), &home),
        Err(_) => {
            shimeji_forge::install_with_home(Path::new(pack), Path::new("/no-such-shimeji-home"))
        }
    };
    if report.exit == 0 {
        println!("{}", report.message);
        Ok(0)
    } else {
        Err(report.message)
    }
}
