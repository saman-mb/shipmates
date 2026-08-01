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

from tools.adapter_contract import CanonicalCommand, CanonicalRole, conformance_report
from tools.adapters.registry import create as create_adapter
from tools.capability_registry import CAPABILITIES, CapabilityError, load_registry


class ExportError(ValueError):
    """Canonical source, target, or adapter contract is invalid."""


KEY_RE = re.compile(r"^[A-Za-z0-9_-]+$")
NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
ARGUMENT_RE = re.compile(r"^[a-z][a-z0-9_-]*$")
INVOCATION_RE = re.compile(r"^@\{\{role\}\}\(\{\{([a-z][a-z0-9_-]*)\}\}\)$")
# Mirrors tools/validate_skills.py. Positional substitution is not in the Agent
# Skills standard and its index base has moved between harness versions, so a
# `$` before a digit anywhere in a source or a generated payload is a defect —
# fenced code blocks included, because substitution is textual over the file.
POSITIONAL_RE = re.compile(r"(?<!\\)\$\{?[0-9]")
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
) -> tuple[dict[str, object], ...]:
    try:
        raw = json.loads(value)
    except json.JSONDecodeError as exc:
        raise ExportError(f"{path}: stages must be valid JSON") from exc
    if not isinstance(raw, list) or not raw:
        raise ExportError(f"{path}: stages must be a non-empty list")
    expected = ("order", "stage", "roles", "gate", "max_loops")
    parsed: list[dict[str, object]] = []
    for index, stage in enumerate(raw, 1):
        if not isinstance(stage, dict) or set(stage) != set(expected):
            raise ExportError(f"{path}: stage {index} must contain {', '.join(expected)}")
        if (
            isinstance(stage["order"], bool)
            or not isinstance(stage["order"], int)
            or stage["order"] != index
        ):
            raise ExportError(f"{path}: stages must be ordered starting at 1")
        # `roles` is a list because a stage can fan out: /pr-review's board runs
        # a product-manager and an sdet on every PR. A singular field forced that
        # to be written as two sequential stages, which is not what happens.
        roles = stage["roles"]
        if not isinstance(roles, list) or not roles or len(set(roles)) != len(roles):
            raise ExportError(f"{path}: stage {index} roles must be a unique non-empty list")
        for role in roles:
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


def _reject_positional(label: str, text: str) -> None:
    """Fail on `$1`-style placeholders anywhere in `text`, fences included."""
    for lineno, line in enumerate(text.splitlines(), 1):
        hit = POSITIONAL_RE.search(line)
        if hit:
            raise ExportError(
                f"{label}:{lineno}: {hit.group(0)!r} — a command has no positional "
                "arguments; use a named `{{argument}}` token, describe the input in "
                "prose, or escape a literal as `\\$1`"
            )


def _mentioned(body: str, role_names: set[str]) -> set[str]:
    return {role for role in role_names if role in body}


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


def load_manifest(root: Path) -> dict:
    """Read and validate canonical/manifest.json — the exporter's only entry point.

    Every root the exporter reads from or gates against is declared here, so a
    reader can answer "what is authoritative?" from one file, not from the code.
    """
    root = root.resolve()
    manifest_path = root / "tools/manifest.json"
    if not manifest_path.is_file():
        manifest_path = root / "canonical/manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError) as exc:
        raise ExportError(f"invalid manifest: {manifest_path}") from exc
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise ExportError("manifest schema_version must be 1")
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
        or not isinstance(schema.get("commands"), dict)
        or schema["crew"].get("body") != "authoritative-persona-body"
        or schema["commands"].get("narrative") != "authoritative workflow body"
    ):
        raise ExportError(f"invalid canonical schema: {schema_path}")
    for section in ("crew", "commands"):
        if not isinstance(manifest.get(section), dict) or not (
            isinstance(manifest[section].get("source_root"), str) or isinstance(manifest[section].get("canonical_root"), str)
        ):
            raise ExportError(f"manifest: {section}.source_root is required")
        root_dir = manifest[section].get("source_root", manifest[section].get("canonical_root"))
        canonical_root = (root / root_dir).resolve()
        if root not in canonical_root.parents or not canonical_root.is_dir():
            raise ExportError(f"manifest: {section}.source_root is not a directory")
    targets = manifest.get("targets")
    if (
        not isinstance(targets, list)
        or not all(isinstance(target, str) and NAME_RE.fullmatch(target) for target in targets)
        or len(set(targets)) != len(targets)
    ):
        raise ExportError("manifest: targets must be a list of valid target names")
    statuses = manifest.get("target_status")
    if not isinstance(statuses, dict) or any(
        not isinstance(name, str)
        or not isinstance(status, str)
        or status not in ("implemented", "registered-not-implemented")
        for name, status in statuses.items()
    ):
        raise ExportError("manifest: target_status must contain known status strings")
    for target in manifest["targets"]:
        if statuses.get(target) != "implemented":
            raise ExportError(f"manifest: enabled target {target!r} is not implemented")
    return manifest


def load_catalog(root: Path) -> tuple[list[CanonicalRole], list[CanonicalCommand]]:
    root = root.resolve()
    manifest = load_manifest(root)

    roles: list[CanonicalRole] = []
    role_names: set[str] = set()
    crew_dir = root / manifest["crew"].get("source_root", manifest["crew"].get("canonical_root"))
    for path in sorted(crew_dir.glob("*.md")):
        values, body = _frontmatter(path)
        _required(values, path, ("name", "description", "capabilities", "writes"))
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
        source_path = path.relative_to(root)
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

    commands: list[CanonicalCommand] = []
    command_names: set[str] = set()
    commands_dir = root / manifest["commands"].get("source_root", manifest["commands"].get("canonical_root"))
    for path in sorted(commands_dir.glob("*.md")):
        values, body = _frontmatter(path)
        _required(
            values,
            path,
            (
                "name",
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
        if name in command_names:
            raise ExportError(f"{path}: duplicate command name {name!r}")
        command_names.add(name)
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
        source_path = path.relative_to(root)
        if not body.strip():
            raise ExportError(f"{path}: canonical narrative must not be empty")
        _reject_positional(path.relative_to(root).as_posix(), path.read_text(encoding="utf-8"))
        # Stage metadata is only worth carrying if it describes the narrative it
        # ships beside. Requiring every declared role to appear in the body stops
        # a stage table drifting into fiction while the export stays green — the
        # failure mode of metadata no adapter reads yet.
        declared_roles = {str(role) for stage in stages for role in stage["roles"]}  # type: ignore[union-attr]
        absent = sorted(declared_roles - _mentioned(body, role_names))
        if absent:
            raise ExportError(
                f"{path}: stage role(s) never mentioned in the narrative: {', '.join(absent)}"
            )
        commands.append(
            CanonicalCommand(
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
    if not commands:
        raise ExportError("canonical/commands: no command sources")
    return roles, commands


def adapter_for(root: Path, target: str, manifest: dict | None = None):
    """Resolve a target's adapter, refusing anything the manifest hasn't enabled.

    The refusal is declarative: a target's status lives in canonical/manifest.json,
    not in a hardcoded name here, so enabling a second adapter is one manifest
    edit rather than a code change an author has to remember to make.
    """
    manifest = manifest if manifest is not None else load_manifest(root)
    if target not in manifest["targets"]:
        status = manifest["target_status"].get(target)
        if status == "registered-not-implemented":
            raise ExportError(
                f"target {target!r} is registered for future work but its adapter is "
                "not implemented; canonical export supports "
                f"{', '.join(manifest['targets'])} only"
            )
        raise ExportError(f"target {target!r} is not enabled by canonical manifest")
    registries = load_registry(root / "tools/capability_registry.json")
    try:
        adapter = create_adapter(target, registries[target])
    except KeyError as exc:
        raise ExportError(f"capability registry has no target: {target}") from exc
    except ValueError as exc:
        raise ExportError(str(exc)) from exc
    reasons = conformance_report(adapter, target)
    if reasons:
        raise ExportError(f"adapter for {target!r} does not conform: {'; '.join(reasons)}")
    return adapter


def _orphan_paths(base: Path, label: str, expected: set[str]) -> list[str]:
    """Files present under `base` that the export does not produce.

    Run against whichever tree is being compared — the in-repo harnesses/ tree or
    the committed golden reference. Without this, an extra or renamed reference
    file is invisible: every generated file matches, and the stray one is never
    looked at.
    """
    if not base.is_dir():
        return []
    return sorted(
        f"{label}{path.relative_to(base).as_posix()}"
        for path in base.rglob("*")
        if path.is_file() and f"{label}{path.relative_to(base).as_posix()}" not in expected
    )


def check_files(root: Path, files: dict[str, str], target: str, golden_dir: Path | None = None) -> list[str]:
    report: list[str] = []
    prefix = f"harnesses/{target}/"
    for relative, expected in sorted(files.items()):
        if golden_dir is not None:
            path = golden_dir / relative[len(prefix) :]
        else:
            path = root / relative
        if not path.is_file():
            report.append(f"missing: {relative}")
        elif path.read_text(encoding="utf-8") != expected:
            report.append(f"drift: {relative}")
    base = golden_dir if golden_dir is not None else root / "harnesses" / target
    report.extend(
        f"unexpected reference file: {path}" for path in _orphan_paths(base, prefix, set(files))
    )
    return report


def check_compatibility(root: Path, files: dict[str, str], manifest: dict) -> list[str]:
    """Compare the generated payload against the committed compatibility trees.

    `agents/` and `skills/` stay in the repository because the site generator and
    the skills validator read them. That makes them a second writable copy of the
    shipped payload, and a second writable copy with no gate is a trap: a
    contributor edits the tree the layout docs point at and ships nothing. This
    pins them as provably frozen mirrors of the canonical export, so such an edit
    fails CI with the file name in the message instead of passing silently.
    """
    compatibility = manifest.get("compatibility")
    if not isinstance(compatibility, dict):
        return []
    target = compatibility["target"]
    prefix = f"harnesses/{target}/"
    exempt = set(compatibility["exempt"])
    report: list[str] = []
    for relative, expected in sorted(files.items()):
        if not relative.startswith(prefix):
            continue
        payload_relative = relative[len(prefix) :]
        if payload_relative in exempt:
            continue
        path = root / payload_relative
        if not path.is_file():
            report.append(f"missing compatibility source: {payload_relative}")
        elif path.read_text(encoding="utf-8") != expected:
            report.append(f"compatibility drift: {payload_relative}")
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
        # Written through open() rather than Path.write_text(newline=...): the
        # newline keyword only exists on Python 3.10+, and the installer runs
        # this on whatever python3 the machine has (macOS ships 3.9).
        try:
            with open(temporary, "w", encoding="utf-8", newline="\n") as handle:
                handle.write(content)
            os.replace(temporary, path)
        except OSError:
            temporary.unlink(missing_ok=True)
            raise
        written.append(relative)
    return written


def canonical_digest(root: Path, manifest: dict) -> str:
    """Digest exactly the files the exporter reads — the manifest, the schema, and
    the two source roots.
    """
    manifest_file = root / "tools/manifest.json"
    if not manifest_file.is_file():
        manifest_file = root / "canonical/manifest.json"
    inputs = [manifest_file, _source(root, manifest["schema"], root)]
    for section in ("crew", "commands"):
        inputs.extend(sorted((root / manifest[section].get("source_root", manifest[section].get("canonical_root"))).glob("*.md")))
    lines: list[str] = []
    for path in inputs:
        relative = path.relative_to(root).as_posix()
        lines.extend((relative, hashlib.sha256(path.read_bytes()).hexdigest()))
    return hashlib.sha256(("\n".join(lines) + "\n").encode("utf-8")).hexdigest()


def payload_manifest(files: dict[str, str], target: str, source_digest: str) -> str:
    """Return a build manifest: what the exporter produced, and from which inputs.

    This is provenance, not a trust boundary. The installer compiles the payload
    from the canonical sources in the tree it just fetched, so re-hashing that
    same tree at install time proves nothing an attacker could not also arrange —
    do not present this as an integrity check. Its real value is as a build
    tripwire: because `canonical_sha256` covers every canonical file, a canonical
    edit that leaves the rendered output byte-identical still moves this digest,
    so the committed golden reference goes red and the edit gets reviewed.
    """
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


def update_references(root: Path, files: dict[str, str], target: str, manifest: dict) -> list[str]:
    """No-op under zero-duplication architecture."""
    return []


def run(
    root: Path,
    target: str,
    check: bool,
    out_dir: Path | None = None,
    update: bool = False,
) -> int:
    try:
        manifest = load_manifest(root)
        roles, commands = load_catalog(root)
        adapter = adapter_for(root, target, manifest)
        files = adapter.build(root, roles, commands)
        for relative, content in sorted(files.items()):
            _reject_positional(relative, content)
        files[f"harnesses/{target}/.shipmates-payload"] = payload_manifest(
            files, target, canonical_digest(root, manifest)
        )
        if update:
            print(f"up to date: {target} (zero-duplication architecture enabled)")
            return 0
        if check:
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


def list_targets(root: Path) -> int:
    """Print the enabled targets, one per line.

    Exists so a caller can ask "which targets can be built?" instead of inferring
    it from a failed build. install.sh needs that distinction: under `--harness
    all` a target with no adapter is skipped, but a target that *has* one and
    fails to build is a hard error. Inferring the difference from an exit code
    conflates "not implemented yet" with "your canonical sources are corrupt",
    and reports the second as a cheerful skip.
    """
    try:
        for target in load_manifest(root)["targets"]:
            print(target)
        return 0
    except (CapabilityError, ExportError, OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Export canonical Shipmates sources for a harness.")
    subparsers = parser.add_subparsers(dest="command", required=True)
    targets_parser = subparsers.add_parser("targets", help="list enabled targets, one per line")
    targets_parser.add_argument("--root", default=str(ROOT))
    for command in ("build", "check"):
        subparser = subparsers.add_parser(command)
        subparser.add_argument("--target", required=True)
        subparser.add_argument("--root", default=str(ROOT))
        subparser.add_argument("--out", default=None, help="output root (default: write to harnesses/ in repo)")
        if command == "build":
            subparser.add_argument("--check", action="store_true", help="check generated golden files")
            subparser.add_argument(
                "--update",
                action="store_true",
                help="rewrite tests/golden/<target>/ and the agents/ + skills/ mirrors",
            )
    args = parser.parse_args(argv)
    if args.command == "targets":
        return list_targets(Path(args.root).resolve())
    out_dir = Path(args.out).resolve() if args.out else None
    return run(
        Path(args.root).resolve(),
        args.target,
        args.command == "check" or getattr(args, "check", False),
        out_dir=out_dir,
        update=getattr(args, "update", False),
    )


if __name__ == "__main__":
    raise SystemExit(main())
