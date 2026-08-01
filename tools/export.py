#!/usr/bin/env python3
"""Compile canonical Shipmates sources for a harness.

Usage:
    python3 tools/export.py build --target claude-code
    python3 tools/export.py build --target claude-code --check
    python3 tools/export.py check --target claude-code

The exporter is intentionally stdlib-only. Build is transactional in memory:
source and schema errors happen before any generated file is written.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.adapter_contract import CanonicalOrder, CanonicalRole
from tools.adapters.registry import create as create_adapter
from tools.capability_registry import CAPABILITIES, CapabilityError, load_registry


class ExportError(ValueError):
    """Canonical source, target, or adapter contract is invalid."""


KEY_RE = re.compile(r"^[A-Za-z0-9_-]+$")
NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
ARGUMENT_RE = re.compile(r"^[a-z][a-z0-9_-]*$")
INVOCATION_RE = re.compile(r"^@\{\{role\}\}\(\{\{([a-z][a-z0-9_-]*)\}\}\)$")
WEB_SCOPES = ("search", "fetch")
READ_SCOPES = ("read", "search", "glob")
TOOL_SCOPES = (
    "read",
    "search",
    "glob",
    "write",
    "edit",
    "bash",
    "web-search",
    "web-fetch",
    "agent",
)


def _frontmatter(path: Path) -> tuple[dict[str, str], str]:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        raise ExportError(f"{path}: missing opening frontmatter")
    try:
        close = next(index for index, line in enumerate(lines[1:], 1) if line.strip() == "---")
    except StopIteration as exc:
        raise ExportError(f"{path}: unterminated frontmatter") from exc
    values: dict[str, str] = {}
    for lineno, line in enumerate(lines[1:close], 2):
        if not line.strip():
            continue
        key, separator, value = line.partition(":")
        if not separator or not KEY_RE.fullmatch(key) or not value.strip():
            raise ExportError(f"{path}:{lineno}: expected `key: value`")
        if key in values:
            raise ExportError(f"{path}:{lineno}: duplicate key {key!r}")
        values[key] = value.strip()
    return values, "\n".join(lines[close + 1 :])


def _required(values: dict[str, str], path: Path, keys: tuple[str, ...]) -> None:
    missing = [key for key in keys if key not in values]
    if missing:
        raise ExportError(f"{path}: missing canonical field(s): {', '.join(missing)}")


def _bool(value: str, path: Path, field: str) -> bool:
    if value.lower() not in ("true", "false"):
        raise ExportError(f"{path}: {field} must be true or false")
    return value.lower() == "true"


def _arguments(value: str, path: Path) -> tuple[str, ...]:
    arguments = tuple(item.strip() for item in value.split(",") if item.strip())
    if not arguments or any(not ARGUMENT_RE.fullmatch(item) for item in arguments):
        raise ExportError(f"{path}: arguments must be unique lowercase names")
    if len(set(arguments)) != len(arguments):
        raise ExportError(f"{path}: arguments must not contain duplicates")
    return arguments


def _stages(
    value: str, path: Path, loop_max: int, role_names: set[str]
) -> tuple[dict[str, str | int], ...]:
    try:
        raw = json.loads(value)
    except json.JSONDecodeError as exc:
        raise ExportError(f"{path}: stages must be valid JSON") from exc
    if not isinstance(raw, list) or not raw:
        raise ExportError(f"{path}: stages must be a non-empty list")
    expected = ("order", "stage", "role", "gate", "max_loops")
    parsed: list[dict[str, str | int]] = []
    for index, stage in enumerate(raw, 1):
        if not isinstance(stage, dict) or set(stage) != set(expected):
            raise ExportError(f"{path}: stage {index} must contain {', '.join(expected)}")
        if (
            isinstance(stage["order"], bool)
            or not isinstance(stage["order"], int)
            or stage["order"] != index
        ):
            raise ExportError(f"{path}: stages must be ordered starting at 1")
        role = stage["role"]
        if not isinstance(role, str) or not NAME_RE.fullmatch(role) or role not in role_names:
            raise ExportError(f"{path}: stage {index} references unknown role {role!r}")
        for field in ("stage", "gate"):
            if not isinstance(stage[field], str) or not stage[field].strip():
                raise ExportError(f"{path}: stage {index} field {field} must be non-empty")
        max_loops = stage["max_loops"]
        if isinstance(max_loops, bool) or not isinstance(max_loops, int) or not 1 <= max_loops <= loop_max:
            raise ExportError(f"{path}: stage {index} max_loops must be between 1 and loop_max")
        parsed.append(dict(stage))
    return tuple(parsed)


def _source(root: Path, relative: str, owner: Path) -> Path:
    candidate = (root / relative).resolve()
    if candidate != root and root not in candidate.parents:
        raise ExportError(f"{owner}: source escapes repository: {relative!r}")
    if not candidate.is_file():
        raise ExportError(f"{owner}: source not found: {relative}")
    return candidate


def _reference(root: Path, relative: str, owner: Path) -> Path:
    """Validate provenance path without making compatibility content a dependency."""
    candidate = (root / relative).resolve()
    if candidate == root or root not in candidate.parents:
        raise ExportError(f"{owner}: reference escapes repository: {relative!r}")
    return candidate


def load_catalog(root: Path) -> tuple[list[CanonicalRole], list[CanonicalOrder]]:
    root = root.resolve()
    manifest_path = root / "canonical/manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError) as exc:
        raise ExportError(f"invalid canonical manifest: {manifest_path}") from exc
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise ExportError("canonical manifest schema_version must be 1")
    if not isinstance(manifest.get("schema"), str):
        raise ExportError("canonical manifest: schema is required")
    schema_path = _source(root, manifest["schema"], manifest_path)
    try:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ExportError(f"invalid canonical schema: {schema_path}") from exc
    if (
        not isinstance(schema, dict)
        or schema.get("schema_version") != 1
        or not isinstance(schema.get("crew"), dict)
        or not isinstance(schema.get("orders"), dict)
        or schema["crew"].get("body") != "authoritative-persona-body"
        or schema["orders"].get("narrative") != "authoritative workflow body"
    ):
        raise ExportError(f"invalid canonical schema: {schema_path}")
    for section in ("crew", "orders"):
        if not isinstance(manifest.get(section), dict) or not isinstance(
            manifest[section].get("source_root"), str
        ):
            raise ExportError(f"canonical manifest: {section}.source_root is required")
    targets = manifest.get("targets")
    if (
        not isinstance(targets, list)
        or not all(isinstance(target, str) and NAME_RE.fullmatch(target) for target in targets)
        or len(set(targets)) != len(targets)
    ):
        raise ExportError("canonical manifest: targets must be a list of valid target names")
    statuses = manifest.get("target_status")
    if not isinstance(statuses, dict) or any(
        not isinstance(name, str)
        or not isinstance(status, str)
        or status not in ("implemented", "registered-not-implemented")
        for name, status in statuses.items()
    ):
        raise ExportError("canonical manifest: target_status must contain known status strings")
    for target in manifest["targets"]:
        if statuses.get(target) != "implemented":
            raise ExportError(f"canonical manifest: enabled target {target!r} is not implemented")

    roles: list[CanonicalRole] = []
    role_names: set[str] = set()
    crew_dir = root / "canonical/crew"
    for path in sorted(crew_dir.glob("*.md")):
        values, body = _frontmatter(path)
        _required(values, path, ("name", "description", "capabilities", "writes", "source"))
        name = values["name"]
        if name != path.stem or not NAME_RE.fullmatch(name):
            raise ExportError(f"{path}: name must match lowercase filename")
        if name in role_names:
            raise ExportError(f"{path}: duplicate role name {name!r}")
        role_names.add(name)
        capabilities = tuple(item.strip() for item in values["capabilities"].split(",") if item.strip())
        if not capabilities:
            raise ExportError(f"{path}: capabilities must not be empty")
        if len(set(capabilities)) != len(capabilities) or any(
            not NAME_RE.fullmatch(capability) for capability in capabilities
        ):
            raise ExportError(f"{path}: capabilities must be unique lowercase names")
        unknown = sorted(set(capabilities) - set(CAPABILITIES))
        if unknown:
            raise ExportError(f"{path}: unknown capability(s): {', '.join(unknown)}")
        writes = _bool(values["writes"], path, "writes")
        if writes != ("edit" in capabilities):
            raise ExportError(
                f"{path}: writes must match edit capability (writes={writes}, "
                f"capabilities={','.join(capabilities)})"
            )
        source_path = _reference(root, values["source"], path)
        web_scopes = tuple(item.strip() for item in values.get("web-scopes", "").split(",") if item.strip())
        if len(set(web_scopes)) != len(web_scopes) or any(scope not in WEB_SCOPES for scope in web_scopes):
            raise ExportError(f"{path}: web-scopes must contain unique values from {WEB_SCOPES}")
        if "web" in capabilities and not web_scopes:
            raise ExportError(f"{path}: web capability requires web-scopes")
        if "web" not in capabilities and web_scopes:
            raise ExportError(f"{path}: web-scopes requires web capability")
        read_scopes = tuple(item.strip() for item in values.get("read-scopes", "").split(",") if item.strip())
        if len(set(read_scopes)) != len(read_scopes) or any(scope not in READ_SCOPES for scope in read_scopes):
            raise ExportError(f"{path}: read-scopes must contain unique values from {READ_SCOPES}")
        if read_scopes and "read" not in capabilities:
            raise ExportError(f"{path}: read-scopes requires read capability")
        tool_order = tuple(item.strip() for item in values.get("tool-order", "").split(",") if item.strip())
        if len(set(tool_order)) != len(tool_order) or any(scope not in TOOL_SCOPES for scope in tool_order):
            raise ExportError(f"{path}: tool-order must contain unique values from {TOOL_SCOPES}")
        if not body.strip() or "Canonical persona body is retained" in body:
            raise ExportError(f"{path}: canonical persona body is empty or compatibility-only")
        roles.append(
            CanonicalRole(
                name=name,
                description=values["description"],
                capabilities=capabilities,
                writes=writes,
                web_scopes=web_scopes,
                read_scopes=read_scopes,
                tool_order=tool_order,
                source=source_path,
                body=body,
            )
        )
    if not roles:
        raise ExportError("canonical/crew: no role sources")

    orders: list[CanonicalOrder] = []
    order_names: set[str] = set()
    orders_dir = root / "canonical/orders"
    for path in sorted(orders_dir.glob("*.md")):
        values, body = _frontmatter(path)
        _required(
            values,
            path,
            (
                "name",
                "source",
                "description",
                "argument-hint",
                "allowed-tools",
                "disable-model-invocation",
                "arguments",
                "loop_max",
                "stages",
                "invocation",
                "board",
            ),
        )
        name = values["name"]
        if name != path.stem or not NAME_RE.fullmatch(name):
            raise ExportError(f"{path}: name must match lowercase filename")
        if name in order_names:
            raise ExportError(f"{path}: duplicate order name {name!r}")
        order_names.add(name)
        try:
            loop_max = int(values["loop_max"])
        except ValueError as exc:
            raise ExportError(f"{path}: loop_max must be an integer") from exc
        if loop_max < 1:
            raise ExportError(f"{path}: loop_max must be positive")
        arguments = _arguments(values["arguments"], path)
        stages = _stages(values["stages"], path, loop_max, role_names)
        invocation = values["invocation"]
        match = INVOCATION_RE.fullmatch(invocation)
        if not match or match.group(1) not in arguments:
            raise ExportError(f"{path}: invocation must reference a declared argument")
        if values["board"] not in ("native", "explicit"):
            raise ExportError(f"{path}: board must be native or explicit")
        disable_model_invocation = _bool(
            values["disable-model-invocation"], path, "disable-model-invocation"
        )
        source_path = _reference(root, values["source"], path)
        if not body.strip():
            raise ExportError(f"{path}: canonical narrative must not be empty")
        orders.append(
            CanonicalOrder(
                name=name,
                source=source_path,
                description=values["description"],
                argument_hint=values["argument-hint"],
                allowed_tools=values["allowed-tools"],
                disable_model_invocation=disable_model_invocation,
                arguments=arguments,
                loop_max=loop_max,
                stages=tuple(stages),
                narrative=body,
                invocation=invocation,
                board=values["board"],
            )
        )
    if not orders:
        raise ExportError("canonical/orders: no order sources")
    return roles, orders


def adapter_for(root: Path, target: str):
    if target == "opencode":
        raise ExportError(
            "target 'opencode' is registered for future work but its adapter is not implemented; "
            "this prerequisite export supports claude-code only"
        )
    registries = load_registry(root / "tools/capability_registry.json")
    manifest = json.loads((root / "canonical/manifest.json").read_text(encoding="utf-8"))
    if target not in manifest["targets"]:
        raise ExportError(f"target {target!r} is not enabled by canonical manifest")
    try:
        return create_adapter(target, registries[target])
    except KeyError as exc:
        raise ExportError(f"capability registry has no target: {target}") from exc
    except ValueError as exc:
        raise ExportError(str(exc)) from exc


def _orphan_paths(root: Path, target: str, expected: set[str]) -> list[str]:
    base = root / "harnesses" / target
    if not base.is_dir():
        return []
    return sorted(
        path.relative_to(root).as_posix()
        for path in base.rglob("*")
        if path.is_file() and path.relative_to(root).as_posix() not in expected
    )


def check_files(root: Path, files: dict[str, str], target: str, golden_dir: Path | None = None) -> list[str]:
    report: list[str] = []
    base = golden_dir if golden_dir is not None else root
    prefix = f"harnesses/{target}/"
    for relative, expected in sorted(files.items()):
        if golden_dir is not None:
            golden_rel = relative.removeprefix(prefix)
            path = base / golden_rel
        else:
            path = root / relative
        if not path.is_file():
            report.append(f"missing: {relative}")
        elif path.read_text(encoding="utf-8") != expected:
            report.append(f"drift: {relative}")
    report.extend(f"unexpected generated file: {path}" for path in _orphan_paths(root, target, set(files)))
    return report


def write_files(root: Path, files: dict[str, str], out_dir: Path | None = None) -> list[str]:
    written = []
    for relative, content in sorted(files.items()):
        if out_dir is not None:
            path = (out_dir / relative).resolve()
        else:
            path = (root / relative).resolve()
            generated_root = (root / "harnesses").resolve()
            if generated_root not in path.parents:
                raise ExportError(f"refusing to write outside harnesses/: {relative}")
        if path.is_file() and path.read_text(encoding="utf-8") == content:
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
        temporary.write_text(content, encoding="utf-8", newline="\n")
        os.replace(temporary, path)
        written.append(relative)
    return written


def canonical_digest(root: Path) -> str:
    lines: list[str] = []
    for path in sorted(path for path in (root / "canonical").rglob("*") if path.is_file()):
        relative = path.relative_to(root).as_posix()
        lines.extend((relative, hashlib.sha256(path.read_bytes()).hexdigest()))
    return hashlib.sha256(("\n".join(lines) + "\n").encode("utf-8")).hexdigest()


def payload_attestation(files: dict[str, str], target: str, source_digest: str) -> str:
    """Return exporter-owned provenance for installer trust checks."""
    lines = [
        "payload_version=1",
        "generator=shipmates-exporter",
        f"target={target}",
        f"canonical_sha256={source_digest}",
    ]
    prefix = f"harnesses/{target}/"
    for relative, content in sorted(files.items()):
        if not relative.startswith(prefix):
            raise ExportError(f"payload path is outside target: {relative}")
        payload_relative = relative[len(prefix) :]
        digest = hashlib.sha256(content.encode("utf-8")).hexdigest()
        lines.append(f"file={payload_relative} sha256={digest}")
    return "\n".join(lines) + "\n"


def run(root: Path, target: str, check: bool, out_dir: Path | None = None) -> int:
    try:
        roles, orders = load_catalog(root)
        adapter = adapter_for(root, target)
        files = adapter.build(root, roles, orders)
        files[f"harnesses/{target}/.shipmates-payload"] = payload_attestation(
            files, target, canonical_digest(root)
        )
        if check:
            golden_dir = root / "tests/golden" / target
            report = check_files(root, files, target, golden_dir=golden_dir if golden_dir.is_dir() else None)
            if report:
                print("\n".join(report))
                return 1
            print(f"up to date: {target} ({len(files)} files)")
            return 0
        written = write_files(root, files, out_dir)
        dest = out_dir / f"harnesses/{target}" if out_dir else root / f"harnesses/{target}"
        for path in written:
            print(f"wrote {path}")
        print(f"{dest}: {len(written)} of {len(files)} files updated")
        return 0
    except (CapabilityError, ExportError, OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Export canonical Shipmates sources for a harness.")
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("build", "check"):
        subparser = subparsers.add_parser(command)
        subparser.add_argument("--target", required=True)
        subparser.add_argument("--root", default=str(ROOT))
        subparser.add_argument("--out", default=None, help="output root (default: write to harnesses/ in repo)")
        if command == "build":
            subparser.add_argument("--check", action="store_true", help="check generated golden files")
    args = parser.parse_args(argv)
    out_dir = Path(args.out).resolve() if args.out else None
    return run(
        Path(args.root).resolve(),
        args.target,
        args.command == "check" or getattr(args, "check", False),
        out_dir=out_dir,
    )


if __name__ == "__main__":
    raise SystemExit(main())
