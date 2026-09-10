---
name: shipmates-domaincheck
description: Shipmates: Check domain name availability via RDAP — registry-authoritative verdicts (available, registered, unknown), batch TLD sweeps, optional registrar detail and whois passthrough. Reach for this when naming a product, verifying a handle's domain, checking a client's domain claim, or sweeping a shortlist across TLDs — RDAP 404 is the registry's own unregistered answer, not a DNS guess. Never a slash command; use it implicitly when the intent calls for one.
---

# domaincheck

A tool the crew uses on its own. When a task needs **whether a domain is
registered** — product naming, launch branding alongside `social-card` and
`pixelart`, verifying a domain claim, or sweeping one name across several TLDs —
check via **RDAP**, not DNS lookups or scraped whois pages. RDAP answers come from
the registry of record (`404` = unregistered, `200` = registered).

## Run it

The checker `domaincheck.py` sits next to this file. It uses only the Python
standard library (`urllib`) — no API keys, no accounts.

```
python3 domaincheck.py github.com
python3 domaincheck.py example.com example.org
python3 domaincheck.py --tld com,app,io shipmates
python3 domaincheck.py --detail github.com
python3 domaincheck.py --whois github.com
```

Output is one line per domain: `name<TAB>verdict` where verdict is `available`,
`registered`, or `unknown`. With `--detail`, registered domains also print
registrar, status, and key dates parsed from the RDAP JSON. `--whois` runs the
system `whois` binary when installed; if absent, the tool skips it cleanly.

Batch mode (`--tld` or multiple domains) staggers queries with a default delay
and backs off on HTTP 429 from rdap.org. Tune with `--delay SEC`.

Exit codes: `0` on success; `2` on a usage or validation error.

## Why RDAP, not DNS

DNS "NXDOMAIN" only means no records today — not necessarily that the name is
free to register. RDAP asks the authoritative registry; its `404` is the
registry stating the name is unallocated. That is the right signal for naming
decisions.

## Caveats

- **Available ≠ cheap.** RDAP `available` means unregistered at the registry.
  Premium, reserved, and aftermarket names may still be expensive or blocked at
  the registrar cart — RDAP does not price-shop.
- **Rate limits.** Public bootstrap (`rdap.org`) throttles rapid sequential
  queries. Batch mode sleeps between checks and retries on `429`; very large sweeps
  may still need a longer `--delay`.
- **Unknown happens.** Transient HTTP errors, unsupported TLD bootstrap paths, or
  malformed responses surface as `unknown` — retry or check the TLD manually.
- **whois is optional.** `--whois` is a human-readable supplement for registered
  names only; it is not used for availability verdicts and degrades cleanly when
  the binary is missing.
