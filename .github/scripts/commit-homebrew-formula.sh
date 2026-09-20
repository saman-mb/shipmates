#!/usr/bin/env bash
# Commit and push Homebrew formula files to the tap repository.
# Environment: GITHUB_USER, GITHUB_EMAIL, PLAN
set -euo pipefail

git config --global user.name "${GITHUB_USER}"
git config --global user.email "${GITHUB_EMAIL}"

for release in $(echo "$PLAN" | jq --compact-output '.releases[] | select([.artifacts[] | endswith(".rb")] | any)'); do
  filename=$(echo "$release" | jq '.artifacts[] | select(endswith(".rb"))' --raw-output)
  name="${filename%.rb}"
  version=$(echo "$release" | jq .app_version --raw-output)

  export PATH="/home/linuxbrew/.linuxbrew/bin:$PATH"
  brew update
  # We avoid reformatting user-provided data such as the app description and homepage.
  brew style --except-cops FormulaAudit/Homepage,FormulaAudit/Desc,FormulaAuditStrict --fix "Formula/${filename}" || true

  git add "Formula/${filename}"
  git commit -m "${name} ${version}"
done
git push
