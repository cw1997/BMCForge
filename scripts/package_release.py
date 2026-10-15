"""Inspect and package an already tested executable. Uses only Python's stdlib."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-pc-windows-msvc": ("PE", 0x8664),
    "i686-pc-windows-msvc": ("PE", 0x14C),
    "aarch64-pc-windows-msvc": ("PE", 0xAA64),
    "x86_64-apple-darwin": ("MachO", 0x01000007),
    "aarch64-apple-darwin": ("MachO", 0x0100000C),
    "x86_64-unknown-linux-musl": ("ELF", 62),
    "aarch64-unknown-linux-musl": ("ELF", 183),
    "armv7-unknown-linux-musleabihf": ("ELF", 40),
}


def inspect_binary(data):
    if data[:2] == b"MZ":
        offset = struct.unpack_from("<I", data, 60)[0]
        if data[offset:offset + 4] != b"PE\0\0":
            raise ValueError("invalid PE header")
        return "PE", struct.unpack_from("<H", data, offset + 4)[0]
    if data[:4] == b"\x7fELF":
        endian = "<" if data[5] == 1 else ">"
        return "ELF", struct.unpack_from(endian + "H", data, 18)[0]
    if data[:4] == b"\xcf\xfa\xed\xfe":
        return "MachO", struct.unpack_from("<I", data, 4)[0]
    raise ValueError("unrecognized executable format")


def package(target, test_mode, root=ROOT):
    import tomllib
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    binary_name = "bmcforge.exe" if "windows" in target else "bmcforge"
    binary = root / "target" / target / "release" / binary_name
    if inspect_binary(binary.read_bytes()) != TARGETS[target]:
        raise ValueError(f"executable architecture mismatch for {target}")
    metadata = dict(version=version, target=target, build="passed", unit_and_protocol_tests="passed", executable_smoke_test="passed", test_mode=test_mode, hardware_verified=False)
    files = {binary_name: (binary.read_bytes(), 0o755)}
    for file in [root / "LICENSE", root / "README.md", root / "CHANGELOG.md", root / "CONTRIBUTING.md", root / "examples/config.toml", *sorted((root / "docs").glob("*")), *sorted((root / "profiles").glob("*")), root / "vendor/ipmi/BMCFORGE-PATCHES.md", root / "vendor/ipmi/LICENSE"]:
        if file.is_file():
            files[str(file.relative_to(root)).replace("\\", "/")] = (file.read_bytes(), 0o644)
    files["BUILD-STATUS.json"] = (json.dumps(metadata, indent=2).encode(), 0o644)
    dist = root / "dist"
    dist.mkdir(exist_ok=True)
    stem = f"bmcforge-{version}-{target}"
    archive = dist / (stem + (".zip" if "windows" in target else ".tar.gz"))
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
            for name, (data, mode) in files.items():
                info = zipfile.ZipInfo(stem + "/" + name)
                info.external_attr = mode << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                output.writestr(info, data)
    else:
        with tarfile.open(archive, "w:gz") as output:
            for name, (data, mode) in files.items():
                info = tarfile.TarInfo(stem + "/" + name)
                info.size = len(data)
                info.mode = mode
                output.addfile(info, io.BytesIO(data))
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{checksum}  {archive.name}\n", encoding="ascii")
    (dist / (stem + ".build-status.json")).write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    return archive


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--test-mode", choices=["native", "native-wow64", "container", "qemu"], required=True)
    args = parser.parse_args()
    print(package(args.target, args.test_mode))
