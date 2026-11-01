#!/usr/bin/env bash
# Publish the assembled pi-shipmates package, or say plainly that there was
# nothing to publish.
#
# Two rules, both learned on pi-cuttle's publisher. First, the registry — not git
# history — decides whether a version is out: comparing against the previous
# commit misreads a merge or a squash and silently skips a real release, while
# asking the registry makes a re-run a no-op instead of an error. Second, a run
# that claims to have published must prove the registry serves it, or the run is
# red. A green publish job that published nothing is the failure mode to design
# out.
#
# Environment:
#   OUT_DIR    required, package root produced by package-pi-npm.sh
#   VERSION    required, npm version (release tag without the "v")
#   PKG_NAME   npm package name  (default: pi-shipmates)
set -euo pipefail

: "${OUT_DIR:?OUT_DIR is required}"
: "${VERSION:?VERSION is required}"
VERSION="${VERSION#v}"
PKG_NAME="${PKG_NAME:-pi-shipmates}"

already=$(npm view "${PKG_NAME}@${VERSION}" version 2>/dev/null || true)
if [ "$already" = "$VERSION" ]; then
  echo "::notice::Skipped npm publish — ${PKG_NAME}@${VERSION} is already on the registry"
  {
    echo "### npm publish skipped"
    echo
    echo "\`${PKG_NAME}@${VERSION}\` was already on the registry, so this run published nothing."
  } >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
  exit 0
fi

( cd "$OUT_DIR" && npm publish --provenance --access public )

for attempt in 1 2 3 4 5 6; do
  if [ "$(npm view "${PKG_NAME}@${VERSION}" version 2>/dev/null || true)" = "$VERSION" ]; then
    echo "::notice::${PKG_NAME}@${VERSION} is live on the registry"
    exit 0
  fi
  echo "attempt ${attempt}: ${PKG_NAME}@${VERSION} not visible yet, waiting"
  sleep 20
done

echo "::error::npm reported success but ${PKG_NAME}@${VERSION} is not on the registry"
exit 1
