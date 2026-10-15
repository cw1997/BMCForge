"""Fail closed if a release is missing any architecture or its checksum."""
import hashlib
import json
import os
from pathlib import Path
import tomllib
from package_release import TARGETS


def check(root, tag):
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    if tag != "v" + version and not tag.startswith("v" + version + "-"):
        raise ValueError(f"tag {tag} does not match Cargo version {version}")
    for target in TARGETS:
        stem = f"bmcforge-{version}-{target}"
        archive = root / "dist" / (stem + (".zip" if "windows" in target else ".tar.gz"))
        checksum = archive.with_name(archive.name + ".sha256").read_text().split()[0]
        if hashlib.sha256(archive.read_bytes()).hexdigest() != checksum:
            raise ValueError(f"checksum mismatch: {target}")
        status = json.loads((root / "dist" / (stem + ".build-status.json")).read_text())
        if status["target"] != target or any(status[key] != "passed" for key in ["build", "unit_and_protocol_tests", "executable_smoke_test"]):
            raise ValueError(f"incomplete verification: {target}")


if __name__ == "__main__":
    check(Path(__file__).resolve().parents[1], os.environ["RELEASE_TAG"])
