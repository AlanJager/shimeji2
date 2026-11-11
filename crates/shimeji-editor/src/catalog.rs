use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::error::{Result, err};
use crate::imagebuf::{Image, data_url, decode_png, hex};
use crate::layout::Layout;

const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED: u64 = 200 * 1024 * 1024;
const HOSTS: &[&str] = &[
    "codeload.github.com",
    "github.com",
    "raw.githubusercontent.com",
];
const REDIRECT_HOSTS: &[&str] = &[
    "codeload.github.com",
    "github.com",
    "raw.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];

pub fn read_catalog(layout: &Layout) -> Result<Value> {
    let text = std::fs::read_to_string(&layout.catalog).map_err(|error| err(error.to_string()))?;
    let value: Value = serde_json::from_str(&text).map_err(|_| err("素材目录版本无效"))?;
    if value.get("version") != Some(&json!(1)) || !value.get("sources").is_some_and(Value::is_array)
    {
        return Err(err("素材目录版本无效"));
    }
    Ok(value)
}

pub fn open_source(layout: &Layout, ident: &str) -> Result<Value> {
    let catalog = read_catalog(layout)?;
    let source = catalog["sources"]
        .as_array()
        .and_then(|sources| {
            sources
                .iter()
                .find(|source| source.get("id").and_then(Value::as_str) == Some(ident))
        })
        .ok_or_else(|| err("目录中没有这个素材来源"))?;
    let archive = checked_archive(layout, source)?;
    let project = editor_project(source, &archive)?;
    crate::project::validate_project(&layout.shime, &project)?;
    Ok(project)
}

fn checked_archive(layout: &Layout, source: &Value) -> Result<std::path::PathBuf> {
    if !importable(source) {
        return Err(err("这个来源仅提供原站入口，不支持直接导入"));
    }
    let ident = source["id"].as_str().unwrap_or("");
    let sha = source["archiveSha256"].as_str().unwrap_or("");
    if !is_id(ident) || sha.len() != 64 || !sha.chars().all(|char| char.is_ascii_hexdigit()) {
        return Err(err("这个来源仅提供原站入口，不支持直接导入"));
    }
    let url = source["archiveUrl"].as_str().unwrap_or("");
    let host = https_host(url)?;
    if !HOSTS.contains(&host.as_str()) || url.contains('@') {
        return Err(err("下载地址必须是目录内固定的 GitHub HTTPS 地址"));
    }
    components(source["archivePrefix"].as_str().unwrap_or(""), true)?;
    components(source["packPrefix"].as_str().unwrap_or(""), true)?;
    let cache = layout.cache();
    std::fs::create_dir_all(&cache).map_err(|error| err(error.to_string()))?;
    let dest = cache.join(format!("{sha}.zip"));
    if dest.is_file() {
        let bytes = std::fs::read(&dest).map_err(|error| err(error.to_string()))?;
        if bytes.len() as u64 <= MAX_ARCHIVE && hex(&Sha256::digest(&bytes)) == sha {
            return Ok(dest);
        }
    }
    let temporary = cache.join(format!("{sha}.tmp"));
    let output = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "120",
            "--max-redirs",
            "5",
            "--user-agent",
            "shimeji2-local-editor/0.2",
            "-o",
            temporary.to_str().ok_or_else(|| err("缓存路径无效"))?,
            "-w",
            "%{url_effective}",
            url,
        ])
        .output()
        .map_err(|_| err("素材下载需要系统里的 curl。可到原站下载后使用本地导入"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&temporary);
        return Err(err("素材下载超时，可到原站下载后使用本地导入"));
    }
    let effective = String::from_utf8_lossy(&output.stdout);
    let effective_host = https_host(effective.trim()).unwrap_or_default();
    if !REDIRECT_HOSTS.contains(&effective_host.as_str()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(err("下载重定向到了目录以外的来源"));
    }
    let bytes = std::fs::read(&temporary).map_err(|error| err(error.to_string()))?;
    if bytes.len() as u64 > MAX_ARCHIVE {
        let _ = std::fs::remove_file(&temporary);
        return Err(err("素材归档超过 64 MiB"));
    }
    if hex(&Sha256::digest(&bytes)) != sha {
        let _ = std::fs::remove_file(&temporary);
        return Err(err("下载文件的 SHA256 与已核实目录不一致，请刷新来源核验"));
    }
    std::fs::rename(&temporary, &dest).map_err(|error| err(error.to_string()))?;
    Ok(dest)
}

fn editor_project(source: &Value, archive: &Path) -> Result<Value> {
    let bytes = std::fs::read(archive).map_err(|error| err(error.to_string()))?;
    let files = selected_files(&bytes, source)?;
    let normalized: HashMap<String, String> = files
        .keys()
        .map(|name| (name.nfc().collect(), name.clone()))
        .collect();
    let lookup = |path: &str| -> Option<String> {
        let path = path
            .replace('\\', "/")
            .trim_start_matches('/')
            .nfc()
            .collect::<String>();
        if let Some(name) = normalized.get(&path) {
            return Some(name.clone());
        }
        let matches: Vec<_> = normalized
            .iter()
            .filter(|(norm, _)| norm.eq_ignore_ascii_case(&path))
            .map(|(_, name)| name.clone())
            .collect();
        if matches.len() == 1 {
            Some(matches[0].clone())
        } else {
            None
        }
    };
    let config = [
        "conf/actions.xml",
        "conf/動作.xml",
        "actions.xml",
        "動作.xml",
    ]
    .iter()
    .find_map(|name| lookup(name));
    let Some(config) = config else {
        return Err(err(
            "选定目录未找到可读取的经典动作 XML，请使用桌宠的本地导入",
        ));
    };
    let xml = files
        .get(&config)
        .ok_or_else(|| err("选定目录未找到可读取的经典动作 XML，请使用桌宠的本地导入"))?;
    if xml.len() > 2 * 1024 * 1024 {
        return Err(err("动作 XML 超过 2 MiB"));
    }
    let document = std::str::from_utf8(xml).map_err(|_| err("动作 XML 超过 2 MiB"))?;
    let definitions = action_elements(document);
    let mut clips = Vec::new();
    let mut used = HashMap::<String, String>::new();
    let mut images = HashMap::<String, Image>::new();
    let mut image_roots: Vec<String> = files
        .keys()
        .filter(|name| name.to_ascii_lowercase().ends_with(".png") && name.starts_with("img/"))
        .map(|name| {
            name.rsplit_once('/')
                .map(|(root, _)| root.to_string())
                .unwrap_or_default()
        })
        .collect();
    image_roots.sort();
    image_roots.dedup();
    for (name, element) in definitions {
        let Some(animation) = element
            .animations
            .iter()
            .find(|animation| animation.condition.is_none())
            .or_else(|| element.animations.first())
        else {
            continue;
        };
        let mut frames = Vec::new();
        for pose in &animation.poses {
            let relative = pose.image.trim_start_matches('/');
            if relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            {
                continue;
            }
            let mut image_path = None;
            for root in std::iter::once("img".to_string())
                .chain(image_roots.iter().cloned())
                .chain(std::iter::once(String::new()))
            {
                let candidate = if root.is_empty() {
                    relative.to_string()
                } else {
                    format!("{root}/{relative}")
                };
                if let Some(found) = lookup(&candidate) {
                    image_path = Some(found);
                    break;
                }
            }
            let Some(image_path) = image_path else {
                continue;
            };
            let Ok(ticks) = pose.duration.parse::<i64>() else {
                continue;
            };
            if !(1..=1500).contains(&ticks) {
                continue;
            };
            let Some((ax, ay)) = parse_anchor(&pose.anchor) else {
                continue;
            };
            if !used.contains_key(&image_path) {
                if used.len() >= 256 {
                    continue;
                }
                let decoded =
                    decode_png(&files[&image_path]).map_err(|_| err("源 PNG 尺寸超出限制"))?;
                if decoded.width.max(decoded.height) > 4096 {
                    return Err(err("源 PNG 尺寸超出限制"));
                }
                let pixels: usize = images
                    .values()
                    .map(|image| image.width as usize * image.height as usize)
                    .sum();
                if (pixels + decoded.width as usize * decoded.height as usize) * 4
                    > 128 * 1024 * 1024
                {
                    return Err(err("素材解码内存超过 128 MiB"));
                }
                used.insert(image_path.clone(), format!("source-{}", used.len() + 1));
                images.insert(image_path.clone(), decoded);
            }
            frames.push(json!({"id": format!("frame-{}", frames.len() + 1), "assetId": used[&image_path], "ticks": ticks, "anchor": [ax, ay]}));
            if frames.len() >= 256 {
                break;
            }
        }
        if !frames.is_empty() {
            clips.push(json!({"id": format!("clip-{}", clips.len() + 1), "name": name.chars().take(80).collect::<String>(), "kind": "frames", "loop": true, "frames": frames}));
        }
        if clips.len() >= 64 {
            break;
        }
    }
    if clips.is_empty() {
        return Err(err(
            "未发现可提取的整帧动画；Live2D 或参数化动作不能作为 PNG 部件直接读取",
        ));
    }
    let max_edge = images
        .values()
        .map(|image| image.width.max(image.height))
        .max()
        .unwrap_or(1) as f64;
    let scale = (128.0 / max_edge).min(1.0);
    let mut assets = Vec::new();
    for (path, image) in &images {
        let mut canvas = Image::new(128, 128);
        let size = (
            (image.width as f64 * scale).round().max(1.0) as u32,
            (image.height as f64 * scale).round().max(1.0) as u32,
        );
        canvas.paste(&image.resize_nearest(size.0, size.1), (0, 0));
        assets.push(json!({
            "id": used[path],
            "name": Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or("image.png"),
            "src": data_url(&canvas.encode_png()?),
        }));
    }
    for clip in &mut clips {
        for frame in clip["frames"].as_array_mut().unwrap() {
            let anchor = frame["anchor"].as_array().unwrap();
            let ax = anchor[0].as_i64().unwrap_or(0) as f64;
            let ay = anchor[1].as_i64().unwrap_or(0) as f64;
            let x = py_round(64.0 - ax * scale);
            let y = py_round(128.0 - ay * scale);
            if !(-128..=128).contains(&x) || !(-128..=128).contains(&y) {
                return Err(err(
                    "源动作锚点超出当前编辑画布；请在桌宠素材目录中原样导入此包",
                ));
            }
            frame.as_object_mut().unwrap().remove("anchor");
            frame["x"] = json!(x);
            frame["y"] = json!(y);
        }
    }
    let default = clips
        .iter()
        .find(|clip| {
            let name = clip["name"].as_str().unwrap_or("");
            let lower = name.to_lowercase();
            lower.contains("stand")
                || lower.contains("idle")
                || name.contains('立')
                || name.contains("待机")
        })
        .and_then(|clip| clip["id"].as_str())
        .unwrap_or(clips[0]["id"].as_str().unwrap());
    let name = format!("{} · 动作提取", source["name"].as_str().unwrap_or("素材"));
    Ok(json!({
        "version": 2,
        "name": name.chars().take(100).collect::<String>(),
        "defaultActionId": default,
        "assets": assets,
        "actions": clips,
        "parts": [],
        "regions": [],
        "rules": [],
        "source": {
            "id": source["id"],
            "url": source["url"],
            "revision": source.get("revision"),
            "archiveSha256": source["archiveSha256"],
            "licenseNote": source["licenseNote"],
            "conversion": "仅提取整帧动画片段；原包表达式、状态机和移动物理请通过桌宠素材目录原样导入。没有自动拆分人体部件。"
        }
    }))
}

struct PoseRec {
    image: String,
    duration: String,
    anchor: String,
}
struct AnimRec {
    condition: Option<String>,
    poses: Vec<PoseRec>,
}
struct ActionRec {
    animations: Vec<AnimRec>,
}

fn action_elements(document: &str) -> Vec<(String, ActionRec)> {
    // A small pull parser is enough: classic packs keep poses directly under one animation.
    let mut actions = Vec::new();
    let mut current: Option<(String, ActionRec)> = None;
    let mut animation: Option<AnimRec> = None;
    for raw in document.split('<').skip(1) {
        let (tag, _rest) = raw.split_once('>').unwrap_or((raw, ""));
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .rsplit('}')
            .next()
            .unwrap_or("");
        if closing {
            if name == "アニメーション" || name == "Animation" {
                if let (Some(action), Some(animation_value)) = (current.as_mut(), animation.take())
                {
                    action.1.animations.push(animation_value);
                }
            } else if name == "動作" || name == "Action" {
                if let Some(action) = current.take() {
                    actions.push(action);
                }
            }
            continue;
        }
        if name == "動作" || name == "Action" {
            if let Some(action_name) = xml_attr(tag, &["名前", "Name"]) {
                current = Some((
                    action_name,
                    ActionRec {
                        animations: Vec::new(),
                    },
                ));
            }
        } else if (name == "アニメーション" || name == "Animation") && current.is_some() {
            animation = Some(AnimRec {
                condition: xml_attr(tag, &["条件", "Condition"]),
                poses: Vec::new(),
            });
        } else if name == "ポーズ" || name == "Pose" {
            if let Some(animation) = animation.as_mut() {
                if let (Some(image), Some(duration), Some(anchor)) = (
                    xml_attr(tag, &["画像", "Image"]),
                    xml_attr(tag, &["長さ", "Duration"]),
                    xml_attr(tag, &["基準座標", "ImageAnchor"]),
                ) {
                    animation.poses.push(PoseRec {
                        image,
                        duration,
                        anchor,
                    });
                }
            }
        }
    }
    actions
}

fn xml_attr(tag: &str, names: &[&str]) -> Option<String> {
    for name in names {
        let needle = format!("{name}=\"");
        if let Some(start) = tag.find(&needle) {
            let rest = &tag[start + needle.len()..];
            let end = rest.find('"')?;
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn parse_anchor(anchor: &str) -> Option<(i64, i64)> {
    let (x, y) = anchor.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn selected_files(data: &[u8], source: &Value) -> Result<HashMap<String, Vec<u8>>> {
    let prefix = {
        let mut parts = components(source["archivePrefix"].as_str().unwrap_or(""), true)?;
        parts.extend(components(
            source["packPrefix"].as_str().unwrap_or(""),
            true,
        )?);
        parts
    };
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|error| err(error.to_string()))?;
    if archive.len() > 5000 {
        return Err(err("归档超过 5000 个条目"));
    }
    let mut selected = HashMap::new();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| err(error.to_string()))?;
        let path = components(entry.name(), false).unwrap_or_default();
        let canonical = path.join("/");
        if !seen.insert(canonical.clone()) {
            return Err(err("归档包含重复文件路径"));
        }
        let mode = entry.unix_mode().unwrap_or(0);
        if entry.is_symlink()
            || !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000)
            || entry.encrypted()
        {
            return Err(err("归档包含链接、特殊文件或加密条目"));
        }
        total += entry.size();
        if total > MAX_EXPANDED {
            return Err(err("归档展开大小超过 200 MiB"));
        }
        if path.len() <= prefix.len() || path[..prefix.len()] != prefix[..] || entry.is_dir() {
            continue;
        }
        let relative = path[prefix.len()..].join("/");
        if selected.contains_key(&relative) {
            return Err(err("归档包含重复文件路径"));
        }
        let lower = relative.to_ascii_lowercase();
        if lower.ends_with(".png") || lower.ends_with(".xml") {
            if entry.size() > 8 * 1024 * 1024 {
                return Err(err("单张素材或 XML 超过 8 MiB"));
            }
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| err(error.to_string()))?;
            if bytes.len() as u64 != entry.size() {
                return Err(err("归档实际文件大小不符"));
            }
            selected.insert(relative, bytes);
        }
    }
    Ok(selected)
}

fn importable(source: &Value) -> bool {
    source.get("kind").and_then(Value::as_str) == Some("pack")
        && source
            .get("archiveUrl")
            .and_then(Value::as_str)
            .is_some_and(|url| !url.is_empty())
        && source
            .get("archiveSha256")
            .and_then(Value::as_str)
            .is_some_and(|sha| !sha.is_empty())
        && source.get("archivePrefix").is_some()
        && source.get("packPrefix").is_some()
}

fn is_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-')
}

fn https_host(url: &str) -> Result<String> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(err("下载地址必须是目录内固定的 GitHub HTTPS 地址"));
    };
    if rest.contains('@') {
        return Err(err("下载地址必须是目录内固定的 GitHub HTTPS 地址"));
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty() {
        return Err(err("下载地址必须是目录内固定的 GitHub HTTPS 地址"));
    }
    Ok(host.to_ascii_lowercase())
}

fn components(path: &str, allow_empty: bool) -> Result<Vec<String>> {
    if path.is_empty() && allow_empty {
        return Ok(Vec::new());
    }
    if path.starts_with(['/', '\\']) || path.contains('\\') || path.contains('\0') {
        return Err(err("归档路径无效"));
    }
    let values: Vec<String> = path
        .trim_end_matches('/')
        .split('/')
        .map(str::to_string)
        .collect();
    if values.len() > 16
        || values
            .iter()
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
    {
        return Err(err("归档路径越界或过深"));
    }
    Ok(values)
}

fn py_round(value: f64) -> i64 {
    let floor = value.floor();
    let diff = value - floor;
    if (diff - 0.5).abs() < 1e-9 {
        let even = floor as i64;
        if even.rem_euclid(2) == 0 {
            even
        } else {
            even + 1
        }
    } else {
        value.round() as i64
    }
}

#[allow(dead_code)]
fn _touch(file: &mut impl Write) {
    let _ = file;
}
