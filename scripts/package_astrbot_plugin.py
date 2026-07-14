#!/usr/bin/env python3
"""Build the two AstrBot plugin ZIP archives from an explicit allowlist."""

from __future__ import annotations

import argparse
import os
import stat
import tempfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import Sequence


ARCHIVE_ROOT = "astrbot_plugin_maimai_auto_send_images"
INCLUDED_FILES = (
    "metadata.yaml",
    "__init__.py",
    "README.md",
    "_conf_schema.json",
    "direct_render.py",
    "image_paths.py",
    "main.py",
)
ARCHIVE_FILENAMES = (
    f"{ARCHIVE_ROOT}.zip",
    f"{ARCHIVE_ROOT}-install.zip",
)
FIXED_ZIP_TIMESTAMP = (1980, 1, 1, 0, 0, 0)
DIRECTORY_MODE = stat.S_IFDIR | 0o755
FILE_MODE = stat.S_IFREG | 0o644
COMPRESSION_LEVEL = 9


def default_source_dir() -> Path:
    return (
        Path(__file__).resolve().parents[1]
        / "deploy"
        / "astrbot"
        / "plugins"
        / ARCHIVE_ROOT
    )


def _validate_source_dir(source_dir: Path) -> Path:
    source = Path(source_dir)
    if source.is_symlink():
        raise ValueError("plugin source directory must not be a symbolic link")
    if not source.is_dir():
        raise ValueError(f"plugin source directory does not exist: {source}")
    for relative_name in INCLUDED_FILES:
        path = source / relative_name
        if path.is_symlink():
            raise ValueError(
                f"plugin source file must not be a symbolic link: {relative_name}"
            )
        if not path.is_file():
            raise ValueError(f"required plugin source file is missing: {relative_name}")
    return source


def _source_payloads(source_dir: Path) -> dict[str, bytes]:
    source = _validate_source_dir(source_dir)
    return {name: (source / name).read_bytes() for name in INCLUDED_FILES}


def _zip_info(filename: str, *, mode: int, is_directory: bool) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(filename=filename, date_time=FIXED_ZIP_TIMESTAMP)
    info.create_system = 3
    info.create_version = 20
    info.extract_version = 20
    info.external_attr = mode << 16
    if is_directory:
        info.external_attr |= 0x10
        info.compress_type = zipfile.ZIP_STORED
    else:
        info.compress_type = zipfile.ZIP_DEFLATED
    info.extra = b""
    info.comment = b""
    return info


def _write_archive(archive_path: Path, payloads: dict[str, bytes]) -> None:
    with zipfile.ZipFile(
        archive_path,
        mode="w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=COMPRESSION_LEVEL,
        allowZip64=True,
    ) as archive:
        archive.comment = b""
        root_info = _zip_info(
            f"{ARCHIVE_ROOT}/",
            mode=DIRECTORY_MODE,
            is_directory=True,
        )
        archive.writestr(root_info, b"", compress_type=zipfile.ZIP_STORED)
        for relative_name in INCLUDED_FILES:
            file_info = _zip_info(
                f"{ARCHIVE_ROOT}/{relative_name}",
                mode=FILE_MODE,
                is_directory=False,
            )
            archive.writestr(
                file_info,
                payloads[relative_name],
                compress_type=zipfile.ZIP_DEFLATED,
                compresslevel=COMPRESSION_LEVEL,
            )


def _validate_member_path(filename: str) -> None:
    if "\\" in filename:
        raise ValueError(f"archive member uses a non-portable separator: {filename}")
    path = PurePosixPath(filename)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError(f"archive member escapes its root: {filename}")
    if "__pycache__" in path.parts or path.suffix in {".pyc", ".pyo"}:
        raise ValueError(f"archive contains a Python cache file: {filename}")


def verify_plugin_archive(archive_path: Path, source_dir: Path) -> None:
    """Verify exact members, normalized metadata, and byte parity with source."""

    source = _validate_source_dir(source_dir)
    expected_names = [
        f"{ARCHIVE_ROOT}/",
        *(f"{ARCHIVE_ROOT}/{name}" for name in INCLUDED_FILES),
    ]
    try:
        with zipfile.ZipFile(archive_path, mode="r") as archive:
            names = archive.namelist()
            if names != expected_names:
                raise ValueError(
                    f"archive member list differs from allowlist: expected {expected_names!r}, got {names!r}"
                )
            if archive.comment:
                raise ValueError("archive comment must be empty")
            bad_member = archive.testzip()
            if bad_member is not None:
                raise ValueError(f"archive CRC check failed: {bad_member}")

            for index, info in enumerate(archive.infolist()):
                _validate_member_path(info.filename)
                if info.date_time != FIXED_ZIP_TIMESTAMP:
                    raise ValueError(
                        f"archive timestamp is not normalized: {info.filename}"
                    )
                if info.create_system != 3:
                    raise ValueError(
                        f"archive member is missing Unix metadata: {info.filename}"
                    )
                if info.extra or info.comment:
                    raise ValueError(
                        f"archive member has non-deterministic metadata: {info.filename}"
                    )

                mode = info.external_attr >> 16
                if index == 0:
                    if not info.is_dir() or not stat.S_ISDIR(mode):
                        raise ValueError("archive root entry is not a directory")
                    if stat.S_IMODE(mode) != 0o755:
                        raise ValueError("archive root permissions must be 0755")
                    if info.compress_type != zipfile.ZIP_STORED:
                        raise ValueError(
                            "archive root entry must be stored without compression"
                        )
                    continue

                relative_name = INCLUDED_FILES[index - 1]
                if info.is_dir() or not stat.S_ISREG(mode):
                    raise ValueError(
                        f"archive member is not a regular file: {info.filename}"
                    )
                if stat.S_IMODE(mode) != 0o644:
                    raise ValueError(
                        f"archive member permissions must be 0644: {info.filename}"
                    )
                if info.compress_type != zipfile.ZIP_DEFLATED:
                    raise ValueError(
                        f"archive member must use deflate compression: {info.filename}"
                    )
                source_bytes = (source / relative_name).read_bytes()
                if info.file_size != len(source_bytes):
                    raise ValueError(
                        f"archive member size differs from source: {relative_name}"
                    )
                if archive.read(info) != source_bytes:
                    raise ValueError(
                        f"archive member differs from source: {relative_name}"
                    )
    except zipfile.BadZipFile as exc:
        raise ValueError(f"invalid plugin ZIP archive: {archive_path}") from exc


def build_plugin_archives(
    source_dir: Path,
    output_dir: Path,
    *,
    overwrite: bool = False,
) -> tuple[Path, Path]:
    """Build and verify normal/install archives without implicit overwrite."""

    source = _validate_source_dir(source_dir)
    payloads = _source_payloads(source)
    destination_dir = Path(output_dir)
    destination_dir.mkdir(parents=True, exist_ok=True)
    destinations = tuple(destination_dir / name for name in ARCHIVE_FILENAMES)

    existing = [path for path in destinations if path.exists() or path.is_symlink()]
    if existing and not overwrite:
        names = ", ".join(path.name for path in existing)
        raise FileExistsError(
            f"refusing to overwrite existing plugin archive(s): {names}"
        )
    for path in existing:
        if path.is_symlink():
            raise ValueError(
                f"plugin archive destination must not be a symbolic link: {path}"
            )
        if not path.is_file():
            raise ValueError(
                f"plugin archive destination is not a regular file: {path}"
            )

    temporary_paths: list[Path] = []
    try:
        for archive_name in ARCHIVE_FILENAMES:
            descriptor, temporary_name = tempfile.mkstemp(
                prefix=f".{archive_name}.",
                suffix=".tmp",
                dir=destination_dir,
            )
            os.close(descriptor)
            temporary_path = Path(temporary_name)
            temporary_paths.append(temporary_path)
            _write_archive(temporary_path, payloads)
            temporary_path.chmod(0o644)
            verify_plugin_archive(temporary_path, source)

        first_bytes = temporary_paths[0].read_bytes()
        if temporary_paths[1].read_bytes() != first_bytes:
            raise ValueError(
                "normal and install plugin archives are not byte-identical"
            )

        for temporary_path, destination in zip(
            temporary_paths,
            destinations,
            strict=True,
        ):
            os.replace(temporary_path, destination)
        temporary_paths.clear()

        for destination in destinations:
            verify_plugin_archive(destination, source)
        if destinations[0].read_bytes() != destinations[1].read_bytes():
            raise ValueError("written normal and install plugin archives differ")
        return destinations  # type: ignore[return-value]
    finally:
        for temporary_path in temporary_paths:
            try:
                temporary_path.unlink()
            except FileNotFoundError:
                pass


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Deterministically build the AstrBot maimai plugin ZIP archives."
    )
    source = default_source_dir()
    parser.add_argument("--source-dir", type=Path, default=source)
    parser.add_argument("--output-dir", type=Path, default=source.parent)
    parser.add_argument(
        "--force",
        action="store_true",
        help="replace the two existing archive files after successful temporary builds",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        archives = build_plugin_archives(
            args.source_dir,
            args.output_dir,
            overwrite=bool(args.force),
        )
    except (FileExistsError, OSError, ValueError) as exc:
        parser.error(str(exc))
    for archive in archives:
        print(f"built {archive}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
