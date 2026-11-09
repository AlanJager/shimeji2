"""Validate and bake editor part rigs into fixed-canvas classic sprites.

Coordinate and interpolation contract: docs/handoff/editor-v2-contract.md.
No source image or project data is changed while rendering.
"""

import hashlib
import io
import math


FIELDS = ("x", "y", "rotation", "scaleX", "scaleY")
IDENTITY = (1, 0, 0, 1, 0, 0)


def number(value, low, high):
    if type(value) not in (int, float) or not math.isfinite(value) or not low <= value <= high:
        raise ValueError(f"部件数值需要在 {low}–{high} 之间")


def validate_pose(pose, asset_ids):
    for field in ("x", "y"):
        number(pose[field], -256, 256)
    number(pose["rotation"], -3600, 3600)
    for field in ("scaleX", "scaleY"):
        number(pose[field], 0.05, 4)
    if type(pose["visible"]) is not bool or pose["assetId"] not in asset_ids:
        raise ValueError("部件图片或可见性无效")
    if "opacity" in pose:
        number(pose["opacity"], 0, 1)
    if "zOrder" in pose and (type(pose["zOrder"]) is not int or not -32 <= pose["zOrder"] <= 32):
        raise ValueError("部件遮挡顺序需要是 -32 到 32 的整数")


def validate_parts(project, asset_ids):
    parts = project.get("parts", [])
    if not isinstance(parts, list) or len(parts) > 32:
        raise ValueError("一个工程最多包含 32 个部件")
    by_id = {}
    for part in parts:
        ident = part["id"]
        if not isinstance(ident, str) or not 1 <= len(ident) <= 64 or not all(c.isascii() and (c.isalnum() or c in "_-") for c in ident) or ident in by_id:
            raise ValueError("部件 ID 无效或重复")
        if not isinstance(part["name"], str) or not 1 <= len(part["name"]) <= 80:
            raise ValueError("部件名称需要包含 1–80 个字符")
        for field in ("pivotX", "pivotY"):
            number(part[field], -256, 256)
        validate_pose(part, asset_ids)
        by_id[ident] = part
    for part in parts:
        seen = {part["id"]}
        parent = part["parentId"]
        while parent is not None:
            if parent not in by_id or parent in seen:
                raise ValueError("部件层级包含循环或不存在的父部件")
            seen.add(parent)
            parent = by_id[parent]["parentId"]
    baked_ticks = 0
    for action in project["actions"]:
        kind = action.get("kind", "frames")
        if kind not in ("frames", "parts"):
            raise ValueError("不支持的动作类型")
        if kind != "parts":
            continue
        duration = action["durationTicks"]
        if type(duration) is not int or not 1 <= duration <= 1000 or not parts:
            raise ValueError("部件动作需要至少一个部件，时长为 1–1000 tick")
        baked_ticks += duration
        tracks = action.get("tracks", [])
        if not isinstance(tracks, list) or len(tracks) > len(parts):
            raise ValueError("部件动画轨道数量无效")
        seen_parts = set()
        for track in tracks:
            if track["partId"] not in by_id or track["partId"] in seen_parts:
                raise ValueError("动画轨道引用无效或重复")
            seen_parts.add(track["partId"])
            keys = track["keys"]
            if not isinstance(keys, list) or not 1 <= len(keys) <= 256:
                raise ValueError("每条轨道需要包含 1–256 个关键帧")
            previous = -1
            for key in keys:
                tick = key["tick"]
                if type(tick) is not int or not previous < tick <= duration:
                    raise ValueError("关键帧时间需要升序、唯一且在动作时长内")
                previous = tick
                validate_pose(key, asset_ids)
    if baked_ticks > 2000:
        raise ValueError("部件动画总时长不能超过 2000 tick（80 秒），请拆分为多个工程")


def multiply(a, b):
    aa, ab, ac, ad, ae, af = a
    ba, bb, bc, bd, be, bf = b
    return (aa * ba + ac * bb, ab * ba + ad * bb,
            aa * bc + ac * bd, ab * bc + ad * bd,
            aa * be + ac * bf + ae, ab * be + ad * bf + af)


def pose_at(part, action, tick):
    track = next((track for track in action.get("tracks", []) if track["partId"] == part["id"]), None)
    if track is None or not track["keys"]:
        return part
    keys = track["keys"]
    if tick <= keys[0]["tick"]:
        return keys[0]
    for left, right in zip(keys, keys[1:]):
        if tick < right["tick"]:
            mix = (tick - left["tick"]) / (right["tick"] - left["tick"])
            pose = {**left, **{field: left[field] + (right[field] - left[field]) * mix for field in FIELDS}}
            pose["opacity"] = left.get("opacity", 1) + (right.get("opacity", 1) - left.get("opacity", 1)) * mix
            pose["zOrder"] = left.get("zOrder", 0)
            return pose
    return keys[-1]


def compose_parts(project, action, tick):
    parts = {part["id"]: part for part in project.get("parts", [])}
    resolved = {}

    def resolve(part):
        if part["id"] in resolved:
            return resolved[part["id"]]
        pose = pose_at(part, action, tick)
        radians = math.radians(pose["rotation"])
        cos, sin = math.cos(radians), math.sin(radians)
        matrix = (cos * pose["scaleX"], sin * pose["scaleX"],
                  -sin * pose["scaleY"], cos * pose["scaleY"], pose["x"], pose["y"])
        visible = pose["visible"]
        if part["parentId"] is not None:
            parent = resolve(parts[part["parentId"]])
            matrix = multiply(parent["nodeMatrix"], matrix)
            visible = visible and parent["visible"]
        image_matrix = multiply(matrix, (1, 0, 0, 1, -part["pivotX"], -part["pivotY"]))
        resolved[part["id"]] = {"id": part["id"], "assetId": pose["assetId"],
                                 "nodeMatrix": matrix, "matrix": image_matrix, "visible": visible,
                                 "opacity": pose.get("opacity", 1), "zOrder": pose.get("zOrder", 0)}
        return resolved[part["id"]]

    return [resolve(part) for part in project.get("parts", [])]


def bake_action(project, action, source_bytes):
    try:
        from PIL import Image
    except ImportError as error:
        raise ValueError("部件导出需要 Pillow，请运行 python3 -m pip install Pillow 后重启编辑器") from error
    assets = {}
    frames, images = [], {}
    for tick in range(action["durationTicks"]):
        canvas = Image.new("RGBA", (128, 128))
        composed = sorted(enumerate(compose_parts(project, action, tick)), key=lambda item: (item[1]["zOrder"], item[0]))
        for _order, part in composed:
            if not part["visible"] or part["opacity"] <= 0:
                continue
            if part["assetId"] not in assets:
                image = Image.open(io.BytesIO(source_bytes[part["assetId"]]))
                if image.size != (128, 128):
                    raise ValueError("部件 PNG 需要归一化为 128 × 128")
                assets[part["assetId"]] = image.convert("RGBA")
            a, b, c, d, e, f = part["matrix"]
            determinant = a * d - b * c
            inverse = (d / determinant, -c / determinant, (c * f - d * e) / determinant,
                       -b / determinant, a / determinant, (b * e - a * f) / determinant)
            warped = assets[part["assetId"]].transform((128, 128), Image.Transform.AFFINE, inverse,
                                                     resample=Image.Resampling.NEAREST)
            if part["opacity"] < 1:
                red, green, blue, alpha = warped.split()
                warped.putalpha(alpha.point(lambda value, scale=part["opacity"]: int(value * scale + 0.5)))
            canvas = Image.alpha_composite(canvas, warped)
        ident = "baked-" + hashlib.sha256(canvas.tobytes()).hexdigest()[:24]
        if ident not in images:
            encoded = io.BytesIO()
            canvas.save(encoded, format="PNG")
            images[ident] = encoded.getvalue()
        if frames and frames[-1]["assetId"] == ident:
            frames[-1]["ticks"] += 1
        else:
            frames.append({"id": f"bake-{tick}", "assetId": ident, "ticks": 1, "x": 0, "y": 0})
    return frames, images
