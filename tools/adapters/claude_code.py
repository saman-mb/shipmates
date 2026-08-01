#!/usr/bin/env python3
"""Claude Code adapter for canonical crew and order sources."""

from __future__ import annotations

from pathlib import Path
from typing import Iterable

try:
    from ..adapter_contract import CanonicalCommand, CanonicalRole, TargetPaths
    from ..capability_registry import HarnessCapabilities
except ImportError:  # pragma: no cover - direct script development convenience.
    from adapter_contract import CanonicalCommand, CanonicalRole, TargetPaths
    from capability_registry import HarnessCapabilities


class ClaudeCodeAdapter:
    name = "claude-code"

    def __init__(self, capabilities: HarnessCapabilities):
        self.capabilities = capabilities

    def target_paths(self) -> TargetPaths:
        return TargetPaths(
            agents=self.capabilities.agent_path,
            commands=self.capabilities.skill_path,
            project_instructions=self.capabilities.project_instructions,
        )

    def emit_frontmatter(self, kind: str, values: dict[str, str]) -> str:
        """Emit Claude's simple scalar frontmatter in stable key order."""
        allowed = (
            "name",
            "description",
            "tools",
            "argument-hint",
            "allowed-tools",
            "disable-model-invocation",
        )
        lines = ["---"]
        for key in allowed:
            if key in values:
                lines.append(f"{key}: {values[key]}")
        lines.extend(("---", ""))
        return "\n".join(lines)

    def map_tools(
        self,
        capabilities: Iterable[str],
        web_scopes: Iterable[str] = (),
        read_scopes: Iterable[str] = (),
        tool_order: Iterable[str] = (),
    ) -> tuple[str, ...]:
        ordered = tuple(tool_order)
        if ordered:
            unknown = sorted(set(ordered) - set(self.capabilities.scopes))
            if unknown:
                raise ValueError(f"unknown tool scope(s): {', '.join(unknown)}")
            return tuple(self.capabilities.scopes[scope] for scope in ordered)
        mapped: list[str] = []
        for capability in capabilities:
            value = self.capabilities.map((capability,))[capability]
            if capability == "read" and read_scopes:
                unknown = sorted(set(read_scopes) - {"read", "search", "glob"})
                if unknown:
                    raise ValueError(f"unknown read scope(s): {', '.join(unknown)}")
                mapped.extend(self.capabilities.scopes[scope] for scope in read_scopes)
            elif capability == "web" and isinstance(value, dict):
                scopes = tuple(web_scopes) or ("search", "fetch")
                unknown = sorted(set(scopes) - set(value))
                if unknown:
                    raise ValueError(f"unknown web scope(s): {', '.join(unknown)}")
                mapped.extend(str(value[scope]) for scope in scopes)
            else:
                values = value if isinstance(value, list) else [value]
                mapped.extend(str(item) for item in values)
        return tuple(mapped)

    def render_neutral(self, text: str) -> str:
        """Render harness-neutral prose into Claude's established source dialect."""
        text = text.replace("`TARGET.md`/`AGENTS.md`", "`TARGET.md`/`__AGENTS__.md`")
        text = text.replace("else `AGENTS.md`", "else `__AGENTS__.md`")
        replacements = (
            ("TARGET.md", "CLAUDE.md"),
            ("AGENTS.md", "CLAUDE.md"),
            ("agent-files/*.md", ".claude/agents/*.md"),
            ("Harness-Session", "Claude-Session"),
            ("agent: `planner`", "agent: `Plan`"),
            ("@role(planner)", "subagent_type: Plan"),
            ("@role(senior-engineer)", "subagent_type: senior-engineer"),
            ("@role(sdet)", "subagent_type: sdet"),
            ("| `@role`           |", "| `subagent_type`   |"),
            ("| `@role` | Runs |", "| `subagent_type` | Runs |"),
            ("`@role` reference", "`subagent_type`"),
        )
        for source, target in replacements:
            text = text.replace(source, target)
        text = text.replace("to an `.claude/agents/*.md`", "to a `.claude/agents/*.md`")
        text = text.replace("__AGENTS__.md", "AGENTS.md")
        return text

    def render_args(self, text: str, arguments: dict[str, str]) -> str:
        for name, value in arguments.items():
            text = text.replace("{{" + name + "}}", value)
        # Compatibility source uses the Claude token. Named values are preferred
        # in canonical sources, but this keeps the bridge useful during migration.
        if "arguments" in arguments:
            text = text.replace("$ARGUMENTS", arguments["arguments"])
        return text

    def render_invocation(self, role: str, argument: str) -> str:
        return f"subagent_type: {role}\nargument: {argument}"

    def degrade_board(self, stage: str) -> str:
        return f"Claude Code board stage `{stage}` is native; no degradation."

    def build(
        self, root: Path, roles: Iterable[CanonicalRole], commands: Iterable[CanonicalCommand]
    ) -> dict[str, str]:
        files: dict[str, str] = {}
        for role in sorted(roles, key=lambda item: item.name):
            tools = ", ".join(
                self.map_tools(
                    role.capabilities,
                    role.web_scopes,
                    role.read_scopes,
                    role.tool_order,
                )
            )
            frontmatter = self.emit_frontmatter(
                "agent",
                {"name": role.name, "description": role.description, "tools": tools},
            )
            destination = self._destination(self.target_paths().agents, role.name)
            files[destination] = frontmatter + "\n" + self.render_neutral(role.body) + "\n"
        for command in sorted(commands, key=lambda item: item.name):
            # Render neutral argument tokens into Claude's runtime token. These
            # calls are deliberately made here, rather than copying a source
            # body, so canonical narrative edits remain authoritative.
            body = self.render_args(
                command.narrative,
                {argument: "$ARGUMENTS" for argument in command.arguments},
            )
            # Claude Code drives these stages from the narrative itself, so the
            # rendered contract is asserted rather than appended: emitting it
            # would be pretend runtime enforcement, and would break byte identity
            # with the compatibility sources. The stage table stays honest
            # because load_catalog requires every declared role to appear in the
            # narrative it ships beside.
            invocations = tuple(
                self.render_invocation(str(stage_role), "$ARGUMENTS")
                for stage in command.stages
                for stage_role in stage["roles"]  # type: ignore[index]
            )
            if not invocations or not self.degrade_board(command.board):
                raise ValueError(f"{command.name}: Claude execution contract rendered empty")
            frontmatter = self.emit_frontmatter(
                "command",
                {
                    "name": command.name,
                    "description": command.description,
                    "argument-hint": command.argument_hint,
                    "allowed-tools": command.allowed_tools,
                    "disable-model-invocation": str(command.disable_model_invocation).lower(),
                },
            )
            destination = self._destination(self.target_paths().commands, command.name)
            files[destination] = frontmatter + "\n" + self.render_neutral(body) + "\n"
        return files

    def _destination(self, template: str, name: str) -> str:
        relative = template.format(name=name).lstrip("/")
        if relative.startswith(".claude/"):
            relative = relative[len(".claude/") :]
        return f"harnesses/{self.name}/{relative}"


def adapter(capabilities: HarnessCapabilities) -> ClaudeCodeAdapter:
    return ClaudeCodeAdapter(capabilities)
