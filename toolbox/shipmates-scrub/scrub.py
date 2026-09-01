#!/usr/bin/env python3
"""scrub — redact secrets and PII from a log or paste before it's shared.

Self-contained: the only dependency is the Python standard library (`re`). No
network, no config, deterministic — the same input always produces the same
cleaned output, byte for byte.

This is the runnable payload of the shipmates `scrub` tool. An agent reaches for
it, per tool.md, when text is about to leave the repo — pasted into an issue, a
bug report, a chat message, or a PR description — and might carry a credential or
personal data. It is never a slash command.

What it catches, each replaced with a typed placeholder:
    PEM private-key blocks ....... [REDACTED_PRIVATE_KEY]
    JWTs (three base64url parts) . [REDACTED_JWT]
    AWS access key IDs (AKIA…) ... [REDACTED_AWS_KEY]
    Bearer tokens ................ Bearer [REDACTED_TOKEN]
    api_key= / token= / secret= .. <key>=[REDACTED_TOKEN]
    email addresses .............. [REDACTED_EMAIL]
    IPv4 addresses ............... [REDACTED_IP]

The cleaned text goes to --out (or stdout); a per-category redaction summary goes
to stderr, so it never pollutes the cleaned output. These are regex heuristics,
not a guarantee: they are tuned to be conservative — they leave ordinary prose
and code identifiers alone, which also means an exotic secret can slip through.
Review the result before sharing.

Usage:
    python3 scrub.py --in log.txt --out clean.txt
    cat log.txt | python3 scrub.py > clean.txt      # stdin -> stdout

Exit codes: 0 ok; 2 on a usage error (bad option, unreadable input, unwritable
output).
"""
import argparse
import re
import sys

# ---------------------------------------------------------------------------
# Detectors, applied in this order. Order matters: the most specific / most
# structural patterns run first, so a value is typed by the narrowest detector
# that fits (an AWS key never lands as a generic assignment, a bearer JWT is
# typed as a JWT) and a multi-line key block is removed whole before any
# line-oriented pattern can nibble at its insides.
# ---------------------------------------------------------------------------

# PEM private-key block, header to footer, across newlines. Non-greedy so two
# adjacent blocks don't merge into one.
PEM_RE = re.compile(
    r"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----"
    r".*?"
    r"-----END (?:[A-Z0-9 ]+ )?PRIVATE KEY-----",
    re.DOTALL,
)

# JWT: base64url header starting `eyJ` (the encoding of `{"`), then two more
# base64url segments. The `eyJ` anchor keeps this off ordinary dotted names.
JWT_RE = re.compile(
    r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\b"
)

# AWS access key ID: AKIA (long-term) or ASIA (temporary) + 16 uppercase chars.
AWS_RE = re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")

# `Authorization: Bearer <token>` — keep the word "Bearer", redact the token.
# A raw-JWT bearer has already been typed as a JWT by the pass above, so what
# reaches here is an opaque non-JWT token; require length >= 10 so the literal
# word "token"/"of"/etc. in prose is never mistaken for one.
BEARER_RE = re.compile(r"(?i)\b(bearer\s+)([A-Za-z0-9][A-Za-z0-9._~+/=-]{9,})")

# Generic secret assignment: a sensitively-named key, then `=` or `:`, then a
# value. Conservative on the value so code and prose survive:
#   - a *quoted* value (4+ chars) is taken as a literal secret, or
#   - an *unquoted* value that actually looks like a credential — 6+ chars from
#     a restricted charset (no dots, so `token = a.b.c` is left alone) AND
#     containing a digit or a symbol (so `token = response` is left alone).
# The key and separator are preserved; only the value is replaced.
ASSIGN_RE = re.compile(
    r"""(?P<pre>
            \b(?:
                api[_-]?key | secret | token | password | passwd | pwd |
                access[_-]?token | refresh[_-]?token | auth[_-]?token |
                client[_-]?secret | private[_-]?token
            )\b
            [ \t]*[=:][ \t]*
            (?P<q>["']?)
        )
        (?P<val>
            (?:(?<=["'])[^"'\n]{4,})
            |
            (?:(?<!["'])(?=[A-Za-z0-9][A-Za-z0-9_+/=~-]*[0-9+/=_~-])
               [A-Za-z0-9][A-Za-z0-9_+/=~-]{5,})
        )
        (?P<post>(?P=q))
    """,
    re.VERBOSE | re.IGNORECASE,
)

# Email address.
EMAIL_RE = re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")

# IPv4 dotted quad, each octet validated 0-255 so version strings and the like
# (which rarely have four in-range octets) are mostly left alone.
_OCTET = r"(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)"
IPV4_RE = re.compile(rf"\b(?:{_OCTET}\.){{3}}{_OCTET}\b")


# Each detector: internal key, human label for the summary, and a replacement
# (a literal placeholder, or a function that keeps a captured prefix).
def _bearer_sub(m):
    return m.group(1) + "[REDACTED_TOKEN]"


def _assign_sub(m):
    return m.group("pre") + "[REDACTED_TOKEN]" + m.group("post")


DETECTORS = (
    ("private_key", "private-key", PEM_RE, "[REDACTED_PRIVATE_KEY]"),
    ("jwt", "jwt", JWT_RE, "[REDACTED_JWT]"),
    ("aws_key", "aws-key", AWS_RE, "[REDACTED_AWS_KEY]"),
    ("bearer_token", "bearer-token", BEARER_RE, _bearer_sub),
    ("assignment", "secret-assignment", ASSIGN_RE, _assign_sub),
    ("email", "email", EMAIL_RE, "[REDACTED_EMAIL]"),
    ("ipv4", "ipv4-address", IPV4_RE, "[REDACTED_IP]"),
)


def scrub(text):
    """Return (cleaned_text, counts) where counts maps internal key -> hits."""
    counts = {}
    for key, _label, regex, repl in DETECTORS:
        text, n = regex.subn(repl, text)
        if n:
            counts[key] = n
    return text, counts


def format_summary(counts):
    """Render the per-category redaction summary shown on stderr."""
    total = sum(counts.values())
    if not total:
        return "scrub: no secrets or PII detected"
    lines = [f"scrub: redacted {total} item{'s' if total != 1 else ''} "
             f"across {len(counts)} categor{'ies' if len(counts) != 1 else 'y'}"]
    for key, label, _regex, _repl in DETECTORS:
        if key in counts:
            lines.append(f"  {label:<18} {counts[key]}")
    return "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(
        prog="scrub",
        description="Redact secrets and PII from a log or paste before sharing it.",
    )
    ap.add_argument("--in", dest="infile",
                    help="input file to read (default: stdin)")
    ap.add_argument("--out", dest="outfile",
                    help="output file for the cleaned text (default: stdout)")
    args = ap.parse_args(argv)

    try:
        if args.infile:
            with open(args.infile, encoding="utf-8") as fh:
                text = fh.read()
        else:
            text = sys.stdin.read()
    except OSError as e:
        print(f"scrub: could not read input: {e}", file=sys.stderr)
        return 2

    cleaned, counts = scrub(text)

    try:
        if args.outfile:
            with open(args.outfile, "w", encoding="utf-8") as fh:
                fh.write(cleaned)
        else:
            sys.stdout.write(cleaned)
    except OSError as e:
        print(f"scrub: could not write output: {e}", file=sys.stderr)
        return 2

    print(format_summary(counts), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
