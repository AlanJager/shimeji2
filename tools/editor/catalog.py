"""Metadata-only catalog and opt-in import of pinned classic animation clips.

Native app import retains original behavior XML. Editor conversion deliberately
extracts pose clips, not expressions or the original behavior state machine.
"""

import base64
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import time
import unicodedata
from urllib.parse import urlsplit
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(os.environ["SHIMEJI_EDITOR_ROOT"]) if os.environ.get("SHIMEJI_EDITOR_ROOT") else Path(__file__).resolve().parents[2]
CATALOG = ROOT / "assets/catalog/sources.json"
CACHE = ROOT / "output/material-catalog/cache"
MAX_ARCHIVE = 64 * 1024 * 1024
MAX_EXPANDED = 200 * 1024 * 1024
ID = re.compile(r"[A-Za-z0-9_-]{1,64}")
SHA = re.compile(r"[0-9a-f]{64}")


def read_catalog():
    value = json.loads(CATALOG.read_text(encoding="utf-8"))
    if value.get("version") != 1 or not isinstance(value.get("sources"), list):
        raise ValueError("素材目录版本无效")
    return value


def source_by_id(ident):
    source = next((s for s in read_catalog()["sources"] if s["id"] == ident), None)
    if source is None:
        raise ValueError("目录中没有这个素材来源")
    return source


def components(path, allow_empty=False):
    if path == "" and allow_empty:
        return []
    if not isinstance(path, str) or path.startswith(("/", "\\")) or "\\" in path or "\0" in path:
        raise ValueError("归档路径无效")
    values = path.rstrip("/").split("/")
    if len(values) > 16 or any(v in ("", ".", "..") or ":" in v for v in values):
        raise ValueError("归档路径越界或过深")
    return values


def importable(source):
    return source.get("kind") == "pack" and all(source.get(k) for k in ("archiveUrl", "archiveSha256")) and all(k in source for k in ("archivePrefix", "packPrefix"))


def checked_archive(source, cached_only=False):
    if not importable(source) or not ID.fullmatch(source["id"]) or not SHA.fullmatch(source["archiveSha256"]):
        raise ValueError("这个来源仅提供原站入口，不支持直接导入")
    url = urlsplit(source["archiveUrl"])
    if url.scheme != "https" or url.hostname not in {"codeload.github.com", "github.com", "raw.githubusercontent.com"} or url.username or url.password:
        raise ValueError("下载地址必须是目录内固定的 GitHub HTTPS 地址")
    components(source["archivePrefix"], allow_empty=True)
    components(source["packPrefix"], allow_empty=True)
    CACHE.mkdir(parents=True, exist_ok=True)
    dest = CACHE / (source["archiveSha256"] + ".zip")
    if dest.is_file() and dest.stat().st_size <= MAX_ARCHIVE:
        if hashlib.sha256(dest.read_bytes()).hexdigest() == source["archiveSha256"]:
            return dest
    if cached_only:
        raise ValueError("该固定归档尚未缓存在本机")
    started = time.monotonic()
    data = bytearray()
    request = Request(source["archiveUrl"], headers={"User-Agent": "shimeji2-local-editor/0.2"})
    with urlopen(request, timeout=20) as response:
        final = urlsplit(response.url)
        if final.scheme != "https" or final.hostname not in {"codeload.github.com", "github.com", "raw.githubusercontent.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"}:
            raise ValueError("下载重定向到了目录以外的来源")
        size = response.headers.get("Content-Length")
        if size and int(size) > MAX_ARCHIVE:
            raise ValueError("素材归档超过 64 MiB")
        while True:
            if time.monotonic() - started > 120:
                raise ValueError("素材下载超时，可到原站下载后使用本地导入")
            chunk = response.read(64 * 1024)
            if not chunk:
                break
            data.extend(chunk)
            if len(data) > MAX_ARCHIVE:
                raise ValueError("素材归档超过 64 MiB")
    if hashlib.sha256(data).hexdigest() != source["archiveSha256"]:
        raise ValueError("下载文件的 SHA256 与已核实目录不一致，请刷新来源核验")
    # Atomic replacement only of a content-addressed cache, never source files.
    import tempfile
    with tempfile.NamedTemporaryFile(dir=CACHE, delete=False, suffix=".tmp") as output:
        temporary = Path(output.name)
        output.write(data)
        output.flush()
    try:
        temporary.replace(dest)
    finally:
        temporary.unlink(missing_ok=True)
    return dest


def selected_files(archive, source):
    prefix = components(source["archivePrefix"], allow_empty=True) + components(source["packPrefix"], allow_empty=True)
    selected = {}
    seen = set()
    total = 0
    with zipfile.ZipFile(archive) as zipped:
        entries = zipped.infolist()
        if len(entries) > 5000:
            raise ValueError("归档超过 5000 个条目")
        for entry in entries:
            path = components(entry.filename)
            canonical = "/".join(path)
            if canonical in seen:
                raise ValueError("归档包含重复文件路径")
            seen.add(canonical)
            mode = entry.external_attr >> 16
            if stat.S_ISLNK(mode) or (stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)) or entry.flag_bits & 1:
                raise ValueError("归档包含链接、特殊文件或加密条目")
            total += entry.file_size
            if total > MAX_EXPANDED:
                raise ValueError("归档展开大小超过 200 MiB")
            if path[:len(prefix)] != prefix or len(path) <= len(prefix) or entry.is_dir():
                continue
            relative = "/".join(path[len(prefix):])
            if relative in selected:
                raise ValueError("归档包含重复文件路径")
            # Read only images/config; no program embedded in a fan pack runs.
            if relative.lower().endswith((".png", ".xml")):
                if entry.file_size > 8 * 1024 * 1024:
                    raise ValueError("单张素材或 XML 超过 8 MiB")
                data = zipped.read(entry)
                if len(data) != entry.file_size:
                    raise ValueError("归档实际文件大小不符")
                selected[relative] = data
    return selected


def editor_project(source, archive):
    from PIL import Image
    files = selected_files(archive, source)
    normalized = {unicodedata.normalize("NFC", name): name for name in files}

    def lookup(path):
        path = unicodedata.normalize("NFC", path.replace("\\", "/").lstrip("/"))
        if path in normalized:
            return normalized[path]
        matches = [name for norm, name in normalized.items() if norm.casefold() == path.casefold()]
        return matches[0] if len(matches) == 1 else None

    config = next((lookup(name) for name in ("conf/actions.xml", "conf/動作.xml", "actions.xml", "動作.xml") if lookup(name)), None)
    if config is None:
        raise ValueError("选定目录未找到可读取的经典动作 XML，请使用桌宠的本地导入")
    if len(files[config]) > 2 * 1024 * 1024:
        raise ValueError("动作 XML 超过 2 MiB")
    document = ET.fromstring(files[config])
    local = lambda element: element.tag.rsplit("}", 1)[-1]
    attr = lambda element, english, japanese: element.get(english, element.get(japanese))
    definitions = {}
    for element in document.iter():
        if local(element) in ("Action", "動作") and attr(element, "Name", "名前"):
            definitions[attr(element, "Name", "名前")] = element
    clips, used = [], {}
    images = {}
    image_roots = sorted({name.rsplit("/", 1)[0] for name in files
                          if name.lower().endswith(".png") and name.startswith("img/")})
    for name, element in definitions.items():
        animations = [a for a in element if local(a) in ("Animation", "アニメーション")]
        if not animations:
            continue
        animation = next((a for a in animations if attr(a, "Condition", "条件") is None), animations[0])
        frames = []
        for pose in animation:
            if local(pose) not in ("Pose", "ポーズ"):
                continue
            image = attr(pose, "Image", "画像") or ""
            relative = image.replace("\\", "/").lstrip("/")
            if any(part in ("", ".", "..") for part in relative.split("/")):
                continue
            image_path = next((lookup(root + "/" + relative) for root in ["img", *image_roots, ""]
                               if lookup((root + "/" if root else "") + relative)), None)
            duration = attr(pose, "Duration", "長さ") or ""
            anchor = attr(pose, "ImageAnchor", "基準座標") or "0,0"
            if not image_path or not re.fullmatch(r"[0-9]+", duration) or not re.fullmatch(r"-?[0-9]+\s*,\s*-?[0-9]+", anchor):
                continue
            ticks = int(duration)
            if not 1 <= ticks <= 1500:
                continue
            if image_path not in used:
                if len(used) >= 256:
                    continue
                decoded = Image.open(io.BytesIO(files[image_path]))
                if decoded.format != "PNG" or max(decoded.size) > 4096:
                    raise ValueError("源 PNG 尺寸超出限制")
                # Cap decoded memory before allocating pixels.
                if (sum(i.width * i.height for i in images.values()) + decoded.width * decoded.height) * 4 > 128 * 1024 * 1024:
                    raise ValueError("素材解码内存超过 128 MiB")
                images[image_path] = decoded.convert("RGBA")
                used[image_path] = f"source-{len(used) + 1}"
            ax, ay = map(int, anchor.split(","))
            frames.append({"id": f"frame-{len(frames) + 1}", "assetId": used[image_path], "ticks": ticks, "anchor": (ax, ay)})
            if len(frames) >= 256:
                break
        if frames:
            clips.append({"id": f"clip-{len(clips) + 1}", "name": name[:80], "kind": "frames", "loop": True, "frames": frames})
        if len(clips) >= 64:
            break
    if not clips:
        raise ValueError("未发现可提取的整帧动画；Live2D 或参数化动作不能作为 PNG 部件直接读取")
    scale = min(1.0, 128 / max(image.width for image in images.values()), 128 / max(image.height for image in images.values()))
    assets = []
    for path, image in images.items():
        canvas = Image.new("RGBA", (128, 128))
        size = (max(1, round(image.width * scale)), max(1, round(image.height * scale)))
        canvas.paste(image.resize(size, Image.Resampling.NEAREST), (0, 0))
        encoded = io.BytesIO()
        canvas.save(encoded, format="PNG")
        assets.append({"id": used[path], "name": Path(path).name,
                       "src": "data:image/png;base64," + base64.b64encode(encoded.getvalue()).decode()})
    for clip in clips:
        for frame in clip["frames"]:
            ax, ay = frame.pop("anchor")
            x, y = round(64 - ax * scale), round(128 - ay * scale)
            if not -128 <= x <= 128 or not -128 <= y <= 128:
                raise ValueError("源动作锚点超出当前编辑画布；请在桌宠素材目录中原样导入此包")
            frame.update(x=x, y=y)
    default = next((c["id"] for c in clips if re.search(r"stand|idle|立つ|立って|待机", c["name"], re.I)), clips[0]["id"])
    return {"version": 2, "name": (source["name"] + " · 动作提取")[:100], "defaultActionId": default,
            "assets": assets, "actions": clips, "parts": [], "regions": [], "rules": [],
            "source": {"id": source["id"], "url": source["url"], "revision": source.get("revision"),
                       "archiveSha256": source["archiveSha256"], "licenseNote": source["licenseNote"],
                       "conversion": "仅提取整帧动画片段；原包表达式、状态机和移动物理请通过桌宠素材目录原样导入。没有自动拆分人体部件。"}}
