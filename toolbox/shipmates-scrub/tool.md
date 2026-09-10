---
name: shipmates-scrub
description: Shipmates: Redact secrets and PII from a log, paste, or bug report before it leaves the repo — emails, AWS access keys, api_key/token/secret assignments, Bearer tokens, JWTs, IPv4 addresses, and PEM private-key blocks, each swapped for a typed placeholder like [REDACTED_AWS_KEY]. Reach for this whenever text you're about to share externally — an issue, a comment, a paste, a PR body, an error dump — might carry a credential or personal data. Never a slash command; use it implicitly when the intent calls for one.
---

# scrub

A tool the crew uses on its own. Any time output or logs are about to leave the
repo — pasted into a GitHub issue, a bug report, a chat message, a PR
description, or a support ticket — run the text through this first so a leaked
credential or someone's email never rides along.

## Run it

The redactor `scrub.py` sits next to this file. It is pure standard library —
no dependencies, no network — so it just runs with `python3`.

```
python3 scrub.py --in log.txt --out clean.txt
# or pipe on stdin -> stdout:
cat log.txt | python3 scrub.py > clean.txt
```

The cleaned text is written to `--out` (or stdout). A per-category redaction
summary is printed to **stderr**, so it never contaminates the cleaned output
even when you're piping:

```
scrub: redacted 9 items across 7 categories
  private-key        1
  jwt                1
  aws-key            1
  bearer-token       1
  secret-assignment  3
  email              1
  ipv4-address       1
```

Exit codes: `0` on success; `2` on a usage error (bad option, unreadable input,
unwritable output).

## What it catches

Each match is replaced with a typed placeholder so the shape of the redaction is
visible in the cleaned text:

| Category | Detects | Placeholder |
| --- | --- | --- |
| PEM private keys | `-----BEGIN … PRIVATE KEY-----` blocks (whole block) | `[REDACTED_PRIVATE_KEY]` |
| JWTs | three base64url segments, `eyJ…` header | `[REDACTED_JWT]` |
| AWS access keys | `AKIA…` / `ASIA…` key IDs | `[REDACTED_AWS_KEY]` |
| Bearer tokens | `Authorization: Bearer <token>` (keeps the word `Bearer`) | `[REDACTED_TOKEN]` |
| Secret assignments | `api_key=` / `token=` / `secret=` / `password=` and kin | `[REDACTED_TOKEN]` |
| Emails | `name@host.tld` | `[REDACTED_EMAIL]` |
| IPv4 addresses | validated dotted quads | `[REDACTED_IP]` |

The assignment detector is deliberately careful with the value: it redacts a
quoted literal or a credential-shaped token, but leaves code like
`token = response.data.token` and prose like "see the api_key rotation policy"
untouched.

## Honesty

These are regex heuristics, not a guarantee. They are tuned to be conservative —
to leave ordinary prose and code identifiers alone — which is the same trade-off
that lets an unusual secret (a bare high-entropy string with no key name, an
exotic token format) slip through. Treat a scrub as a strong first pass, not a
clearance: read the cleaned text before you share it.
