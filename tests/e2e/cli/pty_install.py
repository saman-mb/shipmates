#!/usr/bin/env python3
"""Drive the real `shipmates install` picker over a PTY and check what it wrote.

`shipmates install` without `--harness` prompts for a numbered harness when its
stdin is a terminal, and silently installs claude-code when it is not. Only a
controlling terminal exercises the prompt, so this helper forks the real binary
onto a pseudo-terminal, waits for the menu to appear, answers with the number
`--harness` is displayed under, and then asserts the installed tree: the
harness's command file and its receipt under `.shipmates/receipts/`.

Usage: pty_install.py --target DIR [--harness NAME] [--timeout SECONDS]

`SHIPMATES_BIN` names the binary under test, and the throwaway HOME/XDG
directories come from the calling test harness. Stdlib only, no sleeps used as
synchronisation: every read waits on PTY output with a deadline, and the child
is killed and reaped if it outlives it.
"""

from __future__ import annotations

import json
import os
import pty
import re
import select
import signal
import sys
import time

DEFAULT_HARNESS = "opencode"
DEFAULT_TIMEOUT = 120.0
# The picker's own prompt, printed and flushed before it reads the answer.
PROMPT = b"Select harnesses [e.g."
COMMAND_NAME = "shipmates-ship-issue"

# Where each harness tree keeps an installed command. Mirrors the trees the
# adapters write; the harness this test picks is the one that has to be exact.
COMMAND_PATHS = {
    "claude-code": ".claude/skills/{name}/SKILL.md",
    "opencode": ".opencode/commands/{name}.md",
    "antigravity": ".agents/skills/{name}/SKILL.md",
    "codex": ".agents/skills/{name}/SKILL.md",
    "cursor": ".cursor/skills/{name}/SKILL.md",
    "github-copilot": ".agents/skills/{name}/SKILL.md",
    "pi": ".pi/skills/{name}/SKILL.md",
    "grok-build": ".grok/skills/{name}/SKILL.md",
    "devin": ".devin/skills/{name}/SKILL.md",
}

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
MENU_LINE = r"^\s*(\d+)\)\s+{name}\s"


class Failure(Exception):
    """A check that should exit nonzero with the PTY transcript attached."""


def parse_args(argv):
    options = {"target": None, "harness": DEFAULT_HARNESS, "timeout": DEFAULT_TIMEOUT}
    rest = list(argv)
    while rest:
        flag = rest.pop(0)
        if flag not in ("--target", "--harness", "--timeout") or not rest:
            raise Failure(f"usage: {sys.argv[0]} --target DIR [--harness NAME] [--timeout SECONDS]")
        value = rest.pop(0)
        if flag == "--target":
            options["target"] = value
        elif flag == "--harness":
            options["harness"] = value
        else:
            options["timeout"] = float(value)
    if options["target"] is None:
        raise Failure("--target is required: the throwaway --dir the install writes into")
    return options


def decode(transcript):
    return ANSI.sub("", bytes(transcript).decode("utf-8", "replace"))


def read_available(fd, deadline, transcript, marker=None):
    """Read PTY output into `transcript`; True once `marker` (or EOF) is reached.

    A False return means the deadline expired first — the caller owns the
    diagnostic, and the child still has to be killed.
    """
    while True:
        if marker is not None and marker in transcript:
            return True
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        ready, _, _ = select.select([fd], [], [], min(0.2, remaining))
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            return True  # EIO: the slave is gone, so the child has exited
        if not chunk:
            return True
        transcript += chunk


def menu_number(text, harness):
    match = re.search(MENU_LINE.format(name=re.escape(harness)), text, re.MULTILINE)
    if match is None:
        raise Failure(f"{harness} is not on the displayed picker menu:\n{text}")
    return match.group(1)


def kill_child(pid):
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        os.waitpid(pid, 0)
    except ChildProcessError:
        pass


def drive(pid, fd, options, deadline, transcript):
    """Answer the picker, then wait for the install to exit. Returns its status."""
    timeout = options["timeout"]
    read_available(fd, deadline, transcript, marker=PROMPT)
    if PROMPT not in transcript:
        raise Failure(f"the picker prompt never appeared within {timeout:.0f}s:\n{decode(transcript)}")

    choice = menu_number(decode(transcript), options["harness"])
    os.write(fd, f"{choice}\n".encode())
    if not read_available(fd, deadline, transcript):
        raise Failure(
            f"the install did not exit within {timeout:.0f}s after choosing "
            f"{options['harness']}:\n{decode(transcript)}"
        )
    _, status = os.waitpid(pid, 0)
    return status


def check_installed_tree(target, harness, text):
    template = COMMAND_PATHS.get(harness)
    if template is None:
        raise Failure(f"unknown harness {harness!r}; add its tree to COMMAND_PATHS")
    command_path = template.format(name=COMMAND_NAME)
    if not os.path.isfile(os.path.join(target, command_path)):
        raise Failure(f"{harness} was chosen but {command_path} was not installed:\n{text}")

    receipt = os.path.join(target, ".shipmates", "receipts", f"{harness}.json")
    if not os.path.isfile(receipt):
        raise Failure(
            f"no receipt for the chosen harness at .shipmates/receipts/{harness}.json:\n{text}"
        )
    with open(receipt, encoding="utf-8") as handle:
        recorded = json.load(handle).get("harness")
    if recorded != harness:
        raise Failure(f"the receipt records harness {recorded!r}, not {harness!r}")

    # The non-TTY path defaults to claude-code, so its receipt appearing beside
    # a different choice means the prompt was bypassed.
    fallback = os.path.join(target, ".shipmates", "receipts", "claude-code.json")
    if harness != "claude-code" and os.path.exists(fallback):
        raise Failure("the non-TTY claude-code fallback ran as well — the PTY path was not the only one")

    return command_path


def main(argv):
    options = parse_args(argv)
    binary = os.environ.get("SHIPMATES_BIN")
    if not binary:
        raise Failure("SHIPMATES_BIN must point at the binary under test")
    if not (os.path.isfile(binary) and os.access(binary, os.X_OK)):
        raise Failure(f"SHIPMATES_BIN is not an executable file: {binary}")

    target = os.path.abspath(options["target"])
    os.makedirs(target, exist_ok=True)
    argv = [binary, "install", "--dir", target, "--with-tools", "none"]

    deadline = time.monotonic() + options["timeout"]
    transcript = bytearray()
    pid, fd = pty.fork()
    if pid == 0:
        os.execv(binary, argv)
        os._exit(127)

    status = None
    try:
        status = drive(pid, fd, options, deadline, transcript)
    finally:
        if status is None:
            kill_child(pid)
        os.close(fd)

    text = decode(transcript)
    code = os.WEXITSTATUS(status) if os.WIFEXITED(status) else -1
    if code != 0:
        raise Failure(f"install exited {code} after the picker choice:\n{text}")

    command_path = check_installed_tree(target, options["harness"], text)
    print(f"picker installed {options['harness']}: {command_path} + its receipt")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Failure as failure:
        sys.stderr.write(f"pty_install: {failure}\n")
        sys.exit(1)
