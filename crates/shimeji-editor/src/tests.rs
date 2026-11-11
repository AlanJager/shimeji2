use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use serde_json::json;

use crate::imagebuf::{Image, decode_png};
use crate::media::decode_media;
use crate::parts::bake_action;
use crate::project::{demo_project, export_pack};
use crate::server::request_allowed;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn shime() -> PathBuf {
    repo().join("assets/packs/shime")
}

fn png(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut image = Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.set(x, y, paint(x, y));
        }
    }
    image.encode_png().unwrap()
}

#[test]
fn local_requests_stay_on_localhost() {
    assert!(request_allowed("127.0.0.1:4317", None, 4317));
    assert!(request_allowed(
        "localhost:4317",
        Some("http://localhost:4317"),
        4317
    ));
    assert!(!request_allowed("editor.example", None, 4317));
}

#[test]
fn demo_uses_upstream_walk_and_sit_anchor() {
    let project = demo_project(&shime()).unwrap();
    let walk = project["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["id"] == "walk")
        .unwrap();
    let ticks: Vec<i64> = walk["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["ticks"].as_i64().unwrap())
        .collect();
    let assets: Vec<&str> = walk["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["assetId"].as_str().unwrap())
        .collect();
    assert_eq!(ticks, [6, 6, 6, 6]);
    assert_eq!(assets, ["shime1", "shime2", "shime1", "shime3"]);
    let sit = project["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["id"] == "sit")
        .unwrap();
    assert_eq!(sit["frames"][0]["y"], 16);
}

#[test]
fn export_contains_required_behaviors_and_original_png() {
    let project = demo_project(&shime()).unwrap();
    let bytes = export_pack(&shime(), &project).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let original = std::fs::read(shime().join("img/shime1.png")).unwrap();
    let mut stored = Vec::new();
    archive
        .by_name("img/shime1.png")
        .unwrap()
        .read_to_end(&mut stored)
        .unwrap();
    assert_eq!(stored, original);
    let mut behaviors = String::new();
    archive
        .by_name("conf/behaviors.xml")
        .unwrap()
        .read_to_string(&mut behaviors)
        .unwrap();
    for name in ["ChaseMouse", "Fall", "Dragged", "Thrown", "Editor_idle"] {
        assert!(behaviors.contains(name), "{name}");
    }
}

#[test]
fn rotated_parent_paints_child_at_the_pillow_pixel() {
    let red = png(128, 128, |x, y| {
        if x < 8 && y < 8 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    let blue = png(128, 128, |x, y| {
        if x < 4 && y < 4 {
            [0, 0, 255, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    let mut images = HashMap::new();
    images.insert("red".into(), red);
    images.insert("blue".into(), blue);
    let body = json!({"id": "root", "name": "Root", "assetId": "red", "parentId": null,
        "x": 40, "y": 40, "pivotX": 0, "pivotY": 0, "rotation": 90,
        "scaleX": 1, "scaleY": 1, "visible": true});
    let child = json!({"id": "child", "name": "Child", "assetId": "blue", "parentId": "root",
        "x": 10, "y": 0, "pivotX": 0, "pivotY": 0, "rotation": 0,
        "scaleX": 1, "scaleY": 1, "visible": true});
    let action = json!({"kind": "parts", "durationTicks": 4, "tracks": []});
    let project = json!({"parts": [body, child], "actions": [action]});
    let (frames, rendered) = bake_action(&project, &project["actions"][0], &images).unwrap();
    assert_eq!(frames.len(), 1);
    let image = decode_png(&rendered[0].1).unwrap();
    assert_eq!(image.get(37, 51), [0, 0, 255, 255]);
    assert_eq!(image.get(35, 43), [255, 0, 0, 255]);
}

#[test]
fn higher_draw_order_paints_later_and_opacity_scales_alpha() {
    let red = png(128, 128, |_, _| [255, 0, 0, 255]);
    let blue = png(128, 128, |_, _| [0, 0, 255, 255]);
    let mut images = HashMap::new();
    images.insert("red".into(), red);
    images.insert("blue".into(), blue);
    let back = json!({"id": "back", "name": "Back", "assetId": "red", "parentId": null,
        "x": 0, "y": 0, "pivotX": 0, "pivotY": 0, "rotation": 0,
        "scaleX": 1, "scaleY": 1, "visible": true, "opacity": 1, "zOrder": 0});
    let front = json!({"id": "front", "name": "Front", "assetId": "blue", "parentId": null,
        "x": 0, "y": 0, "pivotX": 0, "pivotY": 0, "rotation": 0,
        "scaleX": 1, "scaleY": 1, "visible": true, "opacity": 0.5, "zOrder": 1});
    let action = json!({"kind": "parts", "durationTicks": 1, "tracks": []});
    let project = json!({"parts": [front, back], "actions": [action]});
    let (frames, rendered) = bake_action(&project, &project["actions"][0], &images).unwrap();
    let pixel = decode_png(&rendered[0].1).unwrap().get(0, 0);
    assert_eq!(pixel[2], 128);
    assert!(pixel[0] > 100);
    let _ = frames;
}

#[test]
fn static_png_keeps_pixels_and_uses_the_legacy_duration() {
    let encoded = png(13, 7, |_, _| [12, 34, 56, 91]);
    let result = decode_media(&encoded, "still.png").unwrap();
    assert_eq!(result["format"], "png");
    assert_eq!(result["clips"].as_array().unwrap().len(), 0);
    assert_eq!(result["frames"][0]["durationMs"], 240.0);
    let image = decode_png(
        &crate::imagebuf::decode_data_url(result["frames"][0]["src"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!((image.width, image.height), (13, 7));
    assert_eq!(image.get(0, 0), [12, 34, 56, 91]);
}

fn frame_pixel(result: &serde_json::Value, frame: usize, x: u32, y: u32) -> [u8; 4] {
    let src = result["frames"][frame]["src"].as_str().unwrap();
    let bytes = crate::imagebuf::decode_data_url(src).unwrap();
    decode_png(&bytes).unwrap().get(x, y)
}

fn rgba_dot(width: usize, height: usize, x: usize, color: [u8; 4]) -> Vec<u8> {
    let mut pixels = vec![0; width * height * 4];
    let at = x * 4;
    pixels[at..at + 4].copy_from_slice(&color);
    pixels
}

fn gif_dots(middle: gif::DisposalMethod, repeat: Option<gif::Repeat>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = gif::Encoder::new(&mut bytes, 4, 2, &[]).unwrap();
    if let Some(repeat) = repeat {
        encoder.set_repeat(repeat).unwrap();
    }
    let frames = [
        (0usize, [255, 0, 0, 255], 4u16, gif::DisposalMethod::Keep),
        (1, [0, 255, 0, 255], 8, middle),
        (2, [0, 0, 255, 255], 12, gif::DisposalMethod::Keep),
    ];
    for (x, color, delay, dispose) in frames {
        let mut pixels = rgba_dot(4, 2, x, color);
        let mut frame = gif::Frame::from_rgba(4, 2, &mut pixels);
        frame.delay = delay;
        frame.dispose = dispose;
        encoder.write_frame(&frame).unwrap();
    }
    drop(encoder);
    bytes
}

fn apng_dots(poster: bool, plays: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 4, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(3, plays).unwrap();
        if poster {
            encoder.set_sep_def_img(true).unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        if poster {
            writer
                .write_image_data(&[255, 255, 0, 255].repeat(8))
                .unwrap();
        }
        let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
        let delays = [4u16, 8, 12];
        let disposes = [
            png::DisposeOp::None,
            png::DisposeOp::Background,
            png::DisposeOp::None,
        ];
        let blends = [png::BlendOp::Source, png::BlendOp::Over, png::BlendOp::Over];
        for index in 0..3 {
            writer.set_frame_delay(delays[index], 100).unwrap();
            writer.set_dispose_op(disposes[index]).unwrap();
            writer.set_blend_op(blends[index]).unwrap();
            writer
                .write_image_data(&rgba_dot(4, 2, index, colors[index]))
                .unwrap();
        }
        writer.finish().unwrap();
    }
    bytes
}

#[test]
fn gif_background_disposal_keeps_earlier_pixels_until_the_rect_is_cleared() {
    let result = decode_media(
        &gif_dots(gif::DisposalMethod::Background, Some(gif::Repeat::Infinite)),
        "delta.gif",
    )
    .unwrap();
    let durations: Vec<f64> = result["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["durationMs"].as_f64().unwrap())
        .collect();
    assert_eq!(durations, [40.0, 80.0, 120.0]);
    assert_eq!(result["clips"][0]["loop"], true);
    assert_eq!(frame_pixel(&result, 1, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame_pixel(&result, 1, 1, 0), [0, 255, 0, 255]);
    assert_eq!(frame_pixel(&result, 2, 0, 0)[3], 0);
    assert_eq!(frame_pixel(&result, 2, 1, 0)[3], 0);
    assert_eq!(frame_pixel(&result, 2, 2, 0), [0, 0, 255, 255]);
}

#[test]
fn gif_restore_previous_puts_back_the_canvas_under_the_disposed_frame() {
    let result = decode_media(
        &gif_dots(gif::DisposalMethod::Previous, Some(gif::Repeat::Infinite)),
        "restore.gif",
    )
    .unwrap();
    assert_eq!(frame_pixel(&result, 2, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame_pixel(&result, 2, 1, 0)[3], 0);
    assert_eq!(frame_pixel(&result, 2, 2, 0), [0, 0, 255, 255]);
}

#[test]
fn apng_blend_and_poster_match_the_pillow_cases() {
    let result = decode_media(&apng_dots(false, 0), "delta.png").unwrap();
    assert_eq!(result["format"], "apng");
    let durations: Vec<f64> = result["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["durationMs"].as_f64().unwrap())
        .collect();
    assert_eq!(durations, [40.0, 80.0, 120.0]);
    assert_eq!(frame_pixel(&result, 1, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame_pixel(&result, 1, 1, 0), [0, 255, 0, 255]);
    assert_eq!(frame_pixel(&result, 2, 0, 0)[3], 0);
    assert_eq!(frame_pixel(&result, 2, 2, 0), [0, 0, 255, 255]);

    let poster = decode_media(&apng_dots(true, 0), "poster.apng").unwrap();
    assert_eq!(poster["frames"].as_array().unwrap().len(), 3);
    assert_eq!(frame_pixel(&poster, 0, 0, 0), [255, 0, 0, 255]);
    let warnings = poster["warnings"].to_string();
    assert!(warnings.contains("封面"), "{warnings}");
}

fn ase_string(text: &str) -> Vec<u8> {
    let raw = text.as_bytes();
    let mut out = (raw.len() as u16).to_le_bytes().to_vec();
    out.extend_from_slice(raw);
    out
}

fn ase_chunk(kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = ((6 + payload.len()) as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn ase_layer(name: &str, flags: u16, kind: u16) -> Vec<u8> {
    let mut payload = Vec::new();
    for value in [flags, kind, 0, 0, 0, 0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(255);
    payload.extend_from_slice(&[0, 0, 0]);
    payload.extend(ase_string(name));
    ase_chunk(0x2004, &payload)
}

fn ase_cel(
    layer: u16,
    pixels: &[u8],
    width: u16,
    height: u16,
    z: i16,
    linked: Option<u16>,
) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&layer.to_le_bytes());
    payload.extend_from_slice(&0i16.to_le_bytes());
    payload.extend_from_slice(&0i16.to_le_bytes());
    payload.push(255);
    let cel_type: u16 = if linked.is_some() { 1 } else { 2 };
    payload.extend_from_slice(&cel_type.to_le_bytes());
    payload.extend_from_slice(&z.to_le_bytes());
    payload.extend_from_slice(&[0; 5]);
    if let Some(link) = linked {
        payload.extend_from_slice(&link.to_le_bytes());
    } else {
        payload.extend_from_slice(&width.to_le_bytes());
        payload.extend_from_slice(&height.to_le_bytes());
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, pixels).unwrap();
        payload.extend(encoder.finish().unwrap());
    }
    ase_chunk(0x2005, &payload)
}

fn ase_frame(chunks: &[Vec<u8>], duration: u16) -> Vec<u8> {
    let payload: Vec<u8> = chunks.iter().flat_map(|chunk| chunk.clone()).collect();
    let mut out = ((16 + payload.len()) as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&0xF1FAu16.to_le_bytes());
    out.extend_from_slice(&(chunks.len() as u16).to_le_bytes());
    out.extend_from_slice(&duration.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    out.extend(payload);
    out
}

fn ase_file(width: u16, height: u16, frames: &[Vec<u8>], depth: u16, transparent: u8) -> Vec<u8> {
    let body: Vec<u8> = frames.iter().flat_map(|frame| frame.clone()).collect();
    let mut header = vec![0; 128];
    header[4..6].copy_from_slice(&0xA5E0u16.to_le_bytes());
    header[6..8].copy_from_slice(&(frames.len() as u16).to_le_bytes());
    header[8..10].copy_from_slice(&width.to_le_bytes());
    header[10..12].copy_from_slice(&height.to_le_bytes());
    header[12..14].copy_from_slice(&depth.to_le_bytes());
    header[14..18].copy_from_slice(&1u32.to_le_bytes());
    header[18..20].copy_from_slice(&100u16.to_le_bytes());
    header[28] = transparent;
    header[34] = 1;
    header[35] = 1;
    let mut data = header;
    data.extend(body);
    let size = data.len() as u32;
    data[..4].copy_from_slice(&size.to_le_bytes());
    data
}

#[test]
fn aseprite_keeps_compressed_pixels_duration_slice_and_ping_pong() {
    let red = [255, 0, 0, 255, 0, 0, 0, 0, 0, 255, 0, 128, 0, 0, 255, 255];
    let mut tags = 1u16.to_le_bytes().to_vec();
    tags.extend_from_slice(&[0; 8]);
    tags.extend_from_slice(&0u16.to_le_bytes());
    tags.extend_from_slice(&0u16.to_le_bytes());
    tags.push(1);
    tags.extend_from_slice(&0u16.to_le_bytes());
    tags.extend_from_slice(&[0; 6]);
    tags.extend_from_slice(&[0; 3]);
    tags.push(0);
    tags.extend(ase_string("look"));
    let mut slice = Vec::new();
    slice.extend_from_slice(&1u32.to_le_bytes());
    slice.extend_from_slice(&2u32.to_le_bytes());
    slice.extend_from_slice(&0u32.to_le_bytes());
    slice.extend(ase_string("head"));
    for value in [0i32, 0, 0] {
        slice.extend_from_slice(&value.to_le_bytes());
    }
    slice.extend_from_slice(&2u32.to_le_bytes());
    slice.extend_from_slice(&1u32.to_le_bytes());
    slice.extend_from_slice(&1i32.to_le_bytes());
    slice.extend_from_slice(&0i32.to_le_bytes());
    let frame = ase_frame(
        &[
            ase_layer("body", 1, 0),
            ase_cel(0, &red, 2, 2, 0, None),
            ase_chunk(0x2018, &tags),
            ase_chunk(0x2022, &slice),
        ],
        80,
    );
    let result = decode_media(&ase_file(2, 2, &[frame], 32, 0), "pet.aseprite").unwrap();
    assert_eq!(frame_pixel(&result, 0, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame_pixel(&result, 0, 1, 0), [0, 0, 0, 0]);
    assert_eq!(frame_pixel(&result, 0, 0, 1), [0, 255, 0, 128]);
    assert_eq!(frame_pixel(&result, 0, 1, 1), [0, 0, 255, 255]);
    assert_eq!(result["frames"][0]["durationMs"], 80.0);
    assert_eq!(result["clips"][0]["name"], "look");
    assert_eq!(result["slices"][0]["pivotX"], 1);

    let pixel = [10, 20, 30, 255];
    let first = ase_frame(
        &[ase_layer("only", 1, 0), ase_cel(0, &pixel, 1, 1, 0, None)],
        40,
    );
    let linked = ase_frame(&[ase_cel(0, &[], 0, 0, 0, Some(0))], 40);
    let mut bob = 1u16.to_le_bytes().to_vec();
    bob.extend_from_slice(&[0; 8]);
    bob.extend_from_slice(&0u16.to_le_bytes());
    bob.extend_from_slice(&2u16.to_le_bytes());
    bob.push(2);
    bob.extend_from_slice(&0u16.to_le_bytes());
    bob.extend_from_slice(&[0; 10]);
    bob.extend(ase_string("bob"));
    let third = ase_frame(
        &[ase_cel(0, &[], 0, 0, 0, Some(0)), ase_chunk(0x2018, &bob)],
        40,
    );
    let result = decode_media(&ase_file(1, 1, &[first, linked, third], 32, 0), "bob.ase").unwrap();
    assert_eq!(frame_pixel(&result, 1, 0, 0), [10, 20, 30, 255]);
    assert_eq!(frame_pixel(&result, 2, 0, 0), [10, 20, 30, 255]);
    assert_eq!(result["clips"][0]["indices"], json!([0, 1, 2, 1]));
}

#[test]
fn piskel_drops_hidden_frames_and_keeps_a_horizontal_strip() {
    let back = png(2, 1, |x, _| {
        if x == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        }
    });
    let front = png(2, 1, |x, _| {
        if x == 0 {
            [0, 0, 255, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    let back_url = crate::imagebuf::data_url(&back);
    let front_url = crate::imagebuf::data_url(&front);
    let document = json!({
        "modelVersion": 2,
        "piskel": {
            "name": "pet",
            "fps": 10,
            "width": 1,
            "height": 1,
            "hiddenFrames": [1, null],
            "layers": [
                json!({"name": "back", "opacity": 1, "frameCount": 2, "base64PNG": back_url}).to_string(),
                json!({"name": "front", "opacity": 0.5, "frameCount": 2, "chunks": [{"layout": [[0], [1]], "base64PNG": front_url}]}).to_string(),
            ]
        }
    });
    let result = decode_media(document.to_string().as_bytes(), "pet.piskel").unwrap();
    assert_eq!(result["frames"].as_array().unwrap().len(), 1);
    assert_eq!(result["frames"][0]["durationMs"], 100.0);
    let painted = frame_pixel(&result, 0, 0, 0);
    assert_eq!(painted[2], 128);
    assert!(painted[0] > 100);

    let strip = png(3, 1, |x, _| {
        [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]][x as usize]
    });
    let strip_doc = json!({
        "modelVersion": 2,
        "piskel": {
            "name": "strip", "fps": 12, "width": 1, "height": 1,
            "layers": [json!({"name": "ink", "opacity": 1, "frameCount": 3, "base64PNG": crate::imagebuf::data_url(&strip)}).to_string()]
        }
    });
    let result = decode_media(strip_doc.to_string().as_bytes(), "strip.piskel").unwrap();
    assert_eq!(frame_pixel(&result, 0, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame_pixel(&result, 1, 0, 0), [0, 255, 0, 255]);
    assert_eq!(frame_pixel(&result, 2, 0, 0), [0, 0, 255, 255]);
}

#[test]
fn pixelorama_paints_later_layers_and_skips_hidden_and_tiles() {
    let document = json!({
        "pxo_version": 5, "size_x": 2, "size_y": 1, "color_mode": 5, "fps": 10,
        "layers": [
            {"name": "back", "type": 0, "visible": true, "opacity": 1, "blend_mode": 0},
            {"name": "front", "type": 0, "visible": true, "opacity": 1, "blend_mode": 4},
            {"name": "guide", "type": 0, "visible": false, "opacity": 1, "blend_mode": 0},
            {"name": "tiles", "type": 3, "visible": true, "opacity": 1, "blend_mode": 0}
        ],
        "frames": [{"duration": 2, "cels": [
            {"opacity": 1, "z_index": 0, "offset": "(0, 0)", "image_size": "(2, 1)"},
            {"opacity": 1, "z_index": 0, "offset": "(1, 0)", "image_size": "(1, 1)"},
            {"opacity": 1, "z_index": 0, "offset": "(0, 0)", "image_size": "(2, 1)"},
            {"opacity": 1, "z_index": 0, "offset": "(0, 0)", "image_size": "(1, 1)"}
        ]}],
        "tags": [{"name": "walk", "from": 1, "to": 1}]
    });
    let images = [
        (
            "image_data/frames/1/layer_1",
            vec![255, 0, 0, 255, 255, 0, 0, 255],
        ),
        ("image_data/frames/1/layer_2", vec![0, 0, 255, 255]),
        (
            "image_data/frames/1/layer_3",
            vec![0, 255, 0, 255, 0, 255, 0, 255],
        ),
        ("image_data/frames/1/layer_4", vec![9, 9, 9, 255]),
    ];
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive.start_file("data.json", options).unwrap();
        std::io::Write::write_all(&mut archive, document.to_string().as_bytes()).unwrap();
        for (name, bytes) in images {
            archive.start_file(name, options).unwrap();
            std::io::Write::write_all(&mut archive, &bytes).unwrap();
        }
        archive.finish().unwrap();
    }
    let result = decode_media(cursor.get_ref(), "pet.pxo").unwrap();
    assert_eq!(frame_pixel(&result, 0, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame_pixel(&result, 0, 1, 0), [0, 0, 255, 255]);
    assert_eq!(result["frames"][0]["durationMs"], 200.0);
    assert_eq!(result["clips"][0]["name"], "walk");
    let warnings = result["warnings"].to_string();
    assert!(warnings.contains("瓦片"), "{warnings}");
    assert!(warnings.contains("混合"), "{warnings}");
    assert!(decode_media(b"not-a-zip", "old.pxo").is_err());
}
