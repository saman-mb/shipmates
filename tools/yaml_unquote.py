"""Undo yaml_scalar double-quoting on one parsed frontmatter value.

Stdlib-only and Python 3.9-safe: the exporter-floor job can import this
without going through gen_command_pages (which needs 3.10 for dataclass slots).

Do not couple this helper to the Rust crate. The escape table below is the
Python inverse of yaml_scalar.
"""
from __future__ import annotations

import re

# Inverse of yaml_scalar's named escapes (src/adapters/render.rs):
#   \\ -> backslash
#   "  -> double quote
#   n  -> newline
#   r  -> carriage return
#   t  -> tab
# Every other control character is written as \uXXXX (lowercase hex); the
# walker accepts either hex case when reading.
YAML_UNQUOTE_ESCAPES = {"\\": "\\", '"': '"', "n": "\n", "r": "\r", "t": "\t"}
YAML_UNICODE_ESCAPE_RE = re.compile(r"[0-9a-fA-F]{4}")


def yaml_unquote(value: str) -> str:
    """Undo yaml_scalar quoting on one frontmatter value.

    The renderer double-quotes free-text scalars and escapes `\\`, `\"`,
    `\\n`, `\\r`, `\\t` and other control characters as `\\uXXXX`. Page copy
    wants the authored string back, not its YAML spelling. Values not wrapped
    in double quotes — the bare `name:` the renderer deliberately leaves alone,
    and every authored source read directly — pass through byte-identical.

    Scanned left to right: sequential replaces would unescape `\\\\n` twice.
    """
    if len(value) < 2 or value[0] != '"' or value[-1] != '"':
        return value
    inner = value[1:-1]
    out = []
    i = 0
    while i < len(inner):
        char = inner[i]
        escape = inner[i + 1] if char == "\\" and i + 1 < len(inner) else ""
        if escape in YAML_UNQUOTE_ESCAPES:
            out.append(YAML_UNQUOTE_ESCAPES[escape])
            i += 2
        elif escape == "u" and YAML_UNICODE_ESCAPE_RE.match(inner, i + 2):
            out.append(chr(int(inner[i + 2 : i + 6], 16)))
            i += 6
        else:
            out.append(char)
            i += 1
    return "".join(out)


# Alias so callers and tests can keep `_yaml_unquote`.
_yaml_unquote = yaml_unquote
