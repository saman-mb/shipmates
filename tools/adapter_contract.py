#!/usr/bin/env python3
"""Adapter interface for compiling neutral Shipmates sources to a harness."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Protocol, runtime_checkable

try:  # Works both as `tools.adapter_contract` and from tools/ scripts.
    from .capability_registry import HarnessCapabilities
except ImportError:  # pragma: no cover - exercised by direct CLI imports.
    from capability_registry import HarnessCapabilities


@dataclass(frozen=True)
class TargetPaths:
    """Harness-owned locations and project instruction filename."""

    agents: str
    commands: str
    project_instructions: str


@dataclass(frozen=True)
class CanonicalRole:
    name: str
    description: str
    capabilities: tuple[str, ...]
    writes: bool
    source: Path
    body: str
    web_scopes: tuple[str, ...] = ()
    read_scopes: tuple[str, ...] = ()
    tool_order: tuple[str, ...] = ()


@dataclass(frozen=True)
class CanonicalCommand:
    name: str
    source: Path
    description: str
    argument_hint: str
    allowed_tools: str
    disable_model_invocation: bool
    arguments: tuple[str, ...]
    loop_max: int
    stages: tuple[dict[str, object], ...]
    narrative: str
    invocation: str
    board: str


@runtime_checkable
class Adapter(Protocol):
    """Contract every target exporter implements.

    Adapters own target paths, frontmatter emission, capability/tool translation,
    argument and invocation dialects, and explicit board degradation. `build` is
    pure: it returns paths and text; CLI code performs writes and checks.

    `runtime_checkable` makes `isinstance(adapter, Adapter)` a real assertion, so
    `conformance_report` below can be run by an adapter author before their
    target is registered. A structural Protocol nobody can execute documents an
    intent; this one fails a half-implemented adapter out loud.
    """

    name: str
    capabilities: HarnessCapabilities

    def target_paths(self) -> TargetPaths: ...

    def emit_frontmatter(self, kind: str, values: dict[str, str]) -> str: ...

    def map_tools(
        self,
        capabilities: Iterable[str],
        web_scopes: Iterable[str] = (),
        read_scopes: Iterable[str] = (),
        tool_order: Iterable[str] = (),
    ) -> tuple[str, ...]: ...

    def render_neutral(self, text: str) -> str: ...

    def render_args(self, text: str, arguments: dict[str, str]) -> str: ...

    def render_invocation(self, role: str, argument: str) -> str: ...

    def degrade_board(self, stage: str) -> str: ...

    def build(
        self, root: Path, roles: Iterable[CanonicalRole], commands: Iterable[CanonicalCommand]
    ) -> dict[str, str]: ...


#: Every member an adapter must supply. Kept as data so the conformance check can
#: name the missing pieces rather than only reporting a boolean.
REQUIRED_MEMBERS = (
    "name",
    "capabilities",
    "target_paths",
    "emit_frontmatter",
    "map_tools",
    "render_neutral",
    "render_args",
    "render_invocation",
    "degrade_board",
    "build",
)


def conformance_report(adapter: object, target: str) -> list[str]:
    """Return the reasons `adapter` cannot serve `target`, empty when it can.

    Run this against any new adapter before adding it to canonical/manifest.json.
    It checks the members exist and that the adapter agrees about which target it
    is — a copy-pasted adapter that forgot to change `name` writes its output over
    another target's paths, which is silent and expensive to discover later.
    """
    reasons = [f"missing member: {member}" for member in REQUIRED_MEMBERS if not hasattr(adapter, member)]
    if reasons:
        return reasons
    if getattr(adapter, "name") != target:
        reasons.append(f"adapter name {getattr(adapter, 'name')!r} does not match target {target!r}")
    paths = adapter.target_paths()  # type: ignore[attr-defined]
    if not isinstance(paths, TargetPaths):
        reasons.append("target_paths() must return a TargetPaths")
    elif not all((paths.agents, paths.commands, paths.project_instructions)):
        reasons.append("target_paths() must populate agents, commands, and project_instructions")
    if not isinstance(adapter, Adapter):
        reasons.append("does not satisfy the Adapter protocol")
    return reasons
