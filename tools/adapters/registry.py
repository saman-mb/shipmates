"""Pluggable adapter registry used by the exporter CLI."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

from .claude_code import ClaudeCodeAdapter


AdapterFactory = Callable[[Any], Any]
ADAPTERS: dict[str, AdapterFactory] = {"claude-code": ClaudeCodeAdapter}


def register(name: str, factory: AdapterFactory) -> None:
    """Register target adapter without changing exporter dispatch code."""
    if not name or name in ADAPTERS:
        raise ValueError(f"adapter name unavailable: {name!r}")
    ADAPTERS[name] = factory


def create(name: str, capabilities: Any) -> Any:
    try:
        return ADAPTERS[name](capabilities)
    except KeyError as exc:
        known = ", ".join(sorted(ADAPTERS))
        raise ValueError(f"no adapter registered for target {name!r}; known: {known}") from exc
