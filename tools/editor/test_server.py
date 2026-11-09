import copy
import io
import json
import os
import unittest
import xml.etree.ElementTree as ET
import zipfile
from types import SimpleNamespace

import server
import parts


class EditorExportTests(unittest.TestCase):
    def test_hostile_media_request_returns_error_and_releases_decode_slot(self):
        for body, route in [(b'{"metadata":' + b'['*2000 + b'0' + b']'*2000 + b'}', '/api/media/atlas'),
                            (b'bad', '/api/media/decode')]:
            responses = []
            fake = SimpleNamespace(headers={'Content-Length':str(len(body))},rfile=io.BytesIO(body),
                                   connection=SimpleNamespace(settimeout=lambda timeout:None),path=route,
                                   respond=lambda status,body,mime:responses.append((status,json.loads(body))))
            server.Handler.media_request(fake,route)
            self.assertEqual(responses[0][0],400)
            self.assertIn('error',responses[0][1])
            self.assertTrue(server.MEDIA_SLOTS.acquire(blocking=False))
            self.assertTrue(server.MEDIA_SLOTS.acquire(blocking=False))
            server.MEDIA_SLOTS.release();server.MEDIA_SLOTS.release()

    def test_group_metadata_roundtrips_without_changing_runtime_frames(self):
        project = server.demo_project()
        project['assetGroups'] = [{'id':'group-a','name':'Imported animation',
                                  'frames':[{'assetId':'shime1','ticks':1},{'assetId':'shime2','ticks':3},{'assetId':'shime1','ticks':1}],
                                  'source':{'format':'apng','fileNames':['source.png']}}]
        with zipfile.ZipFile(io.BytesIO(server.export_pack(project))) as archive:
            self.assertEqual(json.loads(archive.read('editor-project.json'))['assetGroups'], project['assetGroups'])
            runtime = json.loads(archive.read('editor-runtime.json'))
            self.assertNotIn('assetGroups', runtime)
            self.assertEqual(runtime['rules'], project['rules'])
            idle = ET.fromstring(archive.read('conf/actions.xml')).find("./ActionList/Action[@Name='Editor_idle']")
            self.assertEqual(idle.attrib['Duration'],'250')

    def test_invalid_groups_rejected_before_export(self):
        original = {'id':'g','name':'Source','frames':[{'assetId':'shime1','ticks':1}],
                    'source':{'format':'gif','fileNames':['a.gif']}}
        for change in [lambda g:g.update(name=' '),lambda g:g['frames'][0].update(assetId='missing'),
                       lambda g:g['frames'][0].update(ticks=True),lambda g:g.update(frames=[False]),
                       lambda g:g['source'].update(fileNames=['x'*257])]:
            project = server.demo_project();group=copy.deepcopy(original);change(group);project['assetGroups']=[group]
            with self.assertRaises(ValueError): server.export_pack(project)

    def test_vrm_baked_frames_keep_recipe_and_export_as_ordinary_png_action(self):
        project = server.demo_project()
        action = project['actions'][0]
        action['source3d'] = {'format': 'vrm-bake', 'version': 1, 'modelName': 'local.vrm',
                              'modelHash': 'fixture', 'recipe': {'durationTicks': 1}}
        action['frames'] = [dict(action['frames'][0], ticks=1)]
        with zipfile.ZipFile(io.BytesIO(server.export_pack(project))) as archive:
            source = json.loads(archive.read('editor-project.json'))
            self.assertEqual(source['actions'][0]['source3d'], action['source3d'])
            self.assertFalse(any(name.endswith(('.vrm', '.vrma')) for name in archive.namelist()))
            xml = ET.fromstring(archive.read('conf/actions.xml'))
            baked = xml.find("./ActionList/Action[@Name='Editor_idle']")
            self.assertEqual(baked.attrib['Duration'], '1')
            self.assertEqual(baked.find('./Animation/Pose').attrib['Duration'], '1')

    def test_demo_uses_upstream_frames_anchors_and_tick_durations(self):
        project = server.demo_project()
        server.validate_project(project)
        walk = next(a for a in project["actions"] if a["id"] == "walk")
        self.assertEqual([f["ticks"] for f in walk["frames"]], [6, 6, 6, 6])
        self.assertEqual([f["assetId"] for f in walk["frames"]], ["shime1", "shime2", "shime1", "shime3"])
        sit = next(a for a in project["actions"] if a["id"] == "sit")
        self.assertEqual(sit["frames"][0]["y"], 16)

    def test_export_preserves_bytes_and_contains_editable_project_and_required_behaviors(self):
        project = server.demo_project()
        with zipfile.ZipFile(io.BytesIO(server.export_pack(project))) as archive:
            self.assertEqual(archive.read("img/shime1.png"), (server.SHIME / "img/shime1.png").read_bytes())
            self.assertEqual(json.loads(archive.read("editor-project.json")), project)
            behaviors = ET.fromstring(archive.read("conf/behaviors.xml"))
            names = {b.attrib["Name"] for b in behaviors.iter("Behavior")}
            self.assertTrue({"ChaseMouse", "Fall", "Dragged", "Thrown", "Editor_idle"} <= names)
            runtime = json.loads(archive.read("editor-runtime.json"))
            self.assertEqual(runtime["rules"], project["rules"])
            self.assertEqual(runtime["regions"], project["regions"])
            self.assertIn("editor-runtime.json", archive.read("EDITOR-README.txt").decode())
            self.assertIn("bundled-shime-LICENSE.txt", archive.namelist())

    def test_export_writes_changed_order_duration_anchor_and_default(self):
        project = server.demo_project()
        project["defaultActionId"] = "walk"
        walk = next(a for a in project["actions"] if a["id"] == "walk")
        walk["loop"] = False
        walk["frames"].reverse()
        walk["frames"][0].update(ticks=9, x=-5, y=7)
        with zipfile.ZipFile(io.BytesIO(server.export_pack(project))) as archive:
            actions = ET.fromstring(archive.read("conf/actions.xml"))
            exported = actions.find("./ActionList/Action[@Name='Editor_walk']")
            self.assertEqual(exported.attrib["Duration"], "27")
            pose = exported.find("./Animation/Pose")
            self.assertEqual(pose.attrib["Image"], "/shime3.png")
            self.assertEqual(pose.attrib["ImageAnchor"], "69,121")
            self.assertEqual(pose.attrib["Duration"], "9")
            behaviors = ET.fromstring(archive.read("conf/behaviors.xml"))
            default = behaviors.find("./BehaviorList/Behavior[@Name='Editor_walk']")
            self.assertEqual(default.attrib["Frequency"], "1")
            self.assertIn("floor.isOn", default.attrib["Condition"])

    def test_export_rejects_unsafe_paths_sources_invalid_timing_and_dangling_rules(self):
        changes = [
            lambda p: p["assets"][0].update(id="../../escaped"),
            lambda p: p["assets"][0].update(src="/etc/passwd"),
            lambda p: p["assets"][0].update(src="https://example.com/image.png"),
            lambda p: p["actions"][0]["frames"][0].update(ticks=0),
            lambda p: p["actions"][0]["frames"][0].update(ticks=True),
            lambda p: p["rules"][0].update(actionId="missing"),
            lambda p: p["regions"][0].update(width=128),
        ]
        for change in changes:
            project = copy.deepcopy(server.demo_project())
            change(project)
            with self.assertRaises(ValueError):
                server.export_pack(project)

    def test_part_export_preserves_source_rig_and_bakes_the_entire_cycle(self):
        project = server.demo_project()
        action = next(a for a in project["actions"] if a["id"] == "breathe")
        with zipfile.ZipFile(io.BytesIO(server.export_pack(project))) as archive:
            saved = json.loads(archive.read("editor-project.json"))
            self.assertEqual(saved["parts"], project["parts"])
            self.assertEqual(saved["actions"][-1]["tracks"], action["tracks"])
            xml = ET.fromstring(archive.read("conf/actions.xml"))
            exported = xml.find("./ActionList/Action[@Name='Editor_breathe']")
            self.assertEqual(exported.attrib["Duration"], "50")
            poses = exported.findall("./Animation/Pose")
            self.assertEqual(sum(int(p.attrib["Duration"]) for p in poses), 50)
            self.assertTrue(all(p.attrib["Image"].startswith("/baked-") for p in poses))
            for pose in poses:
                self.assertIn("img" + pose.attrib["Image"], archive.namelist())

    def test_invalid_part_graph_keyframes_and_bake_budget_are_rejected(self):
        changes = [
            lambda p: p["parts"][0].update(parentId=p["parts"][1]["id"]),
            lambda p: p["parts"][0].update(parentId="missing"),
            lambda p: p["parts"][0].update(scaleX=0),
            lambda p: p["parts"][0].update(x=float("nan")),
            lambda p: p["actions"][-1]["tracks"][0]["keys"][1].update(tick=0),
            lambda p: p["actions"][-1]["tracks"][0]["keys"][1].update(assetId="missing"),
            lambda p: p["actions"][-1]["tracks"][0].update(partId="missing"),
            lambda p: p["actions"][-1].update(durationTicks=1001),
        ]
        for change in changes:
            project = copy.deepcopy(server.demo_project())
            change(project)
            with self.assertRaises(ValueError):
                server.export_pack(project)

    def test_parent_pivot_rotation_visibility_and_key_interpolation_have_expected_pixels(self):
        from PIL import Image
        red = Image.new("RGBA", (128, 128))
        blue = Image.new("RGBA", (128, 128))
        red.paste((255, 0, 0, 255), (0, 0, 8, 8))
        blue.paste((0, 0, 255, 255), (0, 0, 4, 4))
        images = {}
        for name, image in [("red", red), ("blue", blue)]:
            data = io.BytesIO()
            image.save(data, format="PNG")
            images[name] = data.getvalue()
        body = {"id": "root", "name": "Root", "assetId": "red", "parentId": None,
                "x": 40, "y": 40, "pivotX": 0, "pivotY": 0, "rotation": 90,
                "scaleX": 1, "scaleY": 1, "visible": True}
        child = {**body, "id": "child", "name": "Child", "assetId": "blue", "parentId": "root",
                 "x": 10, "y": 0, "rotation": 0}
        action = {"kind": "parts", "durationTicks": 4, "tracks": []}
        project = {"parts": [body, child], "actions": [action]}
        frames, rendered = parts.bake_action(project, action, images)
        self.assertEqual(len(frames), 1)  # equal adjacent poses are losslessly merged
        image = Image.open(io.BytesIO(rendered[frames[0]["assetId"]]))
        self.assertEqual(image.getpixel((37, 51)), (0, 0, 255, 255))
        self.assertEqual(image.getpixel((35, 43)), (255, 0, 0, 255))
        pose = {field: body[field] for field in (*parts.FIELDS, "assetId", "visible")}
        action["tracks"] = [{"partId": "root", "keys": [{**pose, "tick": 0}, {**pose, "tick": 4, "x": 60}]}]
        self.assertEqual(parts.pose_at(body, action, 2)["x"], 50)
        body["visible"] = False
        action["tracks"] = []
        frames, rendered = parts.bake_action(project, action, images)
        self.assertIsNone(Image.open(io.BytesIO(rendered[frames[0]["assetId"]])).getbbox())

    def test_higher_draw_order_paints_later_and_opacity_scales_alpha(self):
        from PIL import Image
        red = Image.new("RGBA", (128, 128), (255, 0, 0, 255))
        blue = Image.new("RGBA", (128, 128), (0, 0, 255, 255))
        images = {}
        for name, image in [("red", red), ("blue", blue)]:
            data = io.BytesIO()
            image.save(data, format="PNG")
            images[name] = data.getvalue()
        back = {"id": "back", "name": "Back", "assetId": "red", "parentId": None,
                "x": 0, "y": 0, "pivotX": 0, "pivotY": 0, "rotation": 0,
                "scaleX": 1, "scaleY": 1, "visible": True, "opacity": 1, "zOrder": 0}
        front = {**back, "id": "front", "name": "Front", "assetId": "blue", "opacity": 0.5, "zOrder": 1}
        action = {"kind": "parts", "durationTicks": 1, "tracks": []}
        project = {"parts": [front, back], "actions": [action]}
        frames, rendered = parts.bake_action(project, action, images)
        pixel = Image.open(io.BytesIO(rendered[frames[0]["assetId"]])).getpixel((0, 0))
        self.assertEqual(pixel[2], 128)
        self.assertGreater(pixel[0], 100)
        front["zOrder"] = -1
        frames, rendered = parts.bake_action(project, action, images)
        self.assertEqual(Image.open(io.BytesIO(rendered[frames[0]["assetId"]])).getpixel((0, 0)), (255, 0, 0, 255))


class HostedServiceTests(unittest.TestCase):
    def tearDown(self):
        os.environ.pop("VERCEL", None)

    def test_local_editor_rejects_any_other_host(self):
        self.assertTrue(server.request_allowed("127.0.0.1:4317", None, 4317, False))
        self.assertTrue(server.request_allowed("localhost:4317", "http://localhost:4317", 4317, False))
        self.assertFalse(server.request_allowed("editor.vercel.app", None, 4317, False))
        self.assertFalse(server.request_allowed("127.0.0.1:4317", "https://editor.vercel.app", 4317, False))

    def test_hosted_editor_accepts_only_its_own_site(self):
        self.assertTrue(server.request_allowed("editor.vercel.app", "https://editor.vercel.app", 443, True))
        self.assertTrue(server.request_allowed("editor.vercel.app:443", None, 443, True))
        self.assertFalse(server.request_allowed("editor.vercel.app", "https://evil.example", 443, True))
        self.assertFalse(server.request_allowed("evil.example/editor", None, 443, True))

    def test_hosted_catalog_lists_sources_without_downloading_a_pack(self):
        os.environ["VERCEL"] = "1"
        listed = self.exchange("GET", "/api/catalog")
        self.assertEqual(listed.status, 200)
        self.assertIn("sources", json.loads(listed.body))
        opened = self.exchange("POST", "/api/catalog/open", b'{"id":"wispy-miku"}')
        self.assertEqual(opened.status, 400)
        self.assertIn("不代下载", json.loads(opened.body)["error"])

    def test_hosted_media_over_four_megabytes_is_refused_before_decoding(self):
        os.environ["VERCEL"] = "1"
        refused = self.exchange("POST", "/api/media/decode?name=big.png", b"x",
                                length=server.HOSTED_BODY + 1)
        self.assertEqual(refused.status, 400)
        self.assertIn("4 MB", json.loads(refused.body)["error"])

    def exchange(self, method, path, body=b"", length=None):
        payload = body if length is None else b""
        headers = {
            "Host": "editor.vercel.app",
            "Origin": "https://editor.vercel.app",
            "Content-Length": str(len(body) if length is None else length),
        }
        bridge = Call(path, headers, payload)
        if method == "POST":
            server.Handler.do_POST(bridge)
        else:
            server.Handler.do_GET(bridge)
        bridge.body = bridge.wfile.getvalue()
        return bridge


class Call(server.Handler):
    def __init__(self, path, headers, body):
        self.path = path
        self.headers = headers
        self.rfile = io.BytesIO(body)
        self.wfile = io.BytesIO()
        self.connection = SimpleNamespace(settimeout=lambda _timeout: None)
        self.server = SimpleNamespace(server_port=443)
        self.status = None
        self.body = b""

    def send_response(self, code, message=None):
        self.status = code

    def send_header(self, key, value):
        return None

    def end_headers(self):
        return None

    def log_message(self, fmt, *args):
        return None


if __name__ == "__main__":
    unittest.main()
