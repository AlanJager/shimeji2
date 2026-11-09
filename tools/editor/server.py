#!/usr/bin/env python3
"""Local editor. Serves only its UI and bundled shime; parts export uses Pillow.

Run: python3 tools/editor/server.py --open
The browser owns projects; this server never writes them or personal settings.
"""

import argparse
import base64
import binascii
import io
import json
from pathlib import Path
import re
import struct
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit, parse_qs
import webbrowser
import xml.etree.ElementTree as ET
import zipfile
import threading

from parts import bake_action, validate_parts
import catalog
from dance import standing_dance
from media import decode_media, decode_atlas

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SHIME = ROOT / "assets/packs/shime"
MAX_BODY = 24 * 1024 * 1024
PNG_PREFIX = "data:image/png;base64,"
NS = "{http://www.group-finity.com/Mascot}"
EVENTS = {"click", "pet", "enter", "drag", "release", "idle"}
MEDIA_SLOTS = threading.BoundedSemaphore(2)
STATIC = {"/": "index.html", "/index.html": "index.html", "/editor.css": "editor.css",
          "/editor.mjs": "editor.mjs", "/model.mjs": "model.mjs", "/catalog.mjs": "catalog.mjs", "/catalog.css": "catalog.css"}
STATIC.update({"/" + name: name for name in (
    "authoring-timeline.mjs", "authoring-timeline.css", "timeline-ops.mjs",
    "sequence-import.mjs", "sequence-import.css", "sequence-model.mjs",
    "authoring-preview.mjs", "authoring-preview.css", "preview-model.mjs",
    "vrm-model.mjs", "vrm-motion.mjs", "vrm-scene.mjs", "vrm-studio.mjs", "vrm-studio.css", "vrm-integration.mjs",
    "vendor/vrm-libs.mjs", "vendor/vrm-libs.mjs.LEGAL.txt", "vendor/LICENSES.txt", "vendor/manifest.json",
    "media-source.mjs", "media-timing.mjs", "media-integration.mjs",
    "asset-library.mjs", "asset-library.css", "asset-library-model.mjs",
)})


def demo_project():
    """Read real upstream poses and timing, without changing upstream files."""
    document = ET.parse(SHIME / "conf/動作.xml")
    definitions = {a.attrib["名前"]: a for a in document.iter(f"{NS}動作") if "名前" in a.attrib}
    actions = []
    for ident, name, source, loop in [
        ("idle", "站着发呆", "立つ", True),
        ("walk", "轻快走路", "歩く", True),
        ("look", "抬头看看", "座って首が回る", False),
        ("sit", "晃晃小脚", "足をぶらぶらさせる", True),
        ("sleep", "趴下休息", "寝そべる", True),
    ]:
        frames = []
        animation = definitions[source].find(f"{NS}アニメーション")
        for index, pose in enumerate(animation.findall(f"{NS}ポーズ")):
            x, y = map(int, pose.attrib["基準座標"].split(","))
            frames.append({"id": f"{ident}-{index}",
                           "assetId": Path(pose.attrib["画像"]).stem,
                           "ticks": int(pose.attrib["長さ"]), "x": 64 - x, "y": 128 - y})
        actions.append({"id": ident, "name": name, "loop": loop, "frames": frames})
    project = {
        "version": 2, "name": "白色 shime · 我的动作实验", "defaultActionId": "idle",
        "assets": [{"id": f"shime{i}", "name": f"shime{i}.png",
                    "src": f"/assets/shime{i}.png"} for i in range(1, 47)],
        "actions": actions,
        "regions": [{"id": "head", "name": "头部", "x": 24, "y": 4, "width": 80, "height": 68},
                    {"id": "body", "name": "身体", "x": 43, "y": 80, "width": 42, "height": 45}],
        "rules": [
            {"id": "pet-head", "name": "摸摸头，抬头看看", "event": "pet", "regionId": "head",
             "actionId": "look", "fromActionId": "*", "cooldown": 50, "priority": 10,
             "enabled": True, "returnToDefault": True},
            {"id": "click-body", "name": "点一下，晃晃脚", "event": "click", "regionId": "body",
             "actionId": "sit", "fromActionId": "*", "cooldown": 25, "priority": 5,
             "enabled": True, "returnToDefault": True},
            {"id": "idle-sleep", "name": "安静一会儿就休息", "event": "idle", "regionId": "*",
             "actionId": "sleep", "fromActionId": "idle", "cooldown": 125, "priority": 1,
             "enabled": True, "returnToDefault": False},
        ],
    }
    # Optional worked example: split existing art at a fixed horizontal line.
    # These are editable copies in JSON, never replacements of upstream files.
    try:
        from PIL import Image
        source = Image.open(SHIME / "img/shime1.png").convert("RGBA")
        for ident, name, box in [("demo-head-png", "头部.png", (0, 0, 128, 74)),
                                 ("demo-body-png", "身体.png", (0, 74, 128, 128))]:
            image = Image.new("RGBA", (128, 128))
            image.paste(source.crop(box), (box[0], box[1]))
            encoded = io.BytesIO()
            image.save(encoded, format="PNG")
            project["assets"].append({"id": ident, "name": name,
                                      "src": PNG_PREFIX + base64.b64encode(encoded.getvalue()).decode()})
        body = {"id": "demo-body", "name": "身体", "assetId": "demo-body-png", "parentId": None,
                "x": 64, "y": 128, "pivotX": 64, "pivotY": 128, "rotation": 0,
                "scaleX": 1, "scaleY": 1, "visible": True}
        head = {**body, "id": "demo-head", "name": "头部", "assetId": "demo-head-png", "parentId": "demo-body",
                "x": 0, "y": -55, "pivotY": 73}
        project["parts"] = [body, head]
        project["actions"].append(standing_dance(project))
        pose = {field: body[field] for field in ("x", "y", "rotation", "scaleX", "scaleY", "visible", "assetId")}
        project["actions"].append({"id": "breathe", "name": "部件 · 轻轻呼吸", "kind": "parts", "loop": True,
                                   "durationTicks": 50, "frames": [{"id": "breathe-0", "assetId": "shime1", "ticks": 50, "x": 0, "y": 0}],
                                   "tracks": [{"partId": "demo-body", "keys": [
                                       {**pose, "tick": 0}, {**pose, "tick": 25, "scaleY": 1.025}, {**pose, "tick": 50}]}]})
    except ImportError:
        project["parts"] = []
    return project


def dance_preset():
    project = demo_project()
    if not project["parts"]:
        raise ValueError("站立舞蹈需要 Pillow，请安装后重新启动编辑器。")
    action = next(action for action in project["actions"] if action["id"] == "standing-dance")
    frames, images = bake_action(project, action, {asset["id"]: asset_bytes(asset) for asset in project["assets"]})
    return {"assets": [{"id": ident, "name": "舞蹈帧 " + str(index + 1),
                         "src": PNG_PREFIX + base64.b64encode(data).decode()}
                        for index, (ident, data) in enumerate(images.items())],
            "action": {"id": "standing-dance", "name": "站着跳舞", "kind": "frames", "loop": True, "frames": frames}}


def integer(value, low, high):
    if type(value) is not int or not low <= value <= high:
        raise ValueError(f"数值需要在 {low}–{high} 之间")
    return value


def identifier(value):
    if not isinstance(value, str) or not re.fullmatch(r"[a-zA-Z0-9_-]{1,64}", value):
        raise ValueError("无效的项目对象 ID")
    return value


def unique(items, limit):
    if not isinstance(items, list) or not 1 <= len(items) <= limit:
        raise ValueError(f"列表需要包含 1–{limit} 个对象")
    ids = [identifier(item["id"]) for item in items]
    if len(ids) != len(set(ids)):
        raise ValueError("项目包含重复 ID")
    return set(ids)


def asset_bytes(asset):
    src = asset["src"]
    if re.fullmatch(r"/assets/shime([1-9]|[1-3][0-9]|4[0-6])\.png", src):
        return (SHIME / "img" / src.rsplit("/", 1)[1]).read_bytes()
    if not src.startswith(PNG_PREFIX) or len(src) > 2_800_000:
        raise ValueError("图片需要是内置素材或不大于 2 MB 的 PNG")
    try:
        data = base64.b64decode(src[len(PNG_PREFIX):], validate=True)
    except (binascii.Error, ValueError) as error:
        raise ValueError("PNG 数据无效") from error
    if len(data) < 33 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        raise ValueError("PNG 文件头无效")
    width, height = struct.unpack(">II", data[16:24])
    if (width, height) != (128, 128):
        raise ValueError("导入图片需要由编辑器归一化到 128 × 128")
    return data


def validate_project(project):
    if type(project.get("version")) is not int or project["version"] not in (1, 2):
        raise ValueError("不支持的工程版本")
    if not isinstance(project.get("name"), str) or not 1 <= len(project["name"]) <= 100:
        raise ValueError("工程名称需要包含 1–100 个字符")
    assets = unique(project["assets"], 256)
    actions = unique(project["actions"], 64)
    if project["defaultActionId"] not in actions:
        raise ValueError("默认动作不存在")
    for asset in project["assets"]:
        asset_bytes(asset)
    groups = project.get("assetGroups", [])
    if not isinstance(groups, list) or len(groups) > 256:
        raise ValueError("素材组数量超过 256 或格式无效")
    if groups:
        unique(groups, 256)
    def group_text(value, limit):
        return isinstance(value, str) and bool(value.strip()) and len(value) <= limit and not re.search(r"[\x00-\x1f\x7f]", value)
    for group in groups:
        if not group_text(group.get("name"), 80):
            raise ValueError("素材组名称需要包含 1–80 个字符")
        frames = group.get("frames")
        if not isinstance(frames, list) or not 1 <= len(frames) <= 256:
            raise ValueError("素材组需要包含 1–256 帧")
        for frame in frames:
            if not isinstance(frame, dict):
                raise ValueError("素材组帧格式无效")
            if frame.get("assetId") not in assets:
                raise ValueError("素材组引用了不存在的图片")
            integer(frame["ticks"], 1, 1500)
        if "source" in group:
            source = group["source"]
            if not isinstance(source, dict) or not group_text(source.get("format"), 80):
                raise ValueError("素材组来源格式无效")
            names = source.get("fileNames")
            if not isinstance(names, list) or len(names) > 256 or any(not group_text(name, 256) for name in names):
                raise ValueError("素材组来源文件名无效")
    validate_parts(project, assets)
    for action in project["actions"]:
        if not isinstance(action["name"], str) or not action["name"].strip() or not 1 <= len(action["name"]) <= 80:
            raise ValueError("动作名称需要包含 1–80 个字符")
        if type(action["loop"]) is not bool:
            raise ValueError("循环选项需要为布尔值")
        unique(action["frames"], 256)
        for frame in action["frames"]:
            if frame["assetId"] not in assets:
                raise ValueError("动画引用了不存在的图片")
            integer(frame["ticks"], 1, 1500)
            integer(frame["x"], -128, 128)
            integer(frame["y"], -128, 128)
    regions = set()
    if project["regions"]:
        regions = unique(project["regions"], 32)
    for region in project["regions"]:
        if not isinstance(region["name"], str) or not region["name"].strip() or not 1 <= len(region["name"]) <= 80:
            raise ValueError("触摸区域名称需要包含 1–80 个字符")
        for key in ("x", "y"):
            integer(region[key], 0, 127)
        for key in ("width", "height"):
            integer(region[key], 1, 128)
        if region["x"] + region["width"] > 128 or region["y"] + region["height"] > 128:
            raise ValueError("触摸区域超出了画布")
    if project["rules"]:
        unique(project["rules"], 128)
    for rule in project["rules"]:
        if not isinstance(rule["name"], str) or not rule["name"].strip() or not 1 <= len(rule["name"]) <= 80:
            raise ValueError("互动规则名称需要包含 1–80 个字符")
        if rule["event"] not in EVENTS or rule["actionId"] not in actions:
            raise ValueError("互动规则引用无效")
        if rule["regionId"] not in regions | {"*"} or rule["fromActionId"] not in actions | {"*"}:
            raise ValueError("互动条件引用无效")
        integer(rule["cooldown"], 0, 1500)
        integer(rule["priority"], 0, 100)
        if type(rule["enabled"]) is not bool or type(rule["returnToDefault"]) is not bool:
            raise ValueError("规则开关需要为布尔值")


def export_pack(project):
    """Export source, classic baked animation and the executable runtime manifest."""
    validate_project(project)
    source_bytes = {asset["id"]: asset_bytes(asset) for asset in project["assets"]}
    playback = {}
    baked_images = {}
    for action in project["actions"]:
        if action.get("kind") == "parts":
            frames, images = bake_action(project, action, source_bytes)
            if source_bytes.keys() & images.keys():
                raise ValueError("部件素材 ID 与导出动画 ID 冲突")
            baked_images.update(images)
            playback[action["id"]] = frames
        else:
            playback[action["id"]] = action["frames"]
    root = ET.Element("Mascot")
    action_list = ET.SubElement(root, "ActionList")
    behavior_root = ET.Element("Mascot")
    behavior_list = ET.SubElement(behavior_root, "BehaviorList")
    default = "Editor_" + project["defaultActionId"]
    first = playback[project["defaultActionId"]][0]

    def add_action(name, frames, loop=False, builtin=None):
        attrs = {"Name": name, "Type": "Embedded" if builtin else "Stay"}
        if builtin:
            attrs["Class"] = "com.group_finity.mascot.action." + builtin
        else:
            attrs["Duration"] = str(sum(f["ticks"] for f in frames))
        action = ET.SubElement(action_list, "Action", attrs)
        animation = ET.SubElement(action, "Animation")
        for frame in frames:
            ET.SubElement(animation, "Pose", {
                "Image": "/" + frame["assetId"] + ".png",
                "ImageAnchor": f"{64 - frame['x']},{128 - frame['y']}",
                "Velocity": "0,0", "Duration": str(frame["ticks"]),
            })

    for name, builtin in [("ChaseMouse", None), ("Fall", "Fall"),
                          ("Dragged", "Dragged"), ("Thrown", "Fall")]:
        add_action(name, [first], builtin=builtin)
        behavior = ET.SubElement(behavior_list, "Behavior", {"Name": name, "Frequency": "0"})
        next_list = ET.SubElement(behavior, "NextBehaviorList", {"Add": "false"})
        ET.SubElement(next_list, "BehaviorReference", {"Name": default, "Frequency": "1"})
    for action in project["actions"]:
        name = "Editor_" + action["id"]
        add_action(name, playback[action["id"]], action["loop"])
        attrs = {
            "Name": name, "Frequency": "1" if name == default else "0",
        }
        if name == default:
            attrs["Condition"] = "#{mascot.environment.floor.isOn(mascot.anchor) || mascot.environment.activeIE.topBorder.isOn(mascot.anchor)}"
        behavior = ET.SubElement(behavior_list, "Behavior", attrs)
        next_list = ET.SubElement(behavior, "NextBehaviorList", {"Add": "false"})
        ET.SubElement(next_list, "BehaviorReference", {
            "Name": name if action["loop"] else default, "Frequency": "1",
        })
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("conf/actions.xml", ET.tostring(root, encoding="utf-8", xml_declaration=True))
        archive.writestr("conf/behaviors.xml", ET.tostring(behavior_root, encoding="utf-8", xml_declaration=True))
        for ident, data in {**source_bytes, **baked_images}.items():
            archive.writestr(f"img/{ident}.png", data)
        archive.writestr("editor-project.json", json.dumps(project, ensure_ascii=False, indent=2))
        runtime = {"version": 1, "defaultActionId": project["defaultActionId"],
                   "actions": [{"id": action["id"], "name": action["name"], "loop": action["loop"],
                                "durationTicks": sum(frame["ticks"] for frame in playback[action["id"]])}
                               for action in project["actions"]],
                   "regions": project["regions"], "rules": project["rules"]}
        archive.writestr("editor-runtime.json", json.dumps(runtime, ensure_ascii=False, indent=2))
        if project.get("source"):
            archive.writestr("SOURCE.txt", json.dumps(project["source"], ensure_ascii=False, indent=2))
        archive.writestr("EDITOR-README.txt", (
            "shimeji2 动作编辑器原型\n\n"
            "ZIP 可导入 shimeji2；默认动作、动画帧、时长和偏移写入经典 XML。\n"
            "多部件动画在导出时按 40ms tick 合成为固定画布帧；原始部件与关键帧保存在工程中。\n"
            "VRM 动作已烘焙为 128×128 透明序列帧；原始 VRM/VRMA 请在 3D 工作室单独保存工程。\n"
            "动图与图集已还原为 PNG 序列；素材组、来源和组内时长保存在 editor-project.json 中。\n"
            "editor-runtime.json 由带编辑器支持的 shimeji2 执行，包含触摸区域和互动规则。\n"
            "请使用包含编辑器运行时的新构建；旧构建只能播放动画，不能执行新规则。\n"
            "要继续编辑，请在编辑器中打开 editor-project.json。\n"
            "内置 shime 素材来源与许可见下列文件；用户导入素材的许可由用户确认。\n"
        ))
        if any(asset["src"].startswith("/assets/shime") for asset in project["assets"]):
            for name in ("LICENSE.txt", "ATTRIBUTION.md"):
                archive.writestr("bundled-shime-" + name, (SHIME / name).read_bytes())
    return out.getvalue()


class Handler(BaseHTTPRequestHandler):
    def media_request(self, route):
        if not MEDIA_SLOTS.acquire(blocking=False):
            self.respond(429, json.dumps({"error": "当前有素材正在解码，请稍后重试"}, ensure_ascii=False).encode(), "application/json; charset=utf-8")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 16 * 1024 * 1024:
                raise ValueError("单次素材请求需在 16 MiB 以内")
            self.connection.settimeout(15)
            body = self.rfile.read(length)
            if len(body) != length:
                raise ValueError("素材数据未完整接收")
            if route == "/api/media/decode":
                names = parse_qs(urlsplit(self.path).query, max_num_fields=4).get("name", [])
                if len(names) != 1 or len(names[0]) > 1024:
                    raise ValueError("请选择一个具名素材文件")
                result = decode_media(body, names[0])
            else:
                data = json.loads(body)
                encoded = data["sheetBase64"]
                if not isinstance(encoded, str) or len(encoded) > 12 * 1024 * 1024:
                    raise ValueError("图集图片过大或编码无效")
                sheet = base64.b64decode(encoded, validate=True)
                result = decode_atlas(data["metadata"], sheet, data["sheetName"], data["atlasName"])
            response = json.dumps(result, ensure_ascii=False).encode()
            if len(response) > MAX_BODY:
                raise ValueError("解码后的素材超过 24 MiB，请减少帧数或尺寸")
            self.respond(200, response, "application/json; charset=utf-8")
        except (ValueError, KeyError, TypeError, AttributeError, OSError, zipfile.BadZipFile, EOFError, RecursionError, OverflowError) as error:
            self.respond(400, json.dumps({"error": str(error)}, ensure_ascii=False).encode(), "application/json; charset=utf-8")
        finally:
            MEDIA_SLOTS.release()

    def respond(self, status, body, content_type, immutable=False):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "private, max-age=86400, immutable" if immutable else "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy", "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self' blob:; frame-ancestors 'none'")
        self.end_headers()
        self.wfile.write(body)

    def local_request(self):
        host = self.headers.get("Host", "")
        allowed = {f"127.0.0.1:{self.server.server_port}", f"localhost:{self.server.server_port}"}
        origin = self.headers.get("Origin")
        if host not in allowed or (origin is not None and origin != f"http://{host}"):
            self.respond(403, b"Local editor requests only", "text/plain")
            return False
        return True

    def do_GET(self):
        if not self.local_request():
            return
        route = urlsplit(self.path).path
        if route == "/api/demo":
            self.respond(200, json.dumps(demo_project(), ensure_ascii=False).encode(), "application/json; charset=utf-8")
        elif route == "/api/catalog":
            self.respond(200, json.dumps(catalog.read_catalog(), ensure_ascii=False).encode(), "application/json; charset=utf-8")
        elif route == "/api/presets/standing-dance":
            try:
                self.respond(200, json.dumps(dance_preset(), ensure_ascii=False).encode(), "application/json; charset=utf-8")
            except ValueError as error:
                self.respond(400, json.dumps({"error": str(error)}, ensure_ascii=False).encode(), "application/json; charset=utf-8")
        elif route in STATIC:
            path = HERE / STATIC[route]
            mime = {".html": "text/html", ".css": "text/css", ".txt": "text/plain", ".json": "application/json"}.get(path.suffix, "text/javascript")
            self.respond(200, path.read_bytes(), mime + "; charset=utf-8")
        elif re.fullmatch(r"/assets/shime([1-9]|[1-3][0-9]|4[0-6])\.png", route):
            self.respond(200, (SHIME / "img" / route.rsplit("/", 1)[1]).read_bytes(), "image/png", immutable=True)
        else:
            self.respond(404, b"Not found", "text/plain")

    def do_POST(self):
        if not self.local_request():
            return
        route = urlsplit(self.path).path
        if route in ("/api/media/decode", "/api/media/atlas"):
            return self.media_request(route)
        if self.path not in ("/api/export", "/api/catalog/open"):
            self.respond(404, b"Not found", "text/plain")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= MAX_BODY:
                raise ValueError("工程大小需在 24 MB 以内")
            self.connection.settimeout(10)
            project = json.loads(self.rfile.read(length))
            if self.path == "/api/catalog/open":
                source = catalog.source_by_id(project.get("id"))
                archive = catalog.checked_archive(source)
                converted = catalog.editor_project(source, archive)
                validate_project(converted)
                self.respond(200, json.dumps(converted, ensure_ascii=False).encode(), "application/json; charset=utf-8")
            else:
                archive = export_pack(project)
                self.respond(200, archive, "application/zip")
        except (ValueError, KeyError, TypeError, AttributeError, OSError, zipfile.BadZipFile, ET.ParseError, RecursionError, OverflowError) as error:
            self.respond(400, json.dumps({"error": str(error)}, ensure_ascii=False).encode(), "application/json; charset=utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=4317)
    parser.add_argument("--open", action="store_true")
    args = parser.parse_args()
    try:
        server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    except OSError as error:
        parser.exit(1, f"无法启动编辑器：{error}。可用 --port 指定其他端口。\n")
    url = f"http://127.0.0.1:{server.server_port}"
    print(f"shimeji2 动作编辑器 → {url}\nCtrl+C 退出。工程保存在浏览器中，可另存 JSON。", flush=True)
    if args.open:
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
