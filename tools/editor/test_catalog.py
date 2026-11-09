import copy
import hashlib
import io
from pathlib import Path
import tempfile
import unittest
import zipfile

from PIL import Image
import catalog
import server


class CatalogTests(unittest.TestCase):
    def synthetic(self):
        image = Image.new("RGBA", (128, 128), (255, 0, 0, 255))
        png = io.BytesIO()
        image.save(png, format="PNG")
        data = io.BytesIO()
        with zipfile.ZipFile(data, "w") as zipped:
            zipped.writestr("release/Character/conf/actions.xml", '<Mascot><ActionList><Action Name="Stand" Type="Stay"><Animation><Pose Image="/frame.png" ImageAnchor="64,128" Duration="3"/></Animation></Action></ActionList></Mascot>')
            zipped.writestr("release/Character/img/Character/frame.png", png.getvalue())
            zipped.writestr("release/Other/unused.png", png.getvalue())
        return data.getvalue()

    def test_metadata_does_not_imply_direct_import_for_directory_or_live2d(self):
        sources = catalog.read_catalog()["sources"]
        self.assertTrue(any(catalog.importable(source) for source in sources))
        self.assertTrue(all(not catalog.importable(source) for source in sources if source["kind"] != "pack"))

    def test_selected_image_set_is_converted_to_frames_without_inventing_body_layers(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "sample.zip"
            archive.write_bytes(self.synthetic())
            source = {"id": "test", "name": "Character", "url": "https://github.com/a/b", "licenseNote": "Original author",
                      "archivePrefix": "release", "packPrefix": "Character", "archiveSha256": hashlib.sha256(archive.read_bytes()).hexdigest()}
            project = catalog.editor_project(source, archive)
            server.validate_project(project)
            self.assertEqual(len(project["assets"]), 1)
            self.assertEqual(project["parts"], [])
            self.assertEqual(project["rules"], [])
            self.assertEqual(project["actions"][0]["frames"][0]["ticks"], 3)
            self.assertEqual(project["actions"][0]["frames"][0]["y"], 0)
            self.assertIn("没有自动拆分", project["source"]["conversion"])
            with zipfile.ZipFile(io.BytesIO(server.export_pack(project))) as exported:
                self.assertIn("SOURCE.txt", exported.namelist())
                self.assertNotIn("bundled-shime-LICENSE.txt", exported.namelist())

    def test_archive_path_and_special_file_checks_apply_outside_selected_pack(self):
        source = {"archivePrefix": "release", "packPrefix": "Character"}
        for bad in ["../escape.png", "/absolute.png", "release/Other/../../escape.png", "release/Other/C:/escape.png"]:
            data = io.BytesIO()
            with zipfile.ZipFile(data, "w") as zipped:
                zipped.writestr(bad, b"bad")
            with self.assertRaises(ValueError):
                catalog.selected_files(io.BytesIO(data.getvalue()), source)
        data = io.BytesIO()
        with zipfile.ZipFile(data, "w") as zipped:
            link = zipfile.ZipInfo("release/Other/link")
            link.create_system = 3
            link.external_attr = 0o120777 << 16
            zipped.writestr(link, b"../../outside")
        with self.assertRaises(ValueError):
            catalog.selected_files(io.BytesIO(data.getvalue()), source)

    def test_cache_hash_is_reverified_without_network(self):
        old_cache = catalog.CACHE
        with tempfile.TemporaryDirectory() as temporary:
            catalog.CACHE = Path(temporary)
            try:
                data = self.synthetic()
                digest = hashlib.sha256(data).hexdigest()
                source = {"id": "test", "kind": "pack", "archiveUrl": "https://github.com/a/b/archive/commit.zip",
                          "archiveSha256": digest, "archivePrefix": "", "packPrefix": ""}
                cached = Path(temporary) / (digest + ".zip")
                cached.write_bytes(data)
                self.assertEqual(catalog.checked_archive(source, cached_only=True), cached)
                cached.write_bytes(b"changed")
                with self.assertRaises(ValueError):
                    catalog.checked_archive(source, cached_only=True)
            finally:
                catalog.CACHE = old_cache


if __name__ == "__main__":
    unittest.main()
