#!/usr/bin/env python3
"""Adapter interface for compiling neutral Shipmates sources to a harness."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Protocol

try:  # Works both as `tools.adapter_contract` and from tools/ scripts.
    from .capability_registry import HarnessCapabilities
except ImportError:  # pragma: no cover - exercised by direct CLI imports.
    from capability_registry import HarnessCapabilities


@dataclass(frozen=True)
class TargetPaths:
    """Harness-owned locations and project instruction filename."""

    agents: str
    orders: str
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
class CanonicalOrder:
    name: str
    source: Path
    description: str
    argument_hint: str
    allowed_tools: str
    disable_model_invocation: bool
    arguments: tuple[str, ...]
    loop_max: int
    stages: tuple[dict[str, str | int], ...]
    narrative: str
    invocation: str
    board: str


class Adapter(Protocol):
    """Contract every target exporter implements.

    Adapters own target paths, frontmatter emission, capability/tool translation,
    argument and invocation dialects, and explicit board degradation. `build` is
    pure: it returns paths and text; CLI code performs writes and checks.
    """

    name: str
    capabilities: HarnessCapabilities

    def target_paths(self) -> TargetPaths: ...

    def emit_frontmatter(self, kind: str, values: dict[str, str]) -> str: ...

    def map_tools(self, capabilities: Iterable[str]) -> object: ...

    def render_args(self, text: str, arguments: dict[str, str]) -> str: ...

    def render_invocation(self, role: str, argument: str) -> str: ...

    def degrade_board(self, stage: str) -> str: ...

    def build(
        self, root: Path, roles: Iterable[CanonicalRole], orders: Iterable[CanonicalOrder]
    ) -> dict[str, str]: ...
