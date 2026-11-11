use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use crate::catalog::{open_source, read_catalog};
use crate::error::{Result, err};
use crate::layout::Layout;
use crate::media::{decode_atlas, decode_media};
use crate::project::{asset_bytes, dance_preset, demo_project, export_pack, shime_name};

const MAX_BODY: usize = 24 * 1024 * 1024;
const MEDIA_BODY: usize = 16 * 1024 * 1024;

static MEDIA_SLOTS: Mutex<u8> = Mutex::new(0);

pub fn serve(port: u16, open: bool) -> Result<()> {
    let layout = Layout::discover()?;
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|error| err(error.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|error| err(error.to_string()))?
        .port();
    let url = format!("http://127.0.0.1:{port}");
    println!("shimeji2 动作编辑器 → {url}\nCtrl+C 退出。工程保存在浏览器中，可另存 JSON。");
    if open {
        open_browser(&url);
    }
    for connection in listener.incoming() {
        let Ok(stream) = connection else { continue };
        let layout = layout.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &layout, port);
        });
    }
    Ok(())
}

pub fn request_allowed(host: &str, origin: Option<&str>, port: u16) -> bool {
    let host = host.split(',').next().unwrap_or("").trim();
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return false;
    }
    origin.is_none_or(|origin| origin == format!("http://{host}"))
}

fn handle(mut stream: TcpStream, layout: &Layout, port: u16) -> Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let mut header = Vec::new();
    let mut byte = [0; 1];
    while header.len() < 64 * 1024 {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&header);
    let mut lines = text.split("\r\n");
    let Some(request) = lines.next() else {
        return Ok(());
    };
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let mut headers = std::collections::HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let host = headers.get("host").map(String::as_str).unwrap_or("");
    let origin = headers.get("origin").map(String::as_str);
    if !request_allowed(host, origin, port) {
        return respond(
            &mut stream,
            403,
            b"Editor requests must come from this site",
            "text/plain",
            false,
        );
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method == "GET" {
        return get(&mut stream, layout, path);
    }
    if method != "POST" {
        return respond(&mut stream, 404, b"Not found", "text/plain", false);
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = if path.starts_with("/api/media/") {
        MEDIA_BODY
    } else {
        MAX_BODY
    };
    if length == 0 || length > limit {
        let message = if path.starts_with("/api/media/") {
            "单次素材请求需在 16 MiB 以内"
        } else {
            "工程大小需在 24 MB 以内"
        };
        return respond(
            &mut stream,
            400,
            &json_error(message),
            "application/json; charset=utf-8",
            false,
        );
    }
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .map_err(|_| err("素材数据未完整接收"))?;
    if path == "/api/media/decode" || path == "/api/media/atlas" {
        return media(&mut stream, path, query, &body);
    }
    if path != "/api/export" && path != "/api/catalog/open" {
        return respond(&mut stream, 404, b"Not found", "text/plain", false);
    }
    let value: Value = serde_json::from_slice(&body).map_err(|error| err(error.to_string()))?;
    if path == "/api/catalog/open" {
        let ident = value.get("id").and_then(Value::as_str).unwrap_or("");
        match open_source(layout, ident) {
            Ok(project) => {
                let bytes = serde_json::to_vec(&project).unwrap_or_default();
                respond(
                    &mut stream,
                    200,
                    &bytes,
                    "application/json; charset=utf-8",
                    false,
                )
            }
            Err(error) => respond(
                &mut stream,
                400,
                &json_error(&error.0),
                "application/json; charset=utf-8",
                false,
            ),
        }
    } else {
        match export_pack(&layout.shime, &value) {
            Ok(bytes) => respond(&mut stream, 200, &bytes, "application/zip", false),
            Err(error) => respond(
                &mut stream,
                400,
                &json_error(&error.0),
                "application/json; charset=utf-8",
                false,
            ),
        }
    }
}

fn get(stream: &mut TcpStream, layout: &Layout, path: &str) -> Result<()> {
    match path {
        "/api/demo" => {
            let project = demo_project(&layout.shime)?;
            respond(
                stream,
                200,
                &serde_json::to_vec(&project).unwrap_or_default(),
                "application/json; charset=utf-8",
                false,
            )
        }
        "/api/catalog" => {
            let catalog = read_catalog(layout)?;
            respond(
                stream,
                200,
                &serde_json::to_vec(&catalog).unwrap_or_default(),
                "application/json; charset=utf-8",
                false,
            )
        }
        "/api/presets/standing-dance" => match dance_preset(&layout.shime) {
            Ok(preset) => respond(
                stream,
                200,
                &serde_json::to_vec(&preset).unwrap_or_default(),
                "application/json; charset=utf-8",
                false,
            ),
            Err(error) => respond(
                stream,
                400,
                &json_error(&error.0),
                "application/json; charset=utf-8",
                false,
            ),
        },
        _ => {
            if let Some(name) = shime_name(path) {
                let bytes = std::fs::read(layout.shime.join("img").join(name))
                    .map_err(|error| err(error.to_string()))?;
                return respond(stream, 200, &bytes, "image/png", true);
            }
            if let Some(file) = static_file(&layout.editor, path) {
                let bytes = std::fs::read(&file).map_err(|error| err(error.to_string()))?;
                let mime = match file.extension().and_then(|ext| ext.to_str()).unwrap_or("") {
                    "html" => "text/html; charset=utf-8",
                    "css" => "text/css; charset=utf-8",
                    "txt" => "text/plain; charset=utf-8",
                    "json" => "application/json; charset=utf-8",
                    "png" => "image/png",
                    _ => "text/javascript; charset=utf-8",
                };
                return respond(stream, 200, &bytes, mime, false);
            }
            respond(stream, 404, b"Not found", "text/plain", false)
        }
    }
}

fn media(stream: &mut TcpStream, path: &str, query: &str, body: &[u8]) -> Result<()> {
    let mut slots = MEDIA_SLOTS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if *slots >= 2 {
        drop(slots);
        return respond(
            stream,
            429,
            &json_error("当前有素材正在解码，请稍后重试"),
            "application/json; charset=utf-8",
            false,
        );
    }
    *slots += 1;
    drop(slots);
    let result = (|| {
        if path == "/api/media/decode" {
            let name = query_name(query)?;
            decode_media(body, &name)
        } else {
            let value: Value =
                serde_json::from_slice(body).map_err(|error| err(error.to_string()))?;
            let encoded = value
                .get("sheetBase64")
                .and_then(Value::as_str)
                .ok_or_else(|| err("图集图片过大或编码无效"))?;
            if encoded.len() > 12 * 1024 * 1024 {
                return Err(err("图集图片过大或编码无效"));
            }
            let sheet = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
                .map_err(|_| err("图集图片过大或编码无效"))?;
            decode_atlas(
                &value["metadata"],
                &sheet,
                value["sheetName"].as_str().unwrap_or(""),
                value["atlasName"].as_str().unwrap_or(""),
            )
        }
    })();
    *MEDIA_SLOTS
        .lock()
        .unwrap_or_else(|error| error.into_inner()) -= 1;
    match result {
        Ok(value) => {
            let bytes = serde_json::to_vec(&value).unwrap_or_default();
            if bytes.len() > MAX_BODY {
                respond(
                    stream,
                    400,
                    &json_error("解码后的素材超过 24 MiB，请减少帧数或尺寸"),
                    "application/json; charset=utf-8",
                    false,
                )
            } else {
                respond(
                    stream,
                    200,
                    &bytes,
                    "application/json; charset=utf-8",
                    false,
                )
            }
        }
        Err(error) => respond(
            stream,
            400,
            &json_error(&error.0),
            "application/json; charset=utf-8",
            false,
        ),
    }
}

fn query_name(query: &str) -> Result<String> {
    let mut names = Vec::new();
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "name" {
            names.push(percent_decode(value)?);
        }
    }
    if names.len() != 1 || names[0].chars().count() > 1024 {
        return Err(err("请选择一个具名素材文件"));
    }
    Ok(names.remove(0))
}

fn percent_decode(value: &str) -> Result<String> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|_| err("请选择一个具名素材文件"))?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| err("请选择一个具名素材文件"))?);
            index += 3;
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| err("请选择一个具名素材文件"))
}

fn static_file(editor: &Path, route: &str) -> Option<std::path::PathBuf> {
    let relative = if route == "/" {
        "index.html"
    } else {
        route.trim_start_matches('/')
    };
    if relative.contains("..") || relative.contains('\\') || relative.ends_with(".py") {
        return None;
    }
    let path = editor.join(relative);
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if !matches!(
        extension,
        "html" | "css" | "mjs" | "js" | "txt" | "json" | "png"
    ) || !path.is_file()
    {
        return None;
    }
    Some(path)
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    body: &[u8],
    content_type: &str,
    immutable: bool,
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let cache = if immutable {
        "private, max-age=86400, immutable"
    } else {
        "no-store"
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: {cache}\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self' blob:; frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).ok();
    stream.write_all(body).ok();
    Ok(())
}

fn json_error(message: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"error": message}))
        .unwrap_or_else(|_| b"{\"error\":\"error\"}".to_vec())
}

fn open_browser(url: &str) {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    let _ = command.spawn();
}

#[allow(dead_code)]
fn _asset(layout: &Layout) -> Result<Vec<u8>> {
    asset_bytes(&layout.shime, &json!({"src": "/assets/shime1.png"}))
}
