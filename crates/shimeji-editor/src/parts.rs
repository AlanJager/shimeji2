use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::error::{Result, err};
use crate::imagebuf::{Image, decode_png, invert_affine, multiply};

pub fn validate_parts(project: &Value, assets: &HashSet<String>) -> Result<()> {
    let parts = match project.get("parts") {
        None => &[][..],
        Some(Value::Array(parts)) if parts.len() <= 32 => parts.as_slice(),
        _ => return Err(err("一个工程最多包含 32 个部件")),
    };
    let mut by_id = HashMap::<String, &Value>::new();
    for part in parts {
        let ident = part_id(part)?;
        if by_id.contains_key(&ident) {
            return Err(err("部件 ID 无效或重复"));
        }
        let name = part
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| err("部件名称需要包含 1–80 个字符"))?;
        if !(1..=80).contains(&name.chars().count()) {
            return Err(err("部件名称需要包含 1–80 个字符"));
        }
        number(part.get("pivotX"), -256.0, 256.0)?;
        number(part.get("pivotY"), -256.0, 256.0)?;
        validate_pose(part, assets)?;
        by_id.insert(ident, part);
    }
    for part in parts {
        let mut seen = HashSet::from([part_id(part)?]);
        let mut parent = part.get("parentId");
        while !matches!(parent, None | Some(Value::Null)) {
            let Some(ident) = parent.and_then(Value::as_str) else {
                return Err(err("部件层级包含循环或不存在的父部件"));
            };
            if !by_id.contains_key(ident) || !seen.insert(ident.to_string()) {
                return Err(err("部件层级包含循环或不存在的父部件"));
            }
            parent = by_id[ident].get("parentId");
        }
    }
    let mut baked_ticks = 0i64;
    let actions = project
        .get("actions")
        .and_then(Value::as_array)
        .ok_or_else(|| err("不支持的动作类型"))?;
    for action in actions {
        let kind = action
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("frames");
        if kind != "frames" && kind != "parts" {
            return Err(err("不支持的动作类型"));
        }
        if kind != "parts" {
            continue;
        }
        let duration = json_i64(action.get("durationTicks"))
            .filter(|value| (1..=1000).contains(value))
            .ok_or_else(|| err("部件动作需要至少一个部件，时长为 1–1000 tick"))?;
        if parts.is_empty() {
            return Err(err("部件动作需要至少一个部件，时长为 1–1000 tick"));
        }
        baked_ticks += duration;
        let tracks = action
            .get("tracks")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if tracks.len() > parts.len() {
            return Err(err("部件动画轨道数量无效"));
        }
        let mut seen_parts = HashSet::new();
        for track in tracks {
            let part_id = track.get("partId").and_then(Value::as_str).unwrap_or("");
            if !by_id.contains_key(part_id) || !seen_parts.insert(part_id.to_string()) {
                return Err(err("动画轨道引用无效或重复"));
            }
            let keys = track
                .get("keys")
                .and_then(Value::as_array)
                .ok_or_else(|| err("每条轨道需要包含 1–256 个关键帧"))?;
            if !(1..=256).contains(&keys.len()) {
                return Err(err("每条轨道需要包含 1–256 个关键帧"));
            }
            let mut previous = -1i64;
            for key in keys {
                let tick = json_i64(key.get("tick"))
                    .filter(|tick| previous < *tick && *tick <= duration)
                    .ok_or_else(|| err("关键帧时间需要升序、唯一且在动作时长内"))?;
                previous = tick;
                validate_pose(key, assets)?;
            }
        }
    }
    if baked_ticks > 2000 {
        return Err(err(
            "部件动画总时长不能超过 2000 tick（80 秒），请拆分为多个工程",
        ));
    }
    Ok(())
}

pub type BakedFrames = (Vec<Value>, Vec<(String, Vec<u8>)>);

pub fn bake_action(
    project: &Value,
    action: &Value,
    sources: &HashMap<String, Vec<u8>>,
) -> Result<BakedFrames> {
    let duration = json_i64(action.get("durationTicks")).unwrap_or(0);
    let mut decoded = HashMap::<String, Image>::new();
    let mut frames = Vec::new();
    let mut images = Vec::<(String, Vec<u8>)>::new();
    for tick in 0..duration {
        let mut canvas = Image::new(128, 128);
        let mut composed = compose_parts(project, action, tick)?;
        composed.sort_by(|left, right| {
            left.z_order
                .cmp(&right.z_order)
                .then(left.index.cmp(&right.index))
        });
        for part in composed {
            if !part.visible || part.opacity <= 0.0 {
                continue;
            }
            if !decoded.contains_key(&part.asset_id) {
                let bytes = sources
                    .get(&part.asset_id)
                    .ok_or_else(|| err("部件 PNG 需要归一化为 128 × 128"))?;
                let image = decode_png(bytes)?;
                if (image.width, image.height) != (128, 128) {
                    return Err(err("部件 PNG 需要归一化为 128 × 128"));
                }
                decoded.insert(part.asset_id.clone(), image);
            }
            let inverse = invert_affine(part.matrix)?;
            let mut warped = decoded[&part.asset_id].affine_nearest((128, 128), inverse);
            if part.opacity < 1.0 {
                warped.scale_alpha(part.opacity);
            }
            canvas.alpha_composite(&warped, (0, 0));
        }
        let ident = format!("baked-{}", canvas.sha256_prefix());
        if images.iter().all(|(name, _)| name != &ident) {
            images.push((ident.clone(), canvas.encode_png()?));
        }
        if frames
            .last()
            .and_then(|frame: &Value| frame.get("assetId"))
            .and_then(Value::as_str)
            == Some(ident.as_str())
        {
            let ticks = frames.last_mut().unwrap().get_mut("ticks").unwrap();
            *ticks = json!(ticks.as_i64().unwrap_or(0) + 1);
        } else {
            frames.push(json!({
                "id": format!("bake-{tick}"),
                "assetId": ident,
                "ticks": 1,
                "x": 0,
                "y": 0,
            }));
        }
    }
    Ok((frames, images))
}

struct Placed {
    index: usize,
    asset_id: String,
    matrix: [f64; 6],
    visible: bool,
    opacity: f64,
    z_order: i64,
}

#[derive(Clone)]
struct Node {
    matrix: [f64; 6],
    visible: bool,
}

fn compose_parts(project: &Value, action: &Value, tick: i64) -> Result<Vec<Placed>> {
    let parts = project
        .get("parts")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut nodes = HashMap::<String, Node>::new();
    let mut placed = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let node = resolve(part, parts, action, tick, &mut nodes, &mut HashSet::new())?;
        let pose = pose_at(part, action, tick)?;
        let image = multiply(
            node.matrix,
            [
                1.0,
                0.0,
                0.0,
                1.0,
                -number(part.get("pivotX"), -256.0, 256.0)?,
                -number(part.get("pivotY"), -256.0, 256.0)?,
            ],
        );
        placed.push(Placed {
            index,
            asset_id: pose.asset_id,
            matrix: image,
            visible: node.visible,
            opacity: pose.opacity,
            z_order: pose.z_order,
        });
    }
    Ok(placed)
}

fn resolve<'a>(
    part: &'a Value,
    parts: &'a [Value],
    action: &Value,
    tick: i64,
    nodes: &mut HashMap<String, Node>,
    stack: &mut HashSet<String>,
) -> Result<Node> {
    let ident = part_id(part)?;
    if let Some(node) = nodes.get(&ident) {
        return Ok(node.clone());
    }
    if !stack.insert(ident.clone()) {
        return Err(err("部件层级包含循环或不存在的父部件"));
    }
    let pose = pose_at(part, action, tick)?;
    let radians = pose.rotation.to_radians();
    let (sin, cos) = radians.sin_cos();
    let mut matrix = [
        cos * pose.scale_x,
        sin * pose.scale_x,
        -sin * pose.scale_y,
        cos * pose.scale_y,
        pose.x,
        pose.y,
    ];
    let mut visible = pose.visible;
    if let Some(parent_id) = part.get("parentId").and_then(Value::as_str) {
        let parent = parts
            .iter()
            .find(|item| part_id(item).ok().as_deref() == Some(parent_id))
            .ok_or_else(|| err("部件层级包含循环或不存在的父部件"))?;
        let parent = resolve(parent, parts, action, tick, nodes, stack)?;
        matrix = multiply(parent.matrix, matrix);
        visible = visible && parent.visible;
    }
    stack.remove(&ident);
    let node = Node { matrix, visible };
    nodes.insert(ident, node.clone());
    Ok(node)
}

struct Pose {
    x: f64,
    y: f64,
    rotation: f64,
    scale_x: f64,
    scale_y: f64,
    visible: bool,
    asset_id: String,
    opacity: f64,
    z_order: i64,
}

fn pose_at(part: &Value, action: &Value, tick: i64) -> Result<Pose> {
    let tracks = action
        .get("tracks")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let ident = part_id(part)?;
    let track = tracks
        .iter()
        .find(|track| track.get("partId").and_then(Value::as_str) == Some(ident.as_str()));
    let Some(track) = track else {
        return pose_from(part);
    };
    let keys = track
        .get("keys")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if keys.is_empty() || tick <= json_i64(keys[0].get("tick")).unwrap_or(0) {
        return pose_from(if keys.is_empty() { part } else { &keys[0] });
    }
    for pair in keys.windows(2) {
        let right_tick = json_i64(pair[1].get("tick")).unwrap_or(0);
        if tick < right_tick {
            let left_tick = json_i64(pair[0].get("tick")).unwrap_or(0);
            let mix = (tick - left_tick) as f64 / (right_tick - left_tick) as f64;
            let mut pose = pose_from(&pair[0])?;
            let right = pose_from(&pair[1])?;
            pose.x += (right.x - pose.x) * mix;
            pose.y += (right.y - pose.y) * mix;
            pose.rotation += (right.rotation - pose.rotation) * mix;
            pose.scale_x += (right.scale_x - pose.scale_x) * mix;
            pose.scale_y += (right.scale_y - pose.scale_y) * mix;
            pose.opacity += (right.opacity - pose.opacity) * mix;
            return Ok(pose);
        }
    }
    pose_from(keys.last().unwrap_or(part))
}

fn pose_from(value: &Value) -> Result<Pose> {
    Ok(Pose {
        x: number(value.get("x"), -256.0, 256.0)?,
        y: number(value.get("y"), -256.0, 256.0)?,
        rotation: number(value.get("rotation"), -3600.0, 3600.0)?,
        scale_x: number(value.get("scaleX"), 0.05, 4.0)?,
        scale_y: number(value.get("scaleY"), 0.05, 4.0)?,
        visible: value
            .get("visible")
            .and_then(Value::as_bool)
            .ok_or_else(|| err("部件图片或可见性无效"))?,
        asset_id: value
            .get("assetId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        opacity: match value.get("opacity") {
            None => 1.0,
            Some(opacity) => number(Some(opacity), 0.0, 1.0)?,
        },
        z_order: match value.get("zOrder") {
            None => 0,
            Some(order) => json_i64(Some(order))
                .filter(|order| (-32..=32).contains(order))
                .ok_or_else(|| err("部件遮挡顺序需要是 -32 到 32 的整数"))?,
        },
    })
}

fn validate_pose(pose: &Value, assets: &HashSet<String>) -> Result<()> {
    number(pose.get("x"), -256.0, 256.0)?;
    number(pose.get("y"), -256.0, 256.0)?;
    number(pose.get("rotation"), -3600.0, 3600.0)?;
    number(pose.get("scaleX"), 0.05, 4.0)?;
    number(pose.get("scaleY"), 0.05, 4.0)?;
    let visible = pose.get("visible").and_then(Value::as_bool);
    let asset = pose.get("assetId").and_then(Value::as_str);
    if visible.is_none() || asset.is_none_or(|asset| !assets.contains(asset)) {
        return Err(err("部件图片或可见性无效"));
    }
    if pose.get("opacity").is_some() {
        number(pose.get("opacity"), 0.0, 1.0)?;
    }
    if let Some(order) = pose.get("zOrder") {
        if json_i64(Some(order)).is_none_or(|order| !(-32..=32).contains(&order)) {
            return Err(err("部件遮挡顺序需要是 -32 到 32 的整数"));
        }
    }
    Ok(())
}

fn part_id(part: &Value) -> Result<String> {
    let Some(ident) = part.get("id").and_then(Value::as_str) else {
        return Err(err("部件 ID 无效或重复"));
    };
    if !(1..=64).contains(&ident.len())
        || !ident
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-')
    {
        return Err(err("部件 ID 无效或重复"));
    }
    Ok(ident.to_string())
}

fn number(value: Option<&Value>, low: f64, high: f64) -> Result<f64> {
    let Some(Value::Number(number)) = value else {
        return Err(err(format!("部件数值需要在 {low}–{high} 之间")));
    };
    let Some(number) = number.as_f64() else {
        return Err(err(format!("部件数值需要在 {low}–{high} 之间")));
    };
    if !number.is_finite() || !(low..=high).contains(&number) {
        return Err(err(format!("部件数值需要在 {low}–{high} 之间")));
    }
    Ok(number)
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    value?.as_i64()
}

#[allow(dead_code)]
pub fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}
