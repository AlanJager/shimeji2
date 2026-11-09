import base64
import copy
import io
import json
import stat
import struct
import unittest
from unittest.mock import patch
import zipfile
import zlib

from PIL import Image, PngImagePlugin

import media


def encoded(image, kind="PNG", **options):
    output = io.BytesIO()
    image.save(output, kind, **options)
    return output.getvalue()


def frame_image(frame):
    return Image.open(io.BytesIO(base64.b64decode(frame["src"].split(",", 1)[1]))).convert("RGBA")


def animation(kind, disposal=None, loop=0, poster=False):
    frames = []
    for x, color in [(0, (255, 0, 0, 255)), (1, (0, 255, 0, 255)), (2, (0, 0, 255, 255))]:
        image = Image.new("RGBA", (4, 2))
        image.putpixel((x, 0), color)
        frames.append(image)
    options = {"save_all": True, "append_images": frames[1:], "duration": [40, 80, 120]}
    if loop is not None:
        options["loop"] = loop
    if kind == "GIF":
        options.update(disposal=disposal or [1, 2, 1], optimize=False)
    if kind == "PNG":
        options.update(disposal=disposal or [0, 1, 0], blend=[0, 1, 1])
        if poster:
            options.update(default_image=True, append_images=frames)
            return encoded(Image.new("RGBA", (4, 2), "yellow"), kind, **options)
    if kind == "WEBP":
        options["lossless"] = True
    return encoded(frames[0], kind, **options)


def archive(entries, compression=zipfile.ZIP_STORED):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=compression) as target:
        for name, data in entries:
            target.writestr(name, data)
    return output.getvalue()


def atlas():
    sheet = Image.new("RGBA", (6, 2))
    for x, color in [(0, "red"), (2, "green"), (4, "blue")]:
        sheet.paste(color, (x, 0, x + 2, 2))
    frames = [{"filename": f"pose-{i}.png", "frame": {"x": i * 2, "y": 0, "w": 2, "h": 2},
               "rotated": False, "trimmed": False, "spriteSourceSize": {"x": 0, "y": 0, "w": 2, "h": 2},
               "sourceSize": {"w": 2, "h": 2}, "duration": [40, 120, 80][i]} for i in range(3)]
    return {"frames": frames, "meta": {"image": "sheet.png", "size": {"w": 6, "h": 2}, "scale": "1"}}, encoded(sheet)


class RasterTests(unittest.TestCase):
    def test_png_static_preserves_dimensions_pixels_and_legacy_duration_without_metadata(self):
        image = Image.new("RGBA", (13, 7), (12, 34, 56, 91))
        result = media.decode_media(encoded(image), "still.png")
        self.assertEqual((result["format"], result["clips"]), ("png", []))
        frame = result["frames"][0]
        self.assertEqual((frame["width"], frame["height"], frame["durationMs"]), (13, 7, 240))
        self.assertEqual(frame_image(frame).tobytes(), image.tobytes())
        text = PngImagePlugin.PngInfo()
        text.add_text("loop", "ordinary annotation")
        self.assertEqual(media.decode_media(encoded(image, pnginfo=text), "text.png")["format"], "png")

    def test_jpeg_exif_orientation_is_applied_before_returning_source_frame(self):
        image = Image.new("RGB", (6, 2), "red")
        image.paste("blue", (3, 0, 6, 2))
        exif = Image.Exif()
        exif[274] = 6  # Rotate 90 degrees clockwise.
        result = media.decode_media(encoded(image, "JPEG", quality=100, subsampling=0, exif=exif), "photo.jpg")
        frame = result["frames"][0]
        self.assertEqual((frame["width"], frame["height"]), (2, 6))
        restored = frame_image(frame)
        self.assertGreater(restored.getpixel((0, 0))[0], 240)
        self.assertGreater(restored.getpixel((0, 5))[2], 240)
        self.assertFalse(restored.getexif())
        self.assertTrue(any("不会自动抠图" in warning for warning in result["warnings"]))

    def test_gif_background_disposal_composes_the_logical_canvas_and_preserves_timing(self):
        result = media.decode_media(animation("GIF"), "delta.gif")
        self.assertEqual([frame["durationMs"] for frame in result["frames"]], [40, 80, 120])
        self.assertTrue(result["clips"][0]["loop"])
        middle, final = [frame_image(result["frames"][i]) for i in (1, 2)]
        self.assertEqual(middle.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(middle.getpixel((1, 0)), (0, 255, 0, 255))
        self.assertEqual(final.getpixel((0, 0))[3], 0)
        self.assertEqual(final.getpixel((1, 0))[3], 0)
        self.assertEqual(final.getpixel((2, 0)), (0, 0, 255, 255))

    def test_gif_restore_previous_recovers_the_canvas_before_the_disposed_frame(self):
        result = media.decode_media(animation("GIF", disposal=[1, 3, 1]), "restore.gif")
        final = frame_image(result["frames"][2])
        self.assertEqual(final.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(final.getpixel((1, 0))[3], 0)
        self.assertEqual(final.getpixel((2, 0)), (0, 0, 255, 255))

    def test_apng_with_png_extension_keeps_animation_blend_and_disposal(self):
        result = media.decode_media(animation("PNG"), "delta.png")
        self.assertEqual(result["format"], "apng")
        self.assertEqual([f["durationMs"] for f in result["frames"]], [40, 80, 120])
        middle, final = [frame_image(result["frames"][i]) for i in (1, 2)]
        self.assertEqual(middle.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(middle.getpixel((1, 0)), (0, 255, 0, 255))
        self.assertEqual(final.getpixel((0, 0))[3], 0)
        self.assertEqual(final.getpixel((2, 0)), (0, 0, 255, 255))

    def test_apng_default_poster_is_excluded_from_animation_frames(self):
        result = media.decode_media(animation("PNG", poster=True), "poster.apng")
        self.assertEqual(len(result["frames"]), 3)
        self.assertEqual(frame_image(result["frames"][0]).getpixel((0, 0)), (255, 0, 0, 255))
        self.assertTrue(any("封面" in notice for notice in result["warnings"]))

    def test_apng_missing_last_frame_data_never_succeeds_with_a_shorter_animation(self):
        source = animation("PNG")
        at, chunks = 8, []
        while at < len(source):
            end = at + 12 + struct.unpack_from(">I", source, at)[0]
            chunks.append((source[at + 4:at + 8], source[at:end]))
            at = end
        last = max(index for index, (kind, _) in enumerate(chunks) if kind == b"fdAT")
        malformed = source[:8] + b"".join(chunk for index, (_, chunk) in enumerate(chunks) if index != last)
        with patch.object(media.Image, "open", side_effect=AssertionError("decoder reached")):
            with self.assertRaisesRegex(ValueError, "位图数据"):
                media.decode_media(malformed, "missing-frame.apng")
        # The decoder also checks its final count rather than treating premature
        # EOF as a successful shorter animation, independently of preflight.
        with patch.object(media, "_png_chunks", lambda _: None):
            with self.assertRaisesRegex(ValueError, "完整解码"):
                media.decode_media(malformed, "missing-frame.apng")

    def test_animated_webp_preserves_composed_pixels_alpha_and_loaded_frame_duration(self):
        result = media.decode_media(animation("WEBP"), "poses.webp")
        self.assertEqual([f["durationMs"] for f in result["frames"]], [40, 80, 120])
        for index, color in enumerate([(255, 0, 0, 255), (0, 255, 0, 255), (0, 0, 255, 255)]):
            image = frame_image(result["frames"][index])
            self.assertEqual(image.getpixel((index, 0)), color)
            self.assertEqual(image.getpixel(((index + 1) % 3, 0))[3], 0)

    def test_source_loop_semantics_warn_only_unrepresentable_finite_repeats(self):
        for kind, name in [("GIF", "x.gif"), ("PNG", "x.apng"), ("WEBP", "x.webp")]:
            finite = media.decode_media(animation(kind, loop=2), name)
            self.assertFalse(finite["clips"][0]["loop"])
            self.assertTrue(finite["warnings"])
            once = media.decode_media(animation(kind, loop=None if kind == "GIF" else 1), name)
            self.assertFalse(once["clips"][0]["loop"])
            self.assertFalse(once["warnings"])

    def test_signature_truncation_crc_and_unsupported_format_are_rejected(self):
        png = encoded(Image.new("RGBA", (2, 2), "red"))
        for data, name in [(png, "x.jpg"), (png[:-12], "x.png"), (animation("GIF")[:-1], "x.gif"),
                           (encoded(Image.new("RGB", (2, 2)), "BMP"), "x.bmp"), (b"not an image", "x.png")]:
            with self.assertRaises(ValueError):
                media.decode_media(data, name)
        damaged = bytearray(png)
        damaged[29] ^= 1
        with self.assertRaisesRegex(ValueError, "校验"):
            media.decode_media(bytes(damaged), "x.png")
        incomplete = bytearray(animation("PNG"))
        control = incomplete.index(b"acTL")
        struct.pack_into(">I", incomplete, control + 4, 4)
        struct.pack_into(">I", incomplete, control + 12, zlib.crc32(incomplete[control:control + 12]))
        with patch.object(media.Image, "open", side_effect=AssertionError("decoder reached")):
            with self.assertRaisesRegex(ValueError, "声明帧数"):
                media.decode_media(bytes(incomplete), "incomplete.apng")

    def test_preflight_refuses_large_or_expanding_canvas_before_pillow_open(self):
        oversized = encoded(Image.new("RGBA", (4097, 1)))
        bad_gif = bytearray(animation("GIF"))
        at = 13 + 3 * 2 ** ((bad_gif[10] & 7) + 1)
        while bad_gif[at] != 0x2c:
            at += 2
            while bad_gif[at]:
                at += 1 + bad_gif[at]
            at += 1
        struct.pack_into("<H", bad_gif, at + 5, 5000)
        bad_webp = bytearray(animation("WEBP"))
        self.assertEqual(bad_webp[12:16], b"VP8X")
        bad_webp[24:27] = (5000 - 1).to_bytes(3, "little")
        hidden_webp = bytearray(animation("WEBP"))
        header = hidden_webp.index(b"VP8L") + 9
        bits = struct.unpack_from("<I", hidden_webp, header)[0]
        struct.pack_into("<I", hidden_webp, header, (bits & ~0x3fff) | (5000 - 1))
        with patch.object(media.Image, "open", side_effect=AssertionError("decoder reached")):
            for data, name in [(oversized, "big.png"), (bytes(bad_gif), "bad.gif"), (bytes(bad_webp), "big.webp"), (bytes(hidden_webp), "hidden.webp")]:
                with self.assertRaises(ValueError):
                    media.decode_media(data, name)

    def test_frame_pixel_and_encoded_output_budgets_are_cumulative(self):
        source = animation("GIF")
        with patch.object(media, "MAX_FRAMES", 2):
            with self.assertRaises(ValueError):
                media.decode_media(source, "three.gif")
        with patch.object(media, "MAX_PIXELS", 16):
            with self.assertRaisesRegex(ValueError, "像素"):
                media.decode_media(source, "three.gif")
        with patch.object(media, "MAX_PNG_BYTES", 100):
            with self.assertRaisesRegex(ValueError, "PNG"):
                media.decode_media(source, "three.gif")
        with self.assertRaises(ValueError):
            media.decode_media(b"x" * (media.MAX_FILE_BYTES + 1), "big.png")
        with self.assertRaisesRegex(ValueError, "60000"):
            media._duration(60001)


class AtlasTests(unittest.TestCase):
    def test_array_and_hash_order_durations_and_all_tag_directions(self):
        metadata, sheet = atlas()
        for direction, expected in [("forward", [0, 1, 2]), ("reverse", [2, 1, 0]),
                                    ("pingpong", [0, 1, 2, 1]), ("pingpong_reverse", [2, 1, 0, 1])]:
            metadata["meta"]["frameTags"] = [{"name": "walk", "from": 0, "to": 2, "direction": direction}]
            result = media.decode_atlas(metadata, sheet, "sheet.png", "poses.json")
            self.assertEqual(result["clips"][0]["indices"], expected)
            self.assertEqual([f["durationMs"] for f in result["frames"]], [40, 120, 80])
            self.assertEqual(frame_image(result["frames"][2]).getpixel((0, 0)), (0, 0, 255, 255))
        metadata["frames"] = {frame["filename"]: frame for frame in reversed(metadata["frames"])}
        result = media.decode_atlas(json.dumps(metadata), sheet, "sheet.png", "poses.json")
        self.assertEqual([f["durationMs"] for f in result["frames"]], [80, 120, 40])

    def test_rotated_trimmed_region_restores_exact_pixels_and_original_transparent_canvas(self):
        original = Image.new("RGBA", (2, 3))
        for x, y, color in [(0, 0, "red"), (1, 0, "green"), (0, 2, "blue"), (1, 2, "yellow")]:
            original.putpixel((x, y), Image.new("RGBA", (1, 1), color).getpixel((0, 0)))
        sheet = Image.new("RGBA", (6, 4))
        sheet.paste(original.transpose(Image.Transpose.ROTATE_270), (2, 1))
        metadata = {"frames": [{"filename": "trim.png", "frame": {"x": 2, "y": 1, "w": 3, "h": 2},
                                "rotated": True, "trimmed": True, "spriteSourceSize": {"x": 1, "y": 2, "w": 2, "h": 3},
                                "sourceSize": {"w": 5, "h": 6}, "duration": 70}], "meta": {"image": "sheet.png"}}
        result = media.decode_atlas(metadata, encoded(sheet), "sheet.png", "trim.json")
        expected = Image.new("RGBA", (5, 6))
        expected.paste(original, (1, 2))
        self.assertEqual(frame_image(result["frames"][0]).tobytes(), expected.tobytes())

    def test_atlas_rejects_external_sheet_ambiguous_geometry_tags_and_duplicate_json_fields(self):
        changes = [lambda j: j["meta"].update(image="../sheet.png"), lambda j: j["meta"].update(image="https://x/sheet.png"),
                   lambda j: j["meta"].update(image="other.png"), lambda j: j["meta"].update(scale="2"),
                   lambda j: j["frames"][0]["frame"].update(x=5), lambda j: j["frames"][0].update(trimmed="yes"),
                   lambda j: j["frames"][0]["sourceSize"].update(w=5), lambda j: j["frames"][0].update(duration=-1),
                   lambda j: j["meta"].update(frameTags=[{"name": "bad", "from": 0, "to": 4}]),
                   lambda j: j["meta"].update(frameTags=[{"name": "bad", "from": 0, "to": 2, "direction": "random"}])]
        metadata, sheet = atlas()
        for change in changes:
            changed = copy.deepcopy(metadata)
            change(changed)
            before = copy.deepcopy(changed)
            with self.assertRaises(ValueError):
                media.decode_atlas(changed, sheet, "sheet.png", "bad.json")
            self.assertEqual(changed, before)
        with self.assertRaises(ValueError):
            media.decode_atlas('{"frames":[],"frames":{},"meta":{}}', sheet, "sheet.png", "bad.json")
        with self.assertRaises(ValueError):
            media.decode_atlas(metadata, animation("PNG"), "sheet.png", "bad.json")

    def test_tag_endpoints_single_frames_and_finite_repeat_are_explicit(self):
        metadata, sheet = atlas()
        metadata["meta"]["frameTags"] = [{"name": "one", "from": 1, "to": 1, "direction": "pingpong_reverse", "repeat": "2"}]
        result = media.decode_atlas(metadata, sheet, "sheet.png", "tags.json")
        self.assertEqual(result["clips"], [{"name": "one", "indices": [1], "loop": False}])
        self.assertTrue(result["warnings"])

    def test_transparent_empty_trimmed_frame_keeps_canvas_and_duration_without_dropping_the_frame(self):
        metadata = {"frames": [{"filename": "empty", "frame": {"x": 0, "y": 0, "w": 1, "h": 1}, "trimmed": True,
                                "spriteSourceSize": {"x": 0, "y": 0, "w": 0, "h": 0}, "sourceSize": {"w": 12, "h": 10}, "duration": 120}],
                    "meta": {"image": "sheet.png"}}
        result = media.decode_atlas(metadata, encoded(Image.new("RGBA", (1, 1))), "sheet.png", "empty.json")
        self.assertEqual(len(result["frames"]), 1)
        self.assertEqual(result["frames"][0]["durationMs"], 120)
        self.assertEqual(frame_image(result["frames"][0]).size, (12, 10))
        self.assertIsNone(frame_image(result["frames"][0]).getbbox())
        with self.assertRaisesRegex(ValueError, "非透明"):
            media.decode_atlas(metadata, encoded(Image.new("RGBA", (1, 1), "red")), "sheet.png", "empty.json")


class ZipTests(unittest.TestCase):
    def setUp(self):
        self.png = encoded(Image.new("RGBA", (2, 2), "red"))

    def test_zip_natural_order_and_directory_clips_cover_every_source_once(self):
        result = media.decode_media(archive([("walk/10.png", self.png), ("walk/2.png", self.png), ("idle/1.png", self.png), ("walk/1.png", self.png), ("empty/", b"")]), "poses.zip")
        self.assertEqual([f["name"] for f in result["frames"]], ["1-001.png", "1-001.png", "2-001.png", "10-001.png"])
        self.assertEqual(result["clips"], [{"name": "idle", "indices": [0], "loop": True}, {"name": "walk", "indices": [1, 2, 3], "loop": True}])

    def test_mac_metadata_is_ignored_with_warning_after_every_archive_admission_check(self):
        entries = [("poses/1.png", self.png), ("poses/.DS_Store", b"finder metadata"),
                   ("__MACOSX/", b""), ("__MACOSX/poses/._1.png", b"resource fork"), ("poses/._1.png", b"appledouble")]
        result = media.decode_media(archive(entries), "finder.zip")
        self.assertEqual(len(result["frames"]), 1)
        self.assertTrue(any("macOS" in notice for notice in result["warnings"]))
        for bad in ["__MACOSX/../evil.png", "__MACOSX/../../evil.png"]:
            with self.assertRaises(ValueError):
                media.decode_media(archive(entries + [(bad, b"bad")]), "bad.zip")
        link = zipfile.ZipInfo("__MACOSX/._link")
        link.create_system = 3
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        with self.assertRaises(ValueError):
            media.decode_media(archive(entries + [(link, b"target")]), "link.zip")
        with self.assertRaisesRegex(ValueError, "比例"):
            media.decode_media(archive([("1.png", self.png), (".DS_Store", b"x" * 100000)], zipfile.ZIP_DEFLATED), "bomb.zip")
        with self.assertRaisesRegex(ValueError, "文件数量"):
            media.decode_media(archive([("1.png", self.png)] + [(f"__MACOSX/._{i}", b"x") for i in range(257)]), "count.zip")
        metadata, sheet = atlas()
        result = media.decode_media(archive([("poses.json", json.dumps(metadata)), ("sheet.png", sheet), ("._sheet.png", b"fork")]), "atlas.zip")
        self.assertEqual(result["format"], "aseprite")
        self.assertTrue(any("macOS" in notice for notice in result["warnings"]))

    def test_zip_animated_source_keeps_own_timing_and_loop(self):
        result = media.decode_media(archive([("still.png", self.png), ("walk.gif", animation("GIF", loop=2))]), "mixed.zip")
        self.assertEqual(len(result["frames"]), 4)
        self.assertEqual(result["clips"][1], {"name": "walk", "indices": [1, 2, 3], "loop": False})
        self.assertTrue(result["warnings"])

    def test_atlas_zip_requires_exact_same_directory_pair_and_matches_direct_decode(self):
        metadata, sheet = atlas()
        zipped = archive([("folder/poses.json", json.dumps(metadata)), ("folder/sheet.png", sheet)])
        self.assertEqual(media.decode_media(zipped, "atlas.zip"), media.decode_atlas(metadata, sheet, "sheet.png", "poses.json"))
        for entries in [[("poses.json", json.dumps(metadata)), ("other/sheet.png", sheet)],
                        [("poses.json", json.dumps(metadata)), ("sheet.png", sheet), ("extra.png", self.png)]]:
            with self.assertRaises(ValueError):
                media.decode_media(archive(entries), "bad.zip")

    def test_zip_traversal_symlink_duplicates_controls_and_unsupported_entries_fail(self):
        for name in ["../a.png", "/a.png", "a/../b.png", "C:/a.png", "a\\b.png", "a//b.png", "a/./b.png", "a\x00.png", "a\x01.png", "readme.txt", "inner.zip"]:
            with self.assertRaises(ValueError, msg=repr(name)):
                media.decode_media(archive([(name, self.png)]), "bad.zip")
        for entries in [[("a.png", self.png), ("A.png", self.png)], [("é.png", self.png), ("e\u0301.png", self.png)]]:
            with self.assertRaises(ValueError):
                media.decode_media(archive(entries), "duplicate.zip")
        with self.assertRaises(ValueError):
            media.decode_media(archive([("A.png", self.png), ("a.PNG/child.png", self.png)]), "prefix.zip")
        link = zipfile.ZipInfo("link.png")
        link.create_system = 3
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        with self.assertRaisesRegex(ValueError, "链接"):
            media.decode_media(archive([(link, b"target.png")]), "link.zip")

    def test_directory_count_size_ratio_and_corrupt_headers_are_bounded_before_zipfile_allocation(self):
        excessive = archive([(f"dir{i}/", b"") for i in range(513)] + [("a.png", self.png)])
        forged = bytearray(excessive)
        end = forged.rfind(b"PK\x05\x06")
        struct.pack_into("<HH", forged, end + 8, 1, 1)
        with patch.object(media.zipfile, "ZipFile", side_effect=AssertionError("ZipInfo allocation reached")):
            for data in [excessive, bytes(forged)]:
                with self.assertRaises(ValueError):
                    media.decode_media(data, "count.zip")
        for data in [archive([("bomb.png", b"x" * 100000)], zipfile.ZIP_DEFLATED),
                     archive([("big.png", b"x" * (media.MAX_FILE_BYTES + 1))]), archive([("a.png", self.png)])[:-10]]:
            with self.assertRaises(ValueError):
                media.decode_media(data, "bad.zip")
        corrupt = bytearray(archive([("a.png", self.png)]))
        corrupt[35] ^= 1
        with self.assertRaises(ValueError):
            media.decode_media(bytes(corrupt), "crc.zip")

    def test_zip_pixel_budget_applies_across_files_with_real_16m_boundary(self):
        image = encoded(Image.new("RGBA", (1024, 1024), "red"))
        with self.assertRaisesRegex(ValueError, "像素"):
            media.decode_media(archive([(f"{i}.png", image) for i in range(17)]), "pixels.zip")


if __name__ == "__main__":
    unittest.main()
