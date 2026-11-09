import io
import unittest

from PIL import Image

import parts
import server


class StandingDanceTests(unittest.TestCase):
    def test_fixed_root_and_matching_loop_endpoints(self):
        project = server.demo_project()
        action = next(a for a in project["actions"] if a["id"] == "standing-dance")
        server.validate_project(project)
        self.assertEqual(action["durationTicks"], 40)
        root = action["tracks"][0]["keys"]
        self.assertEqual({(k["x"], k["y"]) for k in root}, {(64, 126)})
        self.assertEqual(parts.compose_parts(project, action, 0), parts.compose_parts(project, action, 40))
        self.assertGreater(max(k["rotation"] for k in root), 0)
        self.assertLess(min(k["rotation"] for k in root), 0)

    def test_preset_is_a_complete_playable_frame_action_with_unclipped_art(self):
        preset = server.dance_preset()
        action = preset["action"]
        self.assertEqual(action["kind"], "frames")
        self.assertTrue(action["loop"])
        self.assertEqual(sum(f["ticks"] for f in action["frames"]), 40)
        images = {a["id"]: server.asset_bytes(a) for a in preset["assets"]}
        self.assertGreater(len(images), 16)
        self.assertTrue(all(f["assetId"] in images for f in action["frames"]))
        for data in images.values():
            image = Image.open(io.BytesIO(data)).convert("RGBA")
            self.assertEqual(image.size, (128, 128))
            left, top, right, bottom = image.getbbox()
            self.assertGreater(left, 0)
            self.assertGreater(top, 0)
            self.assertLess(right, 128)
            self.assertLess(bottom, 128)


if __name__ == "__main__":
    unittest.main()
