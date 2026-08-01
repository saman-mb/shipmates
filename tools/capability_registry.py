#!/usr/bin/env python3
"""Semantic capability registry shared by every exporter adapter."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
import re
from typing import Any


CAPABILITIES = ("read", "edit", "bash", "web", "agent")
NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


class CapabilityError(ValueError):
    """Capability registry is malformed or does not cover a request."""


@dataclass(frozen=True)
class HarnessCapabilities:
    name: str
    agent_path: str
    skill_path: str
    project_instructions: str
    permission_model: str
    tools: dict[str, Any]
    scopes: dict[str, str] = field(default_factory=dict)

    def map(self, capabilities: tuple[str, ...] | list[str]) -> dict[str, Any]:
        unknown = sorted(set(capabilities) - set(self.tools))
        if unknown:
            raise CapabilityError(
                f"{self.name}: registry has no mapping for capability(s): {', '.join(unknown)}"
            )
        return {capability: self.tools[capability] for capability in capabilities}


def load_registry(path: Path) -> dict[str, HarnessCapabilities]:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise CapabilityError(f"capability registry not found: {path}") from exc
    except json.JSONDecodeError as exc:
        raise CapabilityError(f"invalid capability registry: {exc}") from exc

    if not isinstance(raw, dict) or raw.get("schema_version") != 1:
        raise CapabilityError("capability registry schema_version must be 1")
    declared = tuple(raw.get("capabilities", ()))
    if declared != CAPABILITIES:
        raise CapabilityError(
            f"capability registry must declare {CAPABILITIES}, got {declared}"
        )

    harnesses = raw.get("harnesses")
    if not isinstance(harnesses, dict) or not harnesses:
        raise CapabilityError("capability registry harnesses must be a non-empty object")
    result: dict[str, HarnessCapabilities] = {}
    for name, value in harnesses.items():
        if not isinstance(name, str) or not NAME_RE.fullmatch(name):
            raise CapabilityError(f"harnesses.{name!r}: invalid target name")
        required = ("agent_path", "skill_path", "project_instructions", "permission_model", "tools")
        if not isinstance(value, dict) or any(key not in value for key in required):
            raise CapabilityError(f"harnesses.{name}: incomplete capability entry")
        if not all(isinstance(value[key], str) and value[key] for key in required[:-1]):
            raise CapabilityError(f"harnesses.{name}: paths and permission_model must be strings")
        for field in ("agent_path", "skill_path"):
            path = value[field]
            if "{name}" not in path or path.startswith("/") or ".." in path.split("/"):
                raise CapabilityError(f"harnesses.{name}: {field} must be a safe {{name}} path")
        instructions = value["project_instructions"]
        if instructions.startswith("/") or ".." in instructions.split("/"):
            raise CapabilityError(f"harnesses.{name}: project_instructions must be a safe path")
        if value["permission_model"] not in ("allowlist", "permission-map"):
            raise CapabilityError(f"harnesses.{name}: unknown permission_model")
        if not isinstance(value["tools"], dict) or set(value["tools"]) != set(CAPABILITIES):
            raise CapabilityError(
                f"harnesses.{name}: tools must map exactly {', '.join(CAPABILITIES)}"
            )
        if any(
            not isinstance(mapped, (str, list, dict)) for mapped in value["tools"].values()
        ):
            raise CapabilityError(
                f"harnesses.{name}: tool mappings must be strings, lists, or objects"
            )
        if any(
            isinstance(mapped, list) and not all(isinstance(item, str) for item in mapped)
            for mapped in value["tools"].values()
        ):
            raise CapabilityError(f"harnesses.{name}: tool lists must contain strings")
        missing = set(CAPABILITIES) - set(value["tools"])
        if missing:
            raise CapabilityError(
                f"harnesses.{name}: missing mappings for {', '.join(sorted(missing))}"
            )
        scopes = value.get("scopes", {})
        if not isinstance(scopes, dict) or any(
            not isinstance(scope, str) or not isinstance(mapped, str)
            for scope, mapped in scopes.items()
        ):
            raise CapabilityError(f"harnesses.{name}: scopes must map strings to strings")
        result[name] = HarnessCapabilities(
            name=name,
            agent_path=value["agent_path"],
            skill_path=value["skill_path"],
            project_instructions=value["project_instructions"],
            permission_model=value["permission_model"],
            tools=value["tools"],
            scopes=scopes,
        )
    return result
