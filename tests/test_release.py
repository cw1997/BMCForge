import importlib.util
from pathlib import Path
import struct
import unittest
import tempfile
import zipfile
import hashlib
import json
import sys

spec = importlib.util.spec_from_file_location("package_release", Path(__file__).resolve().parents[1] / "scripts/package_release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import check_release
import build_catalog


class ReleaseTests(unittest.TestCase):
    def test_catalog_and_registry_include_every_reviewed_profile(self):
        root=release.ROOT
        inventory=json.loads((root / "docs/compatibility.json").read_text(encoding="utf-8"))
        self.assertEqual(inventory,build_catalog.catalog())
        registry=(root / "crates/profiles/src/lib.rs").read_text(encoding="utf-8")
        for path in (root / "profiles").glob("*.toml"):
            self.assertIn(f'profiles/{path.name}',registry)
        model_registry=(root / "crates/profiles/src/models.rs").read_text(encoding="utf-8")
        for path in (root / "profiles/models").glob("*.toml"):
            self.assertIn(f'profiles/models/{path.name}',model_registry)
        self.assertEqual(inventory['model_count'],len(inventory['models']))
        self.assertEqual(len({m['id'] for m in inventory['models']}),inventory['model_count'])
        for model in inventory['models']:
            self.assertEqual(model['identification'],'catalogued_not_hardware_verified')
            if model.get('profile','').endswith('-monitor'):
                self.assertIsNone(model['fan_adapter'])
                self.assertEqual(model['control_status'],'monitoring_only')

    def test_package_contains_binary_docs_status_and_correct_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.3"\n')
            for name in ["LICENSE","README.md","CHANGELOG.md","examples/config.toml","docs/test.md","profiles/test.toml","profiles/models/test.toml","profiles/models/schema.json","locales/en/locale.toml","locales/en/main.ftl","locales/schema.json","vendor/ipmi/LICENSE"]:
                path=root / name
                path.parent.mkdir(parents=True,exist_ok=True)
                path.write_text("test")
            pe=bytearray(128); pe[:2]=b"MZ"; struct.pack_into("<I",pe,60,80); pe[80:84]=b"PE\0\0"; struct.pack_into("<H",pe,84,0x8664)
            target="x86_64-pc-windows-msvc"
            binary=root / "target" / target / "release/bmcforge.exe"
            binary.parent.mkdir(parents=True); binary.write_bytes(pe)
            archive=release.package(target,"native",root)
            self.assertEqual(archive.with_name(archive.name+".sha256").read_text().split()[0],hashlib.sha256(archive.read_bytes()).hexdigest())
            with zipfile.ZipFile(archive) as output:
                self.assertTrue(any(n.endswith("/bmcforge.exe") for n in output.namelist()))
                for resource in ["profiles/models/test.toml","profiles/models/schema.json","locales/en/locale.toml","locales/en/main.ftl","locales/schema.json"]:
                    self.assertTrue(any(n.endswith('/'+resource) for n in output.namelist()),resource)
                status=json.loads(output.read(next(n for n in output.namelist() if n.endswith("BUILD-STATUS.json"))))
                self.assertEqual(status["target"],target)
                self.assertFalse(status["hardware_verified"])
            # Missing any other target must stop a release, even if one package is valid.
            with self.assertRaises(FileNotFoundError):
                check_release.check(root,"v1.2.3")
            with self.assertRaises(ValueError):
                check_release.check(root,"v9.9.9")
            struct.pack_into("<H",pe,84,0xAA64); binary.write_bytes(pe)
            with self.assertRaises(ValueError):
                release.package(target,"native",root)

    def test_debug_package_name_includes_full_commit_and_timestamp(self):
        import re
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.3"\n')
            pe=bytearray(128); pe[:2]=b"MZ"; struct.pack_into("<I",pe,60,80); pe[80:84]=b"PE\0\0"; struct.pack_into("<H",pe,84,0x8664)
            target="x86_64-pc-windows-msvc"
            binary=root / "target" / target / "debug/bmcforge.exe"
            binary.parent.mkdir(parents=True); binary.write_bytes(pe)
            archive=release.package(target,"native",root,build_type="debug")
            commit,_,_=release.get_git_info()
            self.assertRegex(archive.name,rf"^bmcforge-1\.2\.3-{re.escape(commit)}-\d{{8}}-\d{{6}}-debug-{re.escape(target)}\.zip$")
            with zipfile.ZipFile(archive) as output:
                status=json.loads(output.read(next(n for n in output.namelist() if n.endswith("BUILD-STATUS.json"))))
                self.assertEqual(status["build_type"],"debug")
                self.assertEqual(status["git_commit"],commit)
                self.assertIn("build_datetime",status)

    def test_eight_distinct_targets(self):
        self.assertEqual(len(release.TARGETS), 8)
        self.assertEqual(len(set(release.TARGETS.values())), 8)

    def test_executable_headers(self):
        pe = bytearray(128)
        pe[:2] = b"MZ"
        struct.pack_into("<I", pe, 60, 80)
        pe[80:84] = b"PE\0\0"
        struct.pack_into("<H", pe, 84, 0xAA64)
        self.assertEqual(release.inspect_binary(pe), ("PE", 0xAA64))
        elf = bytearray(32)
        elf[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", elf, 18, 183)
        self.assertEqual(release.inspect_binary(elf), ("ELF", 183))
        macho = b"\xcf\xfa\xed\xfe" + struct.pack("<I", 0x0100000C)
        self.assertEqual(release.inspect_binary(macho), ("MachO", 0x0100000C))
        with self.assertRaises(ValueError):
            release.inspect_binary(b"invalid")


if __name__ == "__main__":
    unittest.main()
