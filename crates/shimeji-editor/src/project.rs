use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Write};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::error::{Result, err};
use crate::imagebuf::{Image, data_url, decode_data_url, decode_png, hex};
use crate::parts::{bake_action, validate_parts};

const PNG_PREFIX: &str = "data:image/png;base64,";
const EVENTS: [&str; 6] = ["click", "pet", "enter", "drag", "release", "idle"];
const README: &str = "\
shimeji2 动作编辑器原型\n\n\
ZIP 可导入 shimeji2；默认动作、动画帧、时长和偏移写入经典 XML。\n\
多部件动画在导出时按 40ms tick 合成为固定画布帧；原始部件与关键帧保存在工程中。\n\
VRM 动作已烘焙为 128×128 透明序列帧；原始 VRM/VRMA 请在 3D 工作室单独保存工程。\n\
动图与图集已还原为 PNG 序列；素材组、来源和组内时长保存在 editor-project.json 中。\n\
editor-runtime.json 由带编辑器支持的 shimeji2 执行，包含触摸区域和互动规则。\n\
请使用包含编辑器运行时的新构建；旧构建只能播放动画，不能执行新规则。\n\
要继续编辑，请在编辑器中打开 editor-project.json。\n\
内置 shime 素材来源与许可见下列文件；用户导入素材的许可由用户确认。\n";

pub fn demo_project(shime: &Path) -> Result<Value> {
    let definitions = read_actions(&(shime.join("conf/動作.xml")))?;
    let mut actions = Vec::new();
    for (ident, name, source, looped) in [
        ("idle", "站着发呆", "立つ", true),
        ("walk", "轻快走路", "歩く", true),
        ("look", "抬头看看", "座って首が回る", false),
        ("sit", "晃晃小脚", "足をぶらぶらさせる", true),
        ("sleep", "趴下休息", "寝そべる", true),
    ] {
        let frames = definitions
            .get(source)
            .ok_or_else(|| err(format!("示例动作缺失：{source}")))?;
        let frames = frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let mut frame = frame.clone();
                frame["id"] = json!(format!("{ident}-{index}"));
                frame
            })
            .collect::<Vec<_>>();
        actions.push(json!({
            "id": ident,
            "name": name,
            "loop": looped,
            "frames": frames,
        }));
    }
    let mut assets = Vec::new();
    for index in 1..=46 {
        assets.push(json!({
            "id": format!("shime{index}"),
            "name": format!("shime{index}.png"),
            "src": format!("/assets/shime{index}.png"),
        }));
    }
    let mut project = json!({
        "version": 2,
        "name": "白色 shime · 我的动作实验",
        "defaultActionId": "idle",
        "assets": assets,
        "actions": actions,
        "regions": [
            {"id": "head", "name": "头部", "x": 24, "y": 4, "width": 80, "height": 68},
            {"id": "body", "name": "身体", "x": 43, "y": 80, "width": 42, "height": 45}
        ],
        "rules": [
            {"id": "pet-head", "name": "摸摸头，抬头看看", "event": "pet", "regionId": "head",
             "actionId": "look", "fromActionId": "*", "cooldown": 50, "priority": 10,
             "enabled": true, "returnToDefault": true},
            {"id": "click-body", "name": "点一下，晃晃脚", "event": "click", "regionId": "body",
             "actionId": "sit", "fromActionId": "*", "cooldown": 25, "priority": 5,
             "enabled": true, "returnToDefault": true},
            {"id": "idle-sleep", "name": "安静一会儿就休息", "event": "idle", "regionId": "*",
             "actionId": "sleep", "fromActionId": "idle", "cooldown": 125, "priority": 1,
             "enabled": true, "returnToDefault": false}
        ],
    });
    let source = decode_png(
        &std::fs::read(shime.join("img/shime1.png")).map_err(|error| err(error.to_string()))?,
    )?;
    let mut parts = Vec::new();
    for (ident, name, top, height) in [
        ("demo-head-png", "头部.png", 0u32, 74u32),
        ("demo-body-png", "身体.png", 74, 54),
    ] {
        let mut image = Image::new(128, 128);
        image.paste(&source.crop(0, top, 128, height), (0, top));
        let encoded = image.encode_png()?;
        project["assets"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": ident, "name": name, "src": data_url(&encoded)}));
        parts.push((ident, name));
    }
    let body = json!({
        "id": "demo-body", "name": "身体", "assetId": "demo-body-png", "parentId": null,
        "x": 64, "y": 128, "pivotX": 64, "pivotY": 128, "rotation": 0,
        "scaleX": 1, "scaleY": 1, "visible": true
    });
    let head = json!({
        "id": "demo-head", "name": "头部", "assetId": "demo-head-png", "parentId": "demo-body",
        "x": 0, "y": -55, "pivotX": 64, "pivotY": 73, "rotation": 0,
        "scaleX": 1, "scaleY": 1, "visible": true
    });
    project["parts"] = json!([body, head]);
    let dance = standing_dance(&project)?;
    project["actions"].as_array_mut().unwrap().push(dance);
    let breathe_key = |tick: i64, scale_y: f64| {
        json!({
            "x": 64, "y": 128, "rotation": 0, "scaleX": 1, "scaleY": scale_y,
            "visible": true, "assetId": "demo-body-png", "tick": tick
        })
    };
    project["actions"].as_array_mut().unwrap().push(json!({
        "id": "breathe",
        "name": "部件 · 轻轻呼吸",
        "kind": "parts",
        "loop": true,
        "durationTicks": 50,
        "frames": [{"id": "breathe-0", "assetId": "shime1", "ticks": 50, "x": 0, "y": 0}],
        "tracks": [{"partId": "demo-body", "keys": [
            breathe_key(0, 1.0), breathe_key(25, 1.025), breathe_key(50, 1.0)
        ]}]
    }));
    let _ = parts;
    Ok(project)
}

pub fn standing_dance(project: &Value) -> Result<Value> {
    let parts = project
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| err("站立舞蹈需要内置示例图"))?;
    let body = parts
        .iter()
        .find(|part| part.get("id").and_then(Value::as_str) == Some("demo-body"))
        .ok_or_else(|| err("站立舞蹈需要内置示例图"))?;
    let head = parts
        .iter()
        .find(|part| part.get("id").and_then(Value::as_str) == Some("demo-head"))
        .ok_or_else(|| err("站立舞蹈需要内置示例图"))?;
    let fields = [
        "x", "y", "rotation", "scaleX", "scaleY", "visible", "assetId",
    ];
    let pick = |part: &Value| {
        let mut map = serde_json::Map::new();
        for field in fields {
            if let Some(value) = part.get(field) {
                map.insert(field.to_string(), value.clone());
            }
        }
        map
    };
    let rotations = [0, -5, 0, 5, 0, -4, 0, 4, 0];
    let head_rotations = [0, 6, 0, -6, 0, 4, 0, -4, 0];
    let mut body_keys = Vec::new();
    let mut head_keys = Vec::new();
    for (index, rotation) in rotations.iter().enumerate() {
        let mut key = pick(body);
        key.insert("tick".into(), json!(index * 5));
        key.insert("x".into(), json!(64));
        key.insert("y".into(), json!(126));
        key.insert("scaleX".into(), json!(0.94));
        key.insert("scaleY".into(), json!(0.94));
        key.insert("rotation".into(), json!(rotation));
        body_keys.push(Value::Object(key));
    }
    for (index, rotation) in head_rotations.iter().enumerate() {
        let mut key = pick(head);
        key.insert("tick".into(), json!(index * 5));
        key.insert("rotation".into(), json!(rotation));
        head_keys.push(Value::Object(key));
    }
    Ok(json!({
        "id": "standing-dance",
        "name": "站着跳舞",
        "kind": "parts",
        "loop": true,
        "durationTicks": 40,
        "frames": [{"id": "dance-rest", "assetId": "shime1", "ticks": 40, "x": 0, "y": 0}],
        "tracks": [
            {"partId": "demo-body", "keys": body_keys},
            {"partId": "demo-head", "keys": head_keys}
        ]
    }))
}

pub fn dance_preset(shime: &Path) -> Result<Value> {
    let project = demo_project(shime)?;
    let action = project["actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["id"] == "standing-dance")
        })
        .ok_or_else(|| err("站立舞蹈需要内置示例图"))?;
    let mut sources = HashMap::new();
    for asset in project["assets"].as_array().unwrap() {
        sources.insert(
            asset["id"].as_str().unwrap().to_string(),
            asset_bytes(shime, asset)?,
        );
    }
    let (frames, images) = bake_action(&project, action, &sources)?;
    Ok(json!({
        "assets": images.iter().enumerate().map(|(index, (ident, data))| json!({
            "id": ident,
            "name": format!("舞蹈帧 {}", index + 1),
            "src": data_url(data),
        })).collect::<Vec<_>>(),
        "action": {"id": "standing-dance", "name": "站着跳舞", "kind": "frames", "loop": true, "frames": frames}
    }))
}

pub fn asset_bytes(shime: &Path, asset: &Value) -> Result<Vec<u8>> {
    let src = asset
        .get("src")
        .and_then(Value::as_str)
        .ok_or_else(|| err("图片需要是内置素材或不大于 2 MB 的 PNG"))?;
    if let Some(name) = shime_name(src) {
        return std::fs::read(shime.join("img").join(name)).map_err(|error| err(error.to_string()));
    }
    if !src.starts_with(PNG_PREFIX) || src.len() > 2_800_000 {
        return Err(err("图片需要是内置素材或不大于 2 MB 的 PNG"));
    }
    let data = decode_data_url(src)?;
    if data.len() < 33 || &data[..8] != b"\x89PNG\r\n\x1a\n" || &data[12..16] != b"IHDR" {
        return Err(err("PNG 文件头无效"));
    }
    let width = u32::from_be_bytes(data[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(data[20..24].try_into().unwrap());
    if (width, height) != (128, 128) {
        return Err(err("导入图片需要由编辑器归一化到 128 × 128"));
    }
    Ok(data)
}

pub fn shime_name(src: &str) -> Option<&str> {
    let name = src.strip_prefix("/assets/")?;
    let number = name.strip_prefix("shime")?.strip_suffix(".png")?;
    let value: u32 = number.parse().ok()?;
    if (1..=46).contains(&value) && name == format!("shime{value}.png") {
        Some(name)
    } else {
        None
    }
}

pub fn validate_project(shime: &Path, project: &Value) -> Result<()> {
    let version = project.get("version").and_then(Value::as_i64);
    if !matches!(version, Some(1 | 2)) {
        return Err(err("不支持的工程版本"));
    }
    let name = project.get("name").and_then(Value::as_str).unwrap_or("");
    if !(1..=100).contains(&name.chars().count()) {
        return Err(err("工程名称需要包含 1–100 个字符"));
    }
    let assets = unique(project.get("assets"), 256)?;
    let actions = unique(project.get("actions"), 64)?;
    let default = project
        .get("defaultActionId")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !actions.contains(default) {
        return Err(err("默认动作不存在"));
    }
    for asset in project["assets"].as_array().unwrap() {
        asset_bytes(shime, asset)?;
    }
    let groups = match project.get("assetGroups") {
        None => &[][..],
        Some(Value::Array(groups)) if groups.len() <= 256 => groups.as_slice(),
        _ => return Err(err("素材组数量超过 256 或格式无效")),
    };
    if !groups.is_empty() {
        unique(Some(&Value::Array(groups.to_vec())), 256)?;
    }
    for group in groups {
        if !group_text(group.get("name"), 80) {
            return Err(err("素材组名称需要包含 1–80 个字符"));
        }
        let frames = group
            .get("frames")
            .and_then(Value::as_array)
            .filter(|frames| (1..=256).contains(&frames.len()))
            .ok_or_else(|| err("素材组需要包含 1–256 帧"))?;
        for frame in frames {
            let Value::Object(_) = frame else {
                return Err(err("素材组帧格式无效"));
            };
            if !assets.contains(frame.get("assetId").and_then(Value::as_str).unwrap_or("")) {
                return Err(err("素材组引用了不存在的图片"));
            }
            integer(frame.get("ticks"), 1, 1500)?;
        }
        if let Some(source) = group.get("source") {
            if !group_text(source.get("format"), 80) {
                return Err(err("素材组来源格式无效"));
            }
            let names = source.get("fileNames").and_then(Value::as_array);
            if names.is_none_or(|names| {
                names.len() > 256 || names.iter().any(|name| !group_text(Some(name), 256))
            }) {
                return Err(err("素材组来源文件名无效"));
            }
        }
    }
    validate_parts(project, &assets)?;
    for action in project["actions"].as_array().unwrap() {
        if !group_text(action.get("name"), 80) {
            return Err(err("动作名称需要包含 1–80 个字符"));
        }
        if action.get("loop").and_then(Value::as_bool).is_none() {
            return Err(err("循环选项需要为布尔值"));
        }
        unique(action.get("frames"), 256)?;
        for frame in action["frames"].as_array().unwrap() {
            if !assets.contains(frame.get("assetId").and_then(Value::as_str).unwrap_or("")) {
                return Err(err("动画引用了不存在的图片"));
            }
            integer(frame.get("ticks"), 1, 1500)?;
            integer(frame.get("x"), -128, 128)?;
            integer(frame.get("y"), -128, 128)?;
        }
    }
    let regions = match project.get("regions") {
        Some(Value::Array(regions)) => regions.as_slice(),
        _ => return Err(err("触摸区域名称需要包含 1–80 个字符")),
    };
    let region_ids = if regions.is_empty() {
        HashSet::new()
    } else {
        unique(Some(&Value::Array(regions.to_vec())), 32)?
    };
    for region in regions {
        if !group_text(region.get("name"), 80) {
            return Err(err("触摸区域名称需要包含 1–80 个字符"));
        }
        let x = integer(region.get("x"), 0, 127)?;
        let y = integer(region.get("y"), 0, 127)?;
        let width = integer(region.get("width"), 1, 128)?;
        let height = integer(region.get("height"), 1, 128)?;
        if x + width > 128 || y + height > 128 {
            return Err(err("触摸区域超出了画布"));
        }
    }
    let rules = match project.get("rules") {
        Some(Value::Array(rules)) => rules.as_slice(),
        _ => return Err(err("互动规则名称需要包含 1–80 个字符")),
    };
    if !rules.is_empty() {
        unique(Some(&Value::Array(rules.to_vec())), 128)?;
    }
    for rule in rules {
        if !group_text(rule.get("name"), 80) {
            return Err(err("互动规则名称需要包含 1–80 个字符"));
        }
        let event = rule.get("event").and_then(Value::as_str).unwrap_or("");
        let action = rule.get("actionId").and_then(Value::as_str).unwrap_or("");
        if !EVENTS.contains(&event) || !actions.contains(action) {
            return Err(err("互动规则引用无效"));
        }
        let region = rule.get("regionId").and_then(Value::as_str).unwrap_or("");
        let from = rule
            .get("fromActionId")
            .and_then(Value::as_str)
            .unwrap_or("");
        if (!region_ids.contains(region) && region != "*")
            || (!actions.contains(from) && from != "*")
        {
            return Err(err("互动条件引用无效"));
        }
        integer(rule.get("cooldown"), 0, 1500)?;
        integer(rule.get("priority"), 0, 100)?;
        if rule.get("enabled").and_then(Value::as_bool).is_none()
            || rule
                .get("returnToDefault")
                .and_then(Value::as_bool)
                .is_none()
        {
            return Err(err("规则开关需要为布尔值"));
        }
    }
    Ok(())
}

pub fn export_pack(shime: &Path, project: &Value) -> Result<Vec<u8>> {
    validate_project(shime, project)?;
    let mut sources = HashMap::new();
    let mut order = Vec::new();
    for asset in project["assets"].as_array().unwrap() {
        let ident = asset["id"].as_str().unwrap().to_string();
        order.push(ident.clone());
        sources.insert(ident, asset_bytes(shime, asset)?);
    }
    let mut playback = HashMap::<String, Vec<Value>>::new();
    let mut baked = Vec::<(String, Vec<u8>)>::new();
    for action in project["actions"].as_array().unwrap() {
        let ident = action["id"].as_str().unwrap();
        if action.get("kind").and_then(Value::as_str) == Some("parts") {
            let (frames, images) = bake_action(project, action, &sources)?;
            for (name, _) in &images {
                if sources.contains_key(name) {
                    return Err(err("部件素材 ID 与导出动画 ID 冲突"));
                }
            }
            baked.extend(images);
            playback.insert(ident.to_string(), frames);
        } else {
            playback.insert(
                ident.to_string(),
                action["frames"].as_array().unwrap().clone(),
            );
        }
    }
    let default_id = project["defaultActionId"].as_str().unwrap();
    let default_name = format!("Editor_{default_id}");
    let first = &playback[default_id][0];
    let mut actions_xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Mascot><ActionList>");
    let mut behaviors_xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Mascot><BehaviorList>");
    let add_action = |xml: &mut String, name: &str, frames: &[Value], builtin: Option<&str>| {
        if let Some(class) = builtin {
            xml.push_str(&format!(
                "<Action Name=\"{}\" Type=\"Embedded\" Class=\"com.group_finity.mascot.action.{class}\">",
                xml_escape(name)
            ));
        } else {
            let duration: i64 = frames
                .iter()
                .map(|frame| frame["ticks"].as_i64().unwrap_or(0))
                .sum();
            xml.push_str(&format!(
                "<Action Name=\"{}\" Type=\"Stay\" Duration=\"{duration}\">",
                xml_escape(name)
            ));
        }
        xml.push_str("<Animation>");
        for frame in frames {
            let x = frame["x"].as_i64().unwrap_or(0);
            let y = frame["y"].as_i64().unwrap_or(0);
            xml.push_str(&format!(
                "<Pose Image=\"/{}.png\" ImageAnchor=\"{},{}\" Velocity=\"0,0\" Duration=\"{}\"/>",
                xml_escape(frame["assetId"].as_str().unwrap_or("")),
                64 - x,
                128 - y,
                frame["ticks"].as_i64().unwrap_or(0)
            ));
        }
        xml.push_str("</Animation></Action>");
    };
    for (name, builtin) in [
        ("ChaseMouse", None),
        ("Fall", Some("Fall")),
        ("Dragged", Some("Dragged")),
        ("Thrown", Some("Fall")),
    ] {
        add_action(&mut actions_xml, name, std::slice::from_ref(first), builtin);
        behaviors_xml.push_str(&format!(
            "<Behavior Name=\"{name}\" Frequency=\"0\"><NextBehaviorList Add=\"false\"><BehaviorReference Name=\"{default_name}\" Frequency=\"1\"/></NextBehaviorList></Behavior>"
        ));
    }
    for action in project["actions"].as_array().unwrap() {
        let ident = action["id"].as_str().unwrap();
        let name = format!("Editor_{ident}");
        let frames = &playback[ident];
        add_action(&mut actions_xml, &name, frames, None);
        let looped = action["loop"].as_bool().unwrap_or(false);
        let next = if looped {
            name.as_str()
        } else {
            default_name.as_str()
        };
        if name == default_name {
            behaviors_xml.push_str(&format!(
                "<Behavior Name=\"{name}\" Frequency=\"1\" Condition=\"#{{mascot.environment.floor.isOn(mascot.anchor) || mascot.environment.activeIE.topBorder.isOn(mascot.anchor)}}\"><NextBehaviorList Add=\"false\"><BehaviorReference Name=\"{next}\" Frequency=\"1\"/></NextBehaviorList></Behavior>"
            ));
        } else {
            let frequency = "0";
            behaviors_xml.push_str(&format!(
                "<Behavior Name=\"{name}\" Frequency=\"{frequency}\"><NextBehaviorList Add=\"false\"><BehaviorReference Name=\"{next}\" Frequency=\"1\"/></NextBehaviorList></Behavior>"
            ));
        }
    }
    actions_xml.push_str("</ActionList></Mascot>");
    behaviors_xml.push_str("</BehaviorList></Mascot>");
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut write = |name: &str, bytes: &[u8]| -> Result<()> {
        zip.start_file(name, options)
            .map_err(|error| err(error.to_string()))?;
        zip.write_all(bytes)
            .map_err(|error| err(error.to_string()))?;
        Ok(())
    };
    write("conf/actions.xml", actions_xml.as_bytes())?;
    write("conf/behaviors.xml", behaviors_xml.as_bytes())?;
    for ident in &order {
        write(&format!("img/{ident}.png"), &sources[ident])?;
    }
    for (ident, bytes) in &baked {
        write(&format!("img/{ident}.png"), bytes)?;
    }
    write(
        "editor-project.json",
        &serde_json::to_vec_pretty(project).map_err(|error| err(error.to_string()))?,
    )?;
    let runtime = json!({
        "version": 1,
        "defaultActionId": default_id,
        "actions": project["actions"].as_array().unwrap().iter().map(|action| {
            let ident = action["id"].as_str().unwrap();
            json!({
                "id": ident,
                "name": action["name"],
                "loop": action["loop"],
                "durationTicks": playback[ident].iter().map(|frame| frame["ticks"].as_i64().unwrap_or(0)).sum::<i64>(),
            })
        }).collect::<Vec<_>>(),
        "regions": project["regions"],
        "rules": project["rules"],
    });
    write(
        "editor-runtime.json",
        &serde_json::to_vec_pretty(&runtime).map_err(|error| err(error.to_string()))?,
    )?;
    if let Some(source) = project.get("source") {
        write(
            "SOURCE.txt",
            &serde_json::to_vec_pretty(source).map_err(|error| err(error.to_string()))?,
        )?;
    }
    write("EDITOR-README.txt", README.as_bytes())?;
    if project["assets"].as_array().unwrap().iter().any(|asset| {
        asset
            .get("src")
            .and_then(Value::as_str)
            .is_some_and(|src| src.starts_with("/assets/shime"))
    }) {
        for name in ["LICENSE.txt", "ATTRIBUTION.md"] {
            let bytes = std::fs::read(shime.join(name)).map_err(|error| err(error.to_string()))?;
            write(&format!("bundled-shime-{name}"), &bytes)?;
        }
    }
    let cursor = zip.finish().map_err(|error| err(error.to_string()))?;
    let _ = hex(&Sha256::digest(cursor.get_ref()));
    Ok(cursor.into_inner())
}

fn read_actions(path: &Path) -> Result<HashMap<String, Vec<Value>>> {
    let text = std::fs::read_to_string(path).map_err(|error| err(error.to_string()))?;
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(true);
    let mut actions = HashMap::new();
    let mut current: Option<String> = None;
    let mut capture = false;
    let mut seen_animation = false;
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(tag)) => {
                let name = local(tag.name().as_ref());
                if name == "動作" || name == "Action" {
                    current = attr(&tag, &["名前", "Name"]);
                    capture = false;
                    seen_animation = false;
                } else if (name == "アニメーション" || name == "Animation")
                    && current.is_some()
                    && !seen_animation
                {
                    capture = true;
                } else if (name == "ポーズ" || name == "Pose") && capture {
                    if let Some(action) = &current {
                        if let Some(frame) =
                            pose_frame(action, actions.get(action).map(Vec::len).unwrap_or(0), &tag)
                        {
                            actions.entry(action.clone()).or_default().push(frame);
                        }
                    }
                }
            }
            Ok(Event::Empty(tag)) => {
                let name = local(tag.name().as_ref());
                if (name == "ポーズ" || name == "Pose") && capture {
                    if let Some(action) = &current {
                        if let Some(frame) =
                            pose_frame(action, actions.get(action).map(Vec::len).unwrap_or(0), &tag)
                        {
                            actions.entry(action.clone()).or_default().push(frame);
                        }
                    }
                }
            }
            Ok(Event::End(tag)) => {
                let name = local(tag.name().as_ref());
                if name == "動作" || name == "Action" {
                    current = None;
                    capture = false;
                } else if name == "アニメーション" || name == "Animation" {
                    if capture {
                        seen_animation = true;
                    }
                    capture = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(err(format!("示例动作 XML 无法读取：{error}"))),
            _ => {}
        }
        buffer.clear();
    }
    Ok(actions)
}

fn pose_frame(
    action: &str,
    index: usize,
    tag: &quick_xml::events::BytesStart<'_>,
) -> Option<Value> {
    let image = attr(tag, &["画像", "Image"])?;
    let anchor = attr(tag, &["基準座標", "ImageAnchor"])?;
    let ticks = attr(tag, &["長さ", "Duration"])?.parse::<i64>().ok()?;
    let mut coords = anchor.split(',');
    let x: i64 = coords.next()?.trim().parse().ok()?;
    let y: i64 = coords.next()?.trim().parse().ok()?;
    let asset = image.rsplit(['/', '\\']).next().unwrap_or(&image);
    let asset = asset.strip_suffix(".png").unwrap_or(asset);
    Some(json!({
        "id": format!("{action}-{index}"),
        "assetId": asset,
        "ticks": ticks,
        "x": 64 - x,
        "y": 128 - y,
    }))
}

fn attr(tag: &quick_xml::events::BytesStart<'_>, names: &[&str]) -> Option<String> {
    for attribute in tag.attributes().flatten() {
        let key = String::from_utf8_lossy(attribute.key.as_ref()).to_string();
        let key = key.rsplit('}').next().unwrap_or(&key);
        if names.contains(&key) {
            return Some(String::from_utf8_lossy(&attribute.value).into_owned());
        }
    }
    None
}

fn local(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    name.rsplit('}').next().unwrap_or(&name).to_string()
}

fn unique(value: Option<&Value>, limit: usize) -> Result<HashSet<String>> {
    let items = value
        .and_then(Value::as_array)
        .filter(|items| (1..=limit).contains(&items.len()))
        .ok_or_else(|| err(format!("列表需要包含 1–{limit} 个对象")))?;
    let mut ids = HashSet::new();
    for item in items {
        let ident = identifier(item.get("id"))?;
        if !ids.insert(ident) {
            return Err(err("项目包含重复 ID"));
        }
    }
    Ok(ids)
}

fn identifier(value: Option<&Value>) -> Result<String> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Err(err("无效的项目对象 ID"));
    };
    if (1..=64).contains(&text.len())
        && text
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-')
    {
        Ok(text.to_string())
    } else {
        Err(err("无效的项目对象 ID"))
    }
}

fn integer(value: Option<&Value>, low: i64, high: i64) -> Result<i64> {
    let Some(number) = value.and_then(Value::as_i64) else {
        return Err(err(format!("数值需要在 {low}–{high} 之间")));
    };
    if (low..=high).contains(&number) {
        Ok(number)
    } else {
        Err(err(format!("数值需要在 {low}–{high} 之间")))
    }
}

fn group_text(value: Option<&Value>, limit: usize) -> bool {
    let Some(text) = value.and_then(Value::as_str) else {
        return false;
    };
    let trimmed = text.trim();
    !trimmed.is_empty()
        && text.chars().count() <= limit
        && !text
            .chars()
            .any(|char| char <= '\u{1f}' || char == '\u{7f}')
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
