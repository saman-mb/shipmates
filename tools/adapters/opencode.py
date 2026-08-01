#!/usr/bin/env python3
"""opencode adapter for canonical crew and order sources."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Iterable

try:
    from ..adapter_contract import CanonicalCommand, CanonicalRole, TargetPaths
    from ..capability_registry import HarnessCapabilities
except ImportError:  # pragma: no cover - direct script development convenience.
    from adapter_contract import CanonicalCommand, CanonicalRole, TargetPaths
    from capability_registry import HarnessCapabilities


#: opencode expands ``!`cmd` `` inside a command body when the command is
#: *loaded* — its prompt reader matches ``/!`([^`]+)`/g``, unanchored and
#: global, before the user has confirmed anything. Canonical has no occurrences
#: today; this pattern is what keeps a future canonical edit from quietly
#: shipping shell that runs the moment someone types `/ship-issue`.
SHELL_EXPANSION_RE = re.compile(r"!`")

#: Mirrors tools/export.py's READ_SCOPES — the read capability's sub-scopes.
READ_SCOPES = ("read", "search", "glob")


def _unique(values: Iterable[str]) -> tuple[str, ...]:
    """Drop repeats, keeping first-seen order."""
    return tuple(dict.fromkeys(values))


def _reject_shell_expansion(label: str, text: str) -> None:
    """Fail on ``!` `` anywhere in `text` — opencode would execute it on load."""
    for lineno, line in enumerate(text.splitlines(), 1):
        if SHELL_EXPANSION_RE.search(line):
            raise ValueError(
                f"{label}:{lineno}: '!`' — opencode runs this shell when the command "
                "is loaded, before the user confirms anything; describe the command "
                "in prose or make it a numbered step the agent chooses to run"
            )


class OpencodeAdapter:
    name = "opencode"

    #: The only keys opencode documents for each kind. Unrecognised agent
    #: frontmatter is *passed through to the provider as a request parameter*
    #: rather than ignored, so an extra key is a correctness hazard, not
    #: clutter. `name` is deliberately absent: the filename is the agent name.
    AGENT_KEYS = ("description", "mode", "permission")
    COMMAND_KEYS = ("description",)

    def __init__(self, capabilities: HarnessCapabilities):
        self.capabilities = capabilities

    def target_paths(self) -> TargetPaths:
        return TargetPaths(
            agents=self.capabilities.agent_path,
            commands=self.capabilities.skill_path,
            project_instructions=self.capabilities.project_instructions,
        )

    def emit_frontmatter(self, kind: str, values: dict[str, str]) -> str:
        """Emit opencode's frontmatter in stable key order, rejecting extras."""
        allowed = {"agent": self.AGENT_KEYS, "command": self.COMMAND_KEYS}.get(kind)
        if allowed is None:
            raise ValueError(f"unknown frontmatter kind: {kind!r}")
        unknown = sorted(set(values) - set(allowed))
        if unknown:
            raise ValueError(
                f"{kind}: undocumented opencode frontmatter key(s): {', '.join(unknown)}"
            )
        lines = ["---"]
        for key in allowed:
            if key not in values:
                continue
            if key == "permission":
                # The one block-valued key: a nested mapping, already indented.
                lines.append("permission:")
                lines.extend(values[key].splitlines())
            else:
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
        """Translate neutral capabilities into opencode permission keys.

        Deduped, first-seen order wins — a deliberate divergence from Claude's
        list semantics. opencode's `permission` is a mapping, and both the
        `write` and `edit` scopes resolve to its single `edit` tool, so a role
        declaring `tool-order: read,write,edit,bash,...` would otherwise emit
        the same key twice.
        """
        ordered = tuple(tool_order)
        if ordered:
            unknown = sorted(set(ordered) - set(self.capabilities.scopes))
            if unknown:
                raise ValueError(f"unknown tool scope(s): {', '.join(unknown)}")
            return _unique(self.capabilities.scopes[scope] for scope in ordered)
        mapped: list[str] = []
        for capability in capabilities:
            value = self.capabilities.map((capability,))[capability]
            if capability == "read" and read_scopes:
                unknown = sorted(set(read_scopes) - set(READ_SCOPES))
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
        return _unique(mapped)

    def render_permission(self, tools: Iterable[str]) -> str:
        """Render the agent `permission` mapping: catch-all deny, then allows.

        Rule order is load-bearing. opencode resolves a permission by taking the
        *last* matching rule, so the `"*"` catch-all has to come first for the
        specific allows after it to mean anything. It is mandatory rather than
        defensive: opencode's own agent defaults start from `{"*": "allow"}`, so
        an agent file that omits the deny grants the role everything.
        """
        lines = ['  "*": deny']
        lines.extend(f"  {tool}: allow" for tool in tools)
        return "\n".join(lines)

    def render_neutral(self, text: str) -> str:
        """Render harness-neutral prose into opencode's dialect.

        `AGENTS.md` survives untouched — it is opencode's own project
        instructions filename. The two ordered rules above the general ones
        exist because the onboarding narrative uses `TARGET.md` and `AGENTS.md`
        as two *different* files; rendering both to `AGENTS.md` would produce
        "`AGENTS.md` if one exists, else `AGENTS.md`". Rewriting the fallback
        arm to `CLAUDE.md` reproduces opencode's real rules chain instead.
        """
        replacements = (
            ("`TARGET.md`/`AGENTS.md`", "`AGENTS.md`/`CLAUDE.md`"),
            ("else `AGENTS.md`", "else `CLAUDE.md`"),
            ("TARGET.md", "AGENTS.md"),
            ("agent-files/*.md", ".opencode/agents/*.md"),
            ("Harness-Session", "Opencode-Session"),
            # opencode's built-in `plan` agent is `mode: primary` and cannot
            # legitimately be spawned as a subagent. `architect` is ours, is
            # `mode: subagent`, and carries no edit permission, so it preserves
            # the read-only planning posture the stage depends on. Both the
            # stage heading and the spawn line are rewritten — leaving the
            # heading naming `planner` two lines above a `subagent_type:
            # architect` would name an agent opencode does not have.
            ("agent: `planner`", "agent: `architect`"),
            ("@role(planner)", "subagent_type: architect"),
            ("@role(senior-engineer)", "subagent_type: senior-engineer"),
            ("@role(sdet)", "subagent_type: sdet"),
            ("| `@role`           |", "| `subagent_type`   |"),
            ("| `@role` | Runs |", "| `subagent_type` | Runs |"),
            ("`@role` reference", "`subagent_type`"),
            # opencode's built-in catch-all subagent is named `general`.
            ("general-purpose", "general"),
        )
        for source, target in replacements:
            text = text.replace(source, target)
        return text.replace("to an `.opencode/agents/*.md`", "to a `.opencode/agents/*.md`")

    def render_args(self, text: str, arguments: dict[str, str]) -> str:
        for name, value in arguments.items():
            text = text.replace("{{" + name + "}}", value)
        return text

    def render_invocation(self, role: str, argument: str) -> str:
        # opencode's task tool takes `description`, `prompt` and `subagent_type`,
        # so the Claude shape transfers verbatim. An `@name` mention would not:
        # in the subtask branch only the first text part is forwarded as the
        # prompt, and the resolved agent part is silently dropped.
        return f"subagent_type: {role}\nargument: {argument}"

    def degrade_board(self, stage: str) -> str:
        return f"opencode board stage `{stage}` is native (`mode: subagent`); no degradation."

    def build(
        self, root: Path, roles: Iterable[CanonicalRole], commands: Iterable[CanonicalCommand]
    ) -> dict[str, str]:
        files: dict[str, str] = {}
        for role in sorted(roles, key=lambda item: item.name):
            permission = self.render_permission(
                self.map_tools(
                    role.capabilities,
                    role.web_scopes,
                    role.read_scopes,
                    role.tool_order,
                )
            )
            frontmatter = self.emit_frontmatter(
                "agent",
                {
                    "description": role.description,
                    "mode": "subagent",
                    "permission": permission,
                },
            )
            destination = self._destination(self.target_paths().agents, role.name)
            rendered = self.render_neutral(role.body)
            # Agent bodies are prompt files opencode loads too, so they get the
            # same guard as commands. The exporter runs against whatever
            # canonical/ a user's clone or fork contains, which is what makes
            # this a build-time refusal rather than a CI-time assertion.
            _reject_shell_expansion(destination, rendered)
            files[destination] = frontmatter + "\n" + rendered + "\n"
        for command in sorted(commands, key=lambda item: item.name):
            body = self.render_args(
                command.narrative,
                {argument: "$ARGUMENTS" for argument in command.arguments},
            )
            # Same posture as the Claude adapter: opencode drives these stages
            # from the narrative, so the rendered contract is asserted rather
            # than appended — emitting it would be pretend runtime enforcement.
            invocations = tuple(
                self.render_invocation(str(stage_role), "$ARGUMENTS")
                for stage in command.stages
                for stage_role in stage["roles"]  # type: ignore[index]
            )
            if not invocations or not self.degrade_board(command.board):
                raise ValueError(f"{command.name}: opencode execution contract rendered empty")
            frontmatter = self.emit_frontmatter("command", {"description": command.description})
            rendered = self.render_neutral(body)
            destination = self._destination(self.target_paths().commands, command.name)
            _reject_shell_expansion(destination, rendered)
            files[destination] = frontmatter + "\n" + rendered + "\n"
        return files

    def _destination(self, template: str, name: str) -> str:
        relative = template.format(name=name).lstrip("/")
        if relative.startswith(".opencode/"):
            relative = relative[len(".opencode/") :]
        return f"harnesses/{self.name}/{relative}"


def adapter(capabilities: HarnessCapabilities) -> OpencodeAdapter:
    return OpencodeAdapter(capabilities)
