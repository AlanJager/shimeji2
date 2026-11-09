"""Bounded local raster/atlas preprocessing. Never extracts archives to disk.

Pillow's sequential seek/load performs GIF/APNG/WebP disposal and blend. Returned
PNGs are complete logical frames, deliberately not normalized to editor size.
"""

import base64
import io
import json
import math
from pathlib import PurePosixPath
import re
import stat
import struct
import unicodedata
import warnings
import zipfile
import zlib

from PIL import Image, ImageOps, UnidentifiedImageError

MAX_FILE_BYTES = 8 * 1024 * 1024
MAX_ZIP_BYTES = 16 * 1024 * 1024
MAX_JSON_BYTES = 1024 * 1024
MAX_PIXELS = 16 * 1024 * 1024
MAX_PNG_BYTES = 16 * 1024 * 1024
MAX_FRAMES = 256
MAX_EDGE = 4096
PNG_PREFIX = "data:image/png;base64,"
RASTERS = {".png", ".apng", ".gif", ".webp", ".jpg", ".jpeg"}


def _integer(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise ValueError(f"{name} 需要是 {low}–{high} 之间的整数。")
    return value


def _object(value, name):
    if not isinstance(value, dict):
        raise ValueError(f"{name} 格式无效。")
    return value


def _label(value, fallback="素材"):
    if not isinstance(value, str) or not value or len(value) > 1024 or any(unicodedata.category(c) == "Cc" for c in value):
        raise ValueError("素材名称无效。")
    return PurePosixPath(value.replace("\\", "/")).stem[:255] or fallback


def _basename(value):
    if (not isinstance(value, str) or not value or len(value) > 255
            or any(c in value for c in "/\\:") or value in (".", "..")
            or any(unicodedata.category(c) == "Cc" for c in value)):
        raise ValueError("图集 image 必须是所选图片的准确文件名，不能包含 URL 或路径。")
    return value


def _duration(value, default=100):
    if value is None:
        return default
    if type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 60_000:
        raise ValueError("源帧时长需要在 0–60000 毫秒以内；请先缩短超过 60 秒的源帧。")
    return value


class _Budget:
    def __init__(self):
        self.frames = 0
        self.pixels = 0
        self.png_bytes = 0

    def reserve(self, size, output=True):
        width, height = size
        _integer(width, 1, MAX_EDGE, "图片宽度")
        _integer(height, 1, MAX_EDGE, "图片高度")
        self.pixels += width * height
        self.frames += int(output)
        if self.pixels > MAX_PIXELS:
            raise ValueError("解码帧合计超过 16M 像素上限，请缩小图片或减少帧数。")
        if self.frames > MAX_FRAMES:
            raise ValueError("一次最多导入 256 帧。")

    def frame(self, image, name, duration):
        encoded = io.BytesIO()
        # Do not carry ICC/EXIF/source metadata into the editable frame PNG.
        clean = Image.frombytes("RGBA", image.size, image.tobytes())
        try:
            clean.save(encoded, format="PNG")
        finally:
            clean.close()
        data = encoded.getvalue()
        self.png_bytes += len(data)
        if self.png_bytes > MAX_PNG_BYTES:
            raise ValueError("解码后的 PNG 合计超过 16 MiB 上限。")
        return {"name": name, "src": PNG_PREFIX + base64.b64encode(data).decode("ascii"),
                "width": image.width, "height": image.height, "durationMs": duration}


def _png_chunks(data):
    """Verify chunk bounds/CRC, including IEND; load alone accepts missing IEND."""
    at, chunks, controls = 8, 0, 0
    canvas, declared, frame_data = None, None, False
    while at < len(data):
        chunks += 1
        if chunks > 4096 or at + 12 > len(data):
            raise ValueError("PNG chunk 数量或边界无效。")
        length = struct.unpack_from(">I", data, at)[0]
        end = at + 12 + length
        if end > len(data):
            raise ValueError("PNG chunk 被截断。")
        kind = data[at + 4:at + 8]
        expected = struct.unpack_from(">I", data, end - 4)[0]
        if zlib.crc32(memoryview(data)[at + 4:end - 4]) != expected:
            raise ValueError("PNG chunk 校验失败。")
        if chunks == 1:
            if kind != b"IHDR" or length != 13:
                raise ValueError("PNG 缺少首个 IHDR。")
            canvas = struct.unpack_from(">II", data, at + 8)
            for edge in canvas:
                _integer(edge, 1, MAX_EDGE, "PNG 尺寸")
        elif kind == b"IHDR":
            raise ValueError("PNG 包含重复 IHDR。")
        if kind == b"acTL":
            if length != 8 or declared is not None:
                raise ValueError("APNG acTL 长度无效。")
            declared = _integer(struct.unpack_from(">I", data, at + 8)[0], 1, MAX_FRAMES, "APNG 帧数")
        if kind == b"fcTL":
            if controls and not frame_data:
                raise ValueError("APNG 帧控制缺少对应位图数据。")
            controls += 1
            frame_data = False
            if length != 26 or declared is None or controls > MAX_FRAMES:
                raise ValueError("APNG 帧控制数量或长度无效。")
            width, height, x, y = struct.unpack_from(">IIII", data, at + 12)
            if not width or not height or x + width > canvas[0] or y + height > canvas[1]:
                raise ValueError("APNG 帧区域超出逻辑画布。")
        if kind == b"IDAT" and controls:
            if controls > 1:
                raise ValueError("APNG 后续帧必须使用 fdAT。")
            frame_data |= length > 0
        if kind == b"fdAT":
            if not controls or length < 4:
                raise ValueError("APNG fdAT 没有对应帧控制或数据被截断。")
            frame_data |= length > 4
        if kind == b"IEND":
            if length or end != len(data):
                raise ValueError("PNG IEND 或尾部数据无效。")
            if declared is not None and controls != declared:
                raise ValueError("APNG 声明帧数与完整帧控制数量不一致。")
            if declared is not None and not frame_data:
                raise ValueError("APNG 最后一帧缺少位图数据。")
            return
        at = end
    raise ValueError("PNG 缺少 IEND。")


def _gif_header(data):
    if len(data) < 14:
        raise ValueError("GIF 文件头被截断。")
    width, height = struct.unpack_from("<HH", data, 6)
    for edge in (width, height):
        _integer(edge, 1, MAX_EDGE, "GIF 尺寸")
    at = 13 + (3 * 2 ** ((data[10] & 7) + 1) if data[10] & 128 else 0)
    frames = 0
    def subblocks(offset):
        while offset < len(data):
            length = data[offset]
            offset += 1
            if not length:
                return offset
            offset += length
            if offset > len(data):
                break
        raise ValueError("GIF 数据块被截断。")
    while at < len(data):
        marker = data[at]
        at += 1
        if marker == 0x3b:
            if at != len(data) or not frames:
                raise ValueError("GIF 尾部或帧数无效。")
            return
        if marker == 0x21:
            if at >= len(data):
                break
            at = subblocks(at + 1)
        elif marker == 0x2c:
            if at + 9 > len(data):
                break
            x, y, fw, fh = struct.unpack_from("<HHHH", data, at)
            packed = data[at + 8]
            frames += 1
            if not fw or not fh or x + fw > width or y + fh > height or frames > MAX_FRAMES:
                raise ValueError("GIF 帧区域或帧数超出逻辑画布上限。")
            at += 9 + (3 * 2 ** ((packed & 7) + 1) if packed & 128 else 0)
            if at >= len(data):
                break
            at = subblocks(at + 1)  # LZW minimum code size, then sub-blocks.
        else:
            raise ValueError("GIF 数据块标记无效。")
    raise ValueError("GIF 文件被截断。")


def _webp_dimensions(data, start, end, kind):
    if kind == b"VP8 ":
        if end - start < 10 or data[start + 3:start + 6] != b"\x9d\x01\x2a":
            raise ValueError("WebP VP8 文件头无效。")
        return tuple(value & 0x3fff for value in struct.unpack_from("<HH", data, start + 6))
    if end - start < 5 or data[start] != 0x2f:
        raise ValueError("WebP VP8L 文件头无效。")
    bits = struct.unpack_from("<I", data, start + 1)[0]
    return 1 + (bits & 0x3fff), 1 + ((bits >> 14) & 0x3fff)


def _webp_frame(data, start, end, expected):
    found, chunks = False, 0
    while start < end:
        chunks += 1
        if start + 8 > end or chunks > 4:
            raise ValueError("WebP 动画位图 chunk 被截断。")
        kind, length = data[start:start + 4], struct.unpack_from("<I", data, start + 4)[0]
        finish = start + 8 + length
        if finish + length % 2 > end:
            raise ValueError("WebP 动画位图 chunk 越界。")
        if kind in (b"VP8 ", b"VP8L"):
            if found or _webp_dimensions(data, start + 8, finish, kind) != expected:
                raise ValueError("WebP 动画位图尺寸与帧区域不一致。")
            found = True
        elif kind != b"ALPH":
            raise ValueError("WebP 动画帧包含不支持的嵌套 chunk。")
        start = finish + length % 2
    if not found:
        raise ValueError("WebP 动画帧缺少位图。")


def _webp_header(data):
    at, frames, canvas, chunks = 12, 0, None, 0
    while at < len(data):
        chunks += 1
        if at + 8 > len(data) or chunks > 4096:
            raise ValueError("WebP chunk 边界或数量无效。")
        kind = data[at:at + 4]
        length = struct.unpack_from("<I", data, at + 4)[0]
        start, end = at + 8, at + 8 + length
        if end + length % 2 > len(data):
            raise ValueError("WebP chunk 被截断。")
        if kind == b"VP8X":
            if length != 10:
                raise ValueError("WebP VP8X 长度无效。")
            canvas = (1 + int.from_bytes(data[start + 4:start + 7], "little"), 1 + int.from_bytes(data[start + 7:start + 10], "little"))
        elif kind in (b"VP8 ", b"VP8L"):
            actual = _webp_dimensions(data, start, end, kind)
            if canvas and canvas != actual:
                raise ValueError("WebP 位图尺寸与逻辑画布不一致。")
            canvas = actual
        elif kind == b"ANMF":
            frames += 1
            if length < 16 or not canvas or frames > MAX_FRAMES:
                raise ValueError("WebP 动画帧控制无效或超过 256 帧。")
            values = [int.from_bytes(data[start + i:start + i + 3], "little") for i in (0, 3, 6, 9)]
            x, y, width, height = values[0] * 2, values[1] * 2, values[2] + 1, values[3] + 1
            if x + width > canvas[0] or y + height > canvas[1]:
                raise ValueError("WebP 动画帧超出逻辑画布。")
            _webp_frame(data, start + 16, end, (width, height))
        if canvas:
            for edge in canvas:
                _integer(edge, 1, MAX_EDGE, "WebP 尺寸")
        at = end + length % 2
    if not canvas:
        raise ValueError("WebP 缺少画布尺寸。")


def _signature(data, name):
    if not isinstance(data, bytes) or not 0 < len(data) <= MAX_FILE_BYTES:
        raise ValueError("单个图片需要在 8 MiB 以内。")
    suffix = PurePosixPath(name).suffix.lower()
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        kind, extensions = "PNG", {".png", ".apng"}
        _png_chunks(data)
    elif data.startswith((b"GIF87a", b"GIF89a")):
        kind, extensions = "GIF", {".gif"}
        if not data.endswith(b";"):
            raise ValueError("GIF 缺少结束标记。")
        _gif_header(data)
    elif data.startswith(b"\xff\xd8\xff"):
        kind, extensions = "JPEG", {".jpg", ".jpeg"}
    elif len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        kind, extensions = "WEBP", {".webp"}
        if struct.unpack_from("<I", data, 4)[0] + 8 != len(data):
            raise ValueError("WebP RIFF 长度无效。")
        _webp_header(data)
    else:
        raise ValueError("只支持签名有效的 PNG/APNG、GIF、WebP 或 JPEG。")
    if suffix not in extensions:
        raise ValueError("图片扩展名与实际格式不一致。")
    return kind


def _loop(info, kind, notices):
    count = info.get("loop")
    if count is None:
        return False
    _integer(count, 0, 2**32 - 1, "动画循环次数")
    if count == 0:
        return True
    if kind == "GIF" or count > 1:
        notices.append("源动画指定有限重复次数；桌宠只支持循环或单次，本次按单个周期导入。")
    return False


def _raster(data, name, budget):
    kind = _signature(data, name)
    notices, frames = [], []
    if kind == "JPEG":
        notices.append("JPEG 保留原有不透明背景，不会自动抠图。")
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("error", Image.DecompressionBombWarning)
            with Image.open(io.BytesIO(data), formats=[kind]) as source:
                if source.format != kind:
                    raise ValueError("图片解码格式与签名不一致。")
                size = source.size
                _integer(size[0], 1, MAX_EDGE, "图片宽度")
                _integer(size[1], 1, MAX_EDGE, "图片高度")
                initial = dict(source.info)
                apng = kind == "PNG" and source.get_format_mimetype() == "image/apng"
                poster = bool(initial.get("default_image")) if apng else False
                expected_frames = source.n_frames - int(poster) if kind in ("PNG", "WEBP") else None
                # PNG/WebP expose their frame count without GIF's full-stream scan.
                static = kind == "JPEG" or kind in ("PNG", "WEBP") and getattr(source, "n_frames", 1) == 1 and not apng
                if poster:
                    budget.reserve(size, output=False)
                    notices.append("APNG 默认静态封面已跳过，仅导入动画帧。")
                index = 1 if poster else 0
                while True:
                    try:
                        source.seek(index)
                    except EOFError:
                        break
                    # Reserve before convert/load; malicious large canvases must
                    # never reach the decoder. seek is sequential for disposal.
                    budget.reserve(source.size)
                    if source.size != size:
                        raise ValueError("动画帧改变了逻辑画布尺寸。")
                    oriented = ImageOps.exif_transpose(source) if static else None
                    rgba = (oriented or source).convert("RGBA")
                    try:
                        # WebP populates duration only when the current frame is loaded.
                        duration = 240 if static else _duration(source.info.get("duration"))
                        frames.append(budget.frame(rgba, f"{_label(name)}-{len(frames) + 1:03d}.png", duration))
                    finally:
                        rgba.close()
                        if oriented is not None:
                            oriented.close()
                    index += 1
                if not frames:
                    raise ValueError("图片中没有可导入的动画帧。")
                if expected_frames is not None and len(frames) != expected_frames:
                    raise ValueError(f"源动画声明 {expected_frames} 帧，但只完整解码 {len(frames)} 帧。")
                animated = not static and (len(frames) > 1 or apng or kind in ("GIF", "WEBP"))
                clips = [{"name": _label(name), "indices": list(range(len(frames))), "loop": _loop(initial, kind, notices)}] if animated else []
                return {"format": "apng" if apng else kind.lower(), "name": _label(name),
                        "frames": frames, "clips": clips, "warnings": notices}
    except (OSError, SyntaxError, UnidentifiedImageError, Image.DecompressionBombError, Image.DecompressionBombWarning) as error:
        raise ValueError(f"无法完整解码图片：{error}") from error


def _json(metadata):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("图集 JSON 包含重复字段。")
            result[key] = value
        return result
    if isinstance(metadata, (bytes, str)):
        if len(metadata) > MAX_JSON_BYTES:
            raise ValueError("图集 JSON 需要在 1 MiB 以内。")
        try:
            metadata = json.loads(metadata, object_pairs_hook=pairs, parse_constant=lambda _: (_ for _ in ()).throw(ValueError("JSON 包含非有限数值。")))
        except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
            raise ValueError("图集 JSON 数据损坏。") from error
    _object(metadata, "图集 JSON")
    try:
        encoded = json.dumps(metadata, ensure_ascii=False, allow_nan=False).encode("utf-8")
    except (TypeError, ValueError, RecursionError) as error:
        raise ValueError("图集 JSON 数据无效。") from error
    if len(encoded) > MAX_JSON_BYTES:
        raise ValueError("图集 JSON 需要在 1 MiB 以内。")
    return metadata


def _rect(value, name, allow_zero=False):
    value = _object(value, name)
    return tuple(_integer(value.get(key), 0 if key in ("x", "y") or allow_zero else 1, MAX_EDGE, f"{name}.{key}") for key in ("x", "y", "w", "h"))


def _atlas(metadata, sheet_bytes, sheet_name, atlas_name, budget):
    metadata = _json(metadata)
    sheet_name = _basename(sheet_name)
    meta = _object(metadata.get("meta"), "atlas.meta")
    if _basename(meta.get("image")) != sheet_name:
        raise ValueError("图集 image 与所选图片文件名不一致。")
    if _signature(sheet_bytes, sheet_name) != "PNG":
        raise ValueError("Aseprite 图集需要单张静态 PNG。")
    raw = metadata.get("frames")
    if isinstance(raw, dict):
        entries = [(name, value) for name, value in raw.items()]
    elif isinstance(raw, list):
        entries = [(entry.get("filename", f"frame-{index + 1}") if isinstance(entry, dict) else "", entry) for index, entry in enumerate(raw)]
    else:
        raise ValueError("图集 frames 需要是数组或对象。")
    if not 1 <= len(entries) <= MAX_FRAMES:
        raise ValueError("图集需要包含 1–256 帧。")
    frames, notices = [], []
    try:
        with Image.open(io.BytesIO(sheet_bytes), formats=["PNG"]) as source:
            _integer(source.width, 1, MAX_EDGE, "图集宽度")
            _integer(source.height, 1, MAX_EDGE, "图集高度")
            if source.width * source.height > MAX_PIXELS or getattr(source, "n_frames", 1) != 1 or source.get_format_mimetype() == "image/apng":
                raise ValueError("图集需要是不超过 16M 像素的单张静态 PNG。")
            if "size" in meta:
                declared = _object(meta["size"], "meta.size")
                if tuple(_integer(declared.get(key), 1, MAX_EDGE, f"meta.size.{key}") for key in ("w", "h")) != source.size:
                    raise ValueError("图集 meta.size 与实际图片尺寸不一致。")
            if str(meta.get("scale", "1")) not in ("1", "1.0"):
                raise ValueError("暂不支持缩放导出的图集，请用 scale=1 导出。")
            sheet = source.convert("RGBA")
            try:
                for filename, entry in entries:
                    _label(filename)
                    entry = _object(entry, "frame")
                    x, y, width, height = _rect(entry.get("frame"), "frame")
                    if x + width > sheet.width or y + height > sheet.height:
                        raise ValueError("图集裁切区域超出图片边界。")
                    rotated, trimmed = entry.get("rotated", False), entry.get("trimmed", False)
                    if type(rotated) is not bool or type(trimmed) is not bool:
                        raise ValueError("rotated/trimmed 需要是布尔值。")
                    unrotated = (height, width) if rotated else (width, height)
                    size = entry.get("sourceSize", {"w": unrotated[0], "h": unrotated[1]})
                    size = _object(size, "sourceSize")
                    canvas_size = (_integer(size.get("w"), 1, MAX_EDGE, "sourceSize.w"), _integer(size.get("h"), 1, MAX_EDGE, "sourceSize.h"))
                    placement = entry.get("spriteSourceSize", {"x": 0, "y": 0, "w": unrotated[0], "h": unrotated[1]})
                    px, py, pw, ph = _rect(placement, "spriteSourceSize", allow_zero=True)
                    empty = (pw, ph) == (0, 0)
                    if (empty and (not trimmed or unrotated != (1, 1))
                            or not empty and (pw, ph) != unrotated
                            or px + pw > canvas_size[0] or py + ph > canvas_size[1]):
                        raise ValueError("spriteSourceSize 与裁切尺寸或源画布不一致。")
                    if trimmed and ("sourceSize" not in entry or "spriteSourceSize" not in entry):
                        raise ValueError("裁切图集需要 sourceSize 与 spriteSourceSize。")
                    if not trimmed and ((px, py) != (0, 0) or canvas_size != unrotated):
                        raise ValueError("未裁切的帧与 sourceSize 不一致。")
                    budget.reserve(canvas_size)
                    cropped = sheet.crop((x, y, x + width, y + height))
                    try:
                        if rotated:
                            restored = cropped.transpose(Image.Transpose.ROTATE_90)
                            cropped.close()
                            cropped = restored
                        canvas = Image.new("RGBA", canvas_size)
                        try:
                            if empty:
                                if cropped.getchannel("A").getbbox() is not None:
                                    raise ValueError("空帧占位区域包含非透明像素。")
                            else:
                                canvas.paste(cropped, (px, py))
                            frames.append(budget.frame(canvas, f"{_label(filename)}.png", _duration(entry.get("duration"))))
                        finally:
                            canvas.close()
                    finally:
                        cropped.close()
            finally:
                sheet.close()
    except (OSError, SyntaxError, Image.DecompressionBombError) as error:
        raise ValueError(f"无法完整解码图集：{error}") from error
    tags = meta.get("frameTags", [])
    if not isinstance(tags, list) or len(tags) > 64:
        raise ValueError("图集最多支持 64 个动作标签。")
    clips, names = [], set()
    for tag in tags:
        tag = _object(tag, "frameTag")
        name = tag.get("name")
        if not isinstance(name, str) or not name.strip() or len(name) > 80 or name in names:
            raise ValueError("图集动作标签名称为空、重复或过长。")
        names.add(name)
        start = _integer(tag.get("from"), 0, len(frames) - 1, "tag.from")
        end = _integer(tag.get("to"), start, len(frames) - 1, "tag.to")
        indices = list(range(start, end + 1))
        direction = tag.get("direction", "forward")
        if direction in ("reverse", "pingpong_reverse"):
            indices.reverse()
        if direction in ("pingpong", "pingpong_reverse"):
            indices += indices[-2:0:-1]
        elif direction not in ("forward", "reverse"):
            raise ValueError("图集动作标签方向不支持。")
        repeat = tag.get("repeat", 0)
        if isinstance(repeat, str) and re.fullmatch(r"[0-9]{1,5}", repeat):
            repeat = int(repeat)
        repeat = _integer(repeat, 0, 65535, "tag.repeat")
        if repeat > 1:
            notices.append(f"动作“{name}”指定有限重复次数，本次按单个周期导入。")
        clips.append({"name": name, "indices": indices, "loop": repeat == 0})
    if not clips:
        clips = [{"name": _label(atlas_name), "indices": list(range(len(frames))), "loop": True}]
    return {"format": "aseprite", "name": _label(atlas_name), "frames": frames, "clips": clips, "warnings": notices}


def decode_atlas(metadata, sheet_bytes, sheet_name, atlas_name):
    """Restore Aseprite array/hash frames to their untrimmed logical canvases."""
    return _atlas(metadata, sheet_bytes, sheet_name, atlas_name, _Budget())


def _zip_path(name, directory):
    if (not isinstance(name, str) or not name or len(name) > 1024 or "\\" in name or ":" in name
            or name.startswith("/") or any(unicodedata.category(c) == "Cc" for c in name)):
        raise ValueError("ZIP 包含不安全路径。")
    path = name[:-1] if directory and name.endswith("/") else name
    components = path.split("/")
    if len(components) > 32 or any(part in ("", ".", "..") or len(part) > 255 for part in components):
        raise ValueError("ZIP 包含不安全路径。")
    return path


def _natural(path):
    return tuple((1, int(part)) if part.isdigit() else (0, part.casefold()) for part in re.split(r"([0-9]+)", path))


def _os_metadata(path):
    parts = PurePosixPath(path).parts
    return any(part.casefold() == "__macosx" for part in parts) or parts[-1].casefold() == ".ds_store" or parts[-1].startswith("._")


def _zip_directory(data):
    """Bound central-directory records before ZipFile allocates ZipInfo objects."""
    lower = max(0, len(data) - 65557)
    at = data.rfind(b"PK\x05\x06", lower)
    while at >= lower:
        if at + 22 <= len(data) and at + 22 + struct.unpack_from("<H", data, at + 20)[0] == len(data):
            break
        at = data.rfind(b"PK\x05\x06", lower, at)
    if at < lower:
        raise ValueError("ZIP 缺少有效结束目录。")
    disk, directory_disk, disk_count, count, size, offset = struct.unpack_from("<HHHHII", data, at + 4)
    if disk or directory_disk or count != disk_count or not 1 <= count <= 512 or size == 0xffffffff or offset == 0xffffffff:
        raise ValueError("ZIP 不支持分卷、ZIP64 目录或超量条目。")
    if offset + size != at:
        raise ValueError("ZIP 目录边界无效。")
    position, actual = offset, 0
    while position < at:
        actual += 1
        if actual > 512 or position + 46 > at or data[position:position + 4] != b"PK\x01\x02":
            raise ValueError("ZIP 目录记录数量或边界无效。")
        name, extra, comment = struct.unpack_from("<HHH", data, position + 28)
        position += 46 + name + extra + comment
        if position > at:
            raise ValueError("ZIP 目录记录被截断。")
    if position != at or actual != count:
        raise ValueError("ZIP 目录声明数量不一致。")


def _zip(data, name):
    if not 0 < len(data) <= MAX_ZIP_BYTES:
        raise ValueError("ZIP 需要在 16 MiB 以内。")
    budget, files, seen, offsets = _Budget(), [], set(), set()
    total, file_count, ignored = 0, 0, 0
    all_file_paths = set()
    _zip_directory(data)
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            entries = archive.infolist()
            if not 1 <= len(entries) <= 512:
                raise ValueError("ZIP 最多包含 512 个文件和目录条目。")
            for info in entries:
                directory = info.is_dir()
                # ZipInfo truncates at NUL; orig_filename preserves the attack.
                path = _zip_path(info.orig_filename, directory)
                identity = unicodedata.normalize("NFC", path).casefold()
                if identity in seen:
                    raise ValueError("ZIP 包含重复或大小写冲突的路径。")
                seen.add(identity)
                mode = info.external_attr >> 16
                if stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR):
                    raise ValueError("ZIP 不允许符号链接或特殊文件。")
                if directory:
                    if info.file_size:
                        raise ValueError("ZIP 目录包含数据。")
                    ignored += int(_os_metadata(path))
                    continue
                if info.flag_bits & 1 or info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
                    raise ValueError("ZIP 不支持加密或此压缩方式。")
                if info.header_offset in offsets or not 0 <= info.header_offset < archive.start_dir:
                    raise ValueError("ZIP 文件头位置重叠或越界。")
                offsets.add(info.header_offset)
                suffix = PurePosixPath(path).suffix.lower()
                metadata_file = _os_metadata(path)
                if not metadata_file and suffix not in RASTERS | {".json"}:
                    raise ValueError("ZIP 只能包含图片序列，或一份图集 JSON 和对应 PNG。")
                limit = MAX_JSON_BYTES if suffix == ".json" and not metadata_file else MAX_FILE_BYTES
                if not 0 < info.file_size <= limit or info.file_size > max(1, info.compress_size) * 100:
                    raise ValueError("ZIP 单文件大小或解压比例超过上限。")
                total += info.file_size
                file_count += 1
                all_file_paths.add(identity)
                if total > 32 * 1024 * 1024 or file_count > 257:
                    raise ValueError("ZIP 解压总量或文件数量超过上限。")
                if metadata_file:
                    ignored += 1
                    continue
                files.append((path, info))
            if not files:
                raise ValueError("ZIP 中没有图片。")
            if any(str(parent) in all_file_paths for path in seen for parent in PurePosixPath(path).parents if str(parent) != "."):
                raise ValueError("ZIP 文件与目录路径冲突。")
            ignored_warning = [f"已忽略 {ignored} 个 macOS 元数据条目（.DS_Store、__MACOSX 或 ._*）。"] if ignored else []
            metadata = [(path, info) for path, info in files if path.lower().endswith(".json")]
            def read(info):
                with archive.open(info) as stream:
                    result = stream.read(info.file_size + 1)
                    if len(result) != info.file_size:
                        raise ValueError("ZIP 解压大小与声明不一致。")
                    return result
            if metadata:
                if len(metadata) != 1 or len(files) != 2:
                    raise ValueError("图集 ZIP 必须只有一份 JSON 与其对应 PNG。")
                atlas_path, info = metadata[0]
                raw = _json(read(info))
                image_name = _basename(_object(raw.get("meta"), "meta").get("image"))
                expected = str(PurePosixPath(atlas_path).parent / image_name)
                sheet = next(((path, entry) for path, entry in files if path == expected), None)
                if sheet is None:
                    raise ValueError("ZIP 中没有图集 JSON 引用的同目录图片。")
                result = _atlas(raw, read(sheet[1]), image_name, PurePosixPath(atlas_path).name, budget)
                result["warnings"].extend(ignored_warning)
                return result
            frames, clips, notices, directories = [], [], list(ignored_warning), {}
            for path, info in sorted(files, key=lambda item: (_natural(item[0]), item[0])):
                decoded = _raster(read(info), path, budget)
                first = len(frames)
                frames.extend(decoded["frames"])
                notices.extend(decoded["warnings"])
                if decoded["clips"]:
                    for clip in decoded["clips"]:
                        clips.append({**clip, "name": str(PurePosixPath(path).with_suffix("")), "indices": [first + index for index in clip["indices"]]})
                else:
                    directory = str(PurePosixPath(path).parent)
                    directories.setdefault(directory, []).extend(range(first, len(frames)))
            clips = [{"name": _label(name) if directory == "." else directory, "indices": indices, "loop": True}
                     for directory, indices in directories.items()] + clips
            return {"format": "zip", "name": _label(name), "frames": frames, "clips": clips, "warnings": list(dict.fromkeys(notices))}
    except (zipfile.BadZipFile, zipfile.LargeZipFile, RuntimeError, NotImplementedError, EOFError, OSError) as error:
        raise ValueError(f"ZIP 数据无效：{error}") from error


def decode_media(data, name):
    """Decode a bounded raster/animation, sequence ZIP, or atlas ZIP in memory."""
    _label(name)
    if not isinstance(data, bytes):
        raise ValueError("素材数据需要是二进制字节。")
    if PurePosixPath(name).suffix.lower() == ".zip":
        if not data.startswith((b"PK\x03\x04", b"PK\x05\x06")):
            raise ValueError("ZIP 文件签名无效。")
        return _zip(data, name)
    return _raster(data, name, _Budget())
