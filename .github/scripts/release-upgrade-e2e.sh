#!/usr/bin/env bash
# Exercise upgrading from the previous published Linux release in an isolated HOME.
# Environment: GITHUB_EVENT_NAME, GITHUB_REF_NAME, EVENT_RELEASE_TAG,
#              GITHUB_REPOSITORY, GH_TOKEN, RUNNER_TEMP, BRANCH_BINARY, REPORT_FILE
set -Eeuo pipefail

ISSUE_MARKER='[release-upgrade-e2e] linux previous-release upgrade failed'
ISSUE_TITLE="$ISSUE_MARKER"
# The healthy final line `shipmates doctor` prints (src/doctor.rs). The
# lifecycle mock below prints this variable rather than the sentence itself, so
# the mock and the assertion cannot drift apart from each other — but this
# constant would still agree with a mock while disagreeing with the binary.
# `--doctor-control <binary>` is what pins it to a real build: it installs with
# the binary, reads the last line its `doctor` prints, and fails unless that is
# this sentence. The pages workflow runs it on every pull request.
DOCTOR_HEALTHY_LINE='All shipshape. Your crew is aboard and current.'
ASSET='shipmates-x86_64-unknown-linux-gnu.tar.xz'
CHECKSUM_ASSET="${ASSET}.sha256"
DOWNLOAD_ATTEMPTS=3
RUNNER_TEMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
REPORT_FILE="${REPORT_FILE:-$RUNNER_TEMP/release-upgrade-e2e-report.md}"
SCRIPT_PATH="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/$(basename -- "${BASH_SOURCE[0]}")"
WORK_DIR=''
TEST_HOME=''
PREVIOUS_TAG=''
LAST_STEP_LOG=''
RECEIPT_PATH=''
RECEIPT_PATH_SNAPSHOT=''
RECEIPT_FILES=()
SELF_TEST_DIR=''
DOCTOR_CONTROL_DIR=''

usage() {
  cat <<'USAGE'
Usage: release-upgrade-e2e.sh [--run|--self-test|--report|--doctor-control <binary>]

  --run                      Run the published-release upgrade lifecycle (default).
  --self-test                Exercise lifecycle failure labels and issue dedup with mocks.
  --report                   File a deduplicated issue from REPORT_FILE, if present.
  --doctor-control <binary>  Check DOCTOR_HEALTHY_LINE against the last line a
                             binary built by `cargo build --locked --bin
                             shipmates` prints for a healthy `doctor`.
USAGE
}

cleanup_doctor_control() {
  if [[ -n "$DOCTOR_CONTROL_DIR" && -d "$DOCTOR_CONTROL_DIR" ]]; then
    rm -rf -- "$DOCTOR_CONTROL_DIR"
  fi
  DOCTOR_CONTROL_DIR=''
}

cleanup_self_test() {
  if [[ -n "$SELF_TEST_DIR" && -d "$SELF_TEST_DIR" ]]; then
    rm -rf -- "$SELF_TEST_DIR"
  fi
  SELF_TEST_DIR=''
}

cleanup_run() {
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    rm -rf -- "$WORK_DIR"
  fi
  if [[ -n "$TEST_HOME" && -d "$TEST_HOME" ]]; then
    rm -rf -- "$TEST_HOME"
  fi
}

write_failure_report() {
  local label=$1
  local log_file=$2
  local detail=$3
  shift 3

  mkdir -p -- "$(dirname -- "$REPORT_FILE")"
  {
    printf 'FAILED STEP: %s\n\n' "$label"
    printf 'Previous published release: %s\n' "${PREVIOUS_TAG:-unknown}"
    printf 'Command:'
    printf ' %q' "$@"
    printf '\n'
    if [[ -n "$detail" ]]; then
      printf 'Failure detail: %s\n' "$detail"
    fi
    printf '\nRelevant log tail:\n```text\n'
    if [[ -f "$log_file" ]]; then
      tail -n 60 -- "$log_file" || true
    else
      printf 'No command log was produced.\n'
    fi
    printf '```\n'
  } > "$REPORT_FILE"
}

fail_with_report() {
  local label=$1
  local log_file=$2
  local detail=$3
  shift 3
  write_failure_report "$label" "$log_file" "$detail" "$@"
  printf 'FAILED STEP: %s\n' "$label" >&2
  if [[ -n "$detail" ]]; then
    printf '%s\n' "$detail" >&2
  fi
  exit 1
}

run_step() {
  local label=$1
  shift
  if [[ "${1:-}" != '--' ]]; then
    printf 'run_step requires: run_step <label> -- <command...>\n' >&2
    return 64
  fi
  shift

  local log_file="$WORK_DIR/${label}.log"
  local status=0
  LAST_STEP_LOG=$log_file

  if "$@" > "$log_file" 2>&1; then
    status=0
  else
    status=$?
  fi
  cat -- "$log_file"
  if (( status != 0 )); then
    fail_with_report "$label" "$log_file" "command exited with status $status" "$@"
  fi
}

# A required tool that is missing is a preflight failure like any other: it
# goes through the failure-report path so the separate `file-issue` job — which
# reads REPORT_FILE and nothing else — still files a report.
require_tool() {
  local tool=$1
  local log_file=$2
  if command -v "$tool" >/dev/null 2>&1; then
    return 0
  fi
  printf 'Required command is unavailable: %s\n' "$tool" > "$log_file"
  preflight_failure "$log_file" "required command is unavailable: $tool" command -v "$tool"
}

# Anything that fails before the lifecycle steps run — a missing tool, an
# unusable release list, a failed download or checksum, an archive without a
# binary — reports under its own `preflight` label. Labelling all of them
# `install` named a step that never ran and hid what actually broke.
preflight_failure() {
  local log_file=$1
  local detail=$2
  shift 2
  fail_with_report preflight "$log_file" "$detail" "$@"
}

select_previous_release() {
  local list_log="$WORK_DIR/release-list.log"
  local release_tags=''
  local excluded_tag=''
  local candidate=''

  if [[ "${GITHUB_EVENT_NAME:-}" == 'release' ]]; then
    excluded_tag="${EVENT_RELEASE_TAG:-${GITHUB_REF_NAME:-}}"
    if [[ -z "$excluded_tag" ]]; then
      printf 'Release event did not provide its tag through EVENT_RELEASE_TAG or GITHUB_REF_NAME.\n' > "$list_log"
      preflight_failure "$list_log" 'cannot exclude the release event tag' gh release list
    fi
  fi

  if ! gh release list --repo "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}" \
    --exclude-drafts --limit 1000 --json tagName --jq '.[].tagName' > "$list_log" 2>&1; then
    preflight_failure "$list_log" 'could not list published releases' gh release list --repo "$GITHUB_REPOSITORY" --exclude-drafts --limit 1000 --json tagName --jq '.[].tagName'
  fi
  release_tags=$(cat -- "$list_log")

  while IFS= read -r candidate; do
    [[ -n "$candidate" ]] || continue
    [[ "$candidate" != "$excluded_tag" ]] || continue
    if [[ ! "$candidate" =~ ^[[:alnum:]][[:alnum:].+_-]*$ ]]; then
      # A tag with an unexpected shape is not a failure: the tag only ever
      # reaches `gh` as a quoted argv, and the next published release is just
      # as good a previous release to upgrade from.
      printf 'Skipping release tag with an unsupported format: %s\n' "$candidate"
      continue
    fi
    PREVIOUS_TAG=$candidate
    break
  done <<< "$release_tags"

  if [[ -z "$PREVIOUS_TAG" ]]; then
    printf 'No published release was available after excluding %s.\n' "${excluded_tag:-<none>}" > "$list_log"
    preflight_failure "$list_log" 'no previous published release was found' gh release list --repo "$GITHUB_REPOSITORY" --exclude-drafts --limit 1000 --json tagName --jq '.[].tagName'
  fi
  printf 'Selected previous published release: %s\n' "$PREVIOUS_TAG"
}

verify_download_checksum() {
  local checksum_file="$WORK_DIR/download/$CHECKSUM_ASSET"
  local archive_file="$WORK_DIR/download/$ASSET"
  local checksum_line=''
  local digest=''
  local listed_asset=''
  local extra=''
  local verify_log="$WORK_DIR/checksum.log"

  checksum_line=$(cat -- "$checksum_file")
  if [[ "$checksum_line" == *$'\n'* ]]; then
    printf 'Checksum asset must contain exactly one checksum line.\n' > "$verify_log"
    preflight_failure "$verify_log" 'invalid checksum asset format' cat "$checksum_file"
  fi
  read -r digest listed_asset extra <<< "$checksum_line"
  if [[ ! "$digest" =~ ^[[:xdigit:]]{64}$ || -n "$extra" ]]; then
    printf 'Checksum asset must contain a SHA-256 digest and at most one filename.\n' > "$verify_log"
    preflight_failure "$verify_log" 'invalid checksum asset format' cat "$checksum_file"
  fi
  if [[ -n "$listed_asset" && "$listed_asset" != "$ASSET" && "$listed_asset" != "*$ASSET" ]]; then
    printf 'Checksum names an unexpected asset: %s\n' "$listed_asset" > "$verify_log"
    preflight_failure "$verify_log" 'checksum asset does not name the required Linux tarball' cat "$checksum_file"
  fi

  printf '%s  %s\n' "$digest" "$archive_file" > "$WORK_DIR/download/verified.sha256"
  if command -v sha256sum >/dev/null 2>&1; then
    if ! sha256sum -c "$WORK_DIR/download/verified.sha256" > "$verify_log" 2>&1; then
      preflight_failure "$verify_log" 'Linux release tarball failed SHA-256 verification' sha256sum -c "$WORK_DIR/download/verified.sha256"
    fi
  elif command -v shasum >/dev/null 2>&1; then
    if ! shasum -a 256 -c "$WORK_DIR/download/verified.sha256" > "$verify_log" 2>&1; then
      preflight_failure "$verify_log" 'Linux release tarball failed SHA-256 verification' shasum -a 256 -c "$WORK_DIR/download/verified.sha256"
    fi
  else
    printf 'Neither sha256sum nor shasum is available.\n' > "$verify_log"
    preflight_failure "$verify_log" 'no SHA-256 verifier is installed' sha256sum -c "$WORK_DIR/download/verified.sha256"
  fi
  cat -- "$verify_log"
}

download_and_extract_release() {
  local download_dir="$WORK_DIR/download"
  local extract_dir="$WORK_DIR/extracted"
  local download_log="$WORK_DIR/download.log"
  local attempt=1
  local downloaded=false

  mkdir -p -- "$download_dir" "$extract_dir"
  while (( attempt <= DOWNLOAD_ATTEMPTS )); do
    rm -f -- "$download_dir/$ASSET" "$download_dir/$CHECKSUM_ASSET"
    if gh release download "$PREVIOUS_TAG" --repo "$GITHUB_REPOSITORY" \
      --pattern "$ASSET" --pattern "$CHECKSUM_ASSET" --dir "$download_dir" --clobber > "$download_log" 2>&1 \
      && [[ -s "$download_dir/$ASSET" && -s "$download_dir/$CHECKSUM_ASSET" ]]; then
      downloaded=true
      break
    fi
    printf 'Download attempt %s/%s failed.\n' "$attempt" "$DOWNLOAD_ATTEMPTS" >> "$download_log"
    if (( attempt < DOWNLOAD_ATTEMPTS )); then
      sleep 2
    fi
    attempt=$((attempt + 1))
  done

  if [[ "$downloaded" != true ]]; then
    preflight_failure "$download_log" "could not download both exact release assets after $DOWNLOAD_ATTEMPTS attempts" \
      gh release download "$PREVIOUS_TAG" --repo "$GITHUB_REPOSITORY" --pattern "$ASSET" --pattern "$CHECKSUM_ASSET" --dir "$download_dir" --clobber
  fi
  cat -- "$download_log"

  # Verify the exact downloaded archive before extraction or executing its binary.
  verify_download_checksum
  local extract_log="$WORK_DIR/extract.log"
  if ! tar -xJf "$download_dir/$ASSET" -C "$extract_dir" > "$extract_log" 2>&1; then
    preflight_failure "$extract_log" 'verified release tarball could not be extracted' tar -xJf "$download_dir/$ASSET" -C "$extract_dir"
  fi
  RELEASED_BINARY=$(find "$extract_dir" -type f -name shipmates -print -quit)
  if [[ -z "$RELEASED_BINARY" || ! -x "$RELEASED_BINARY" ]]; then
    printf 'No executable shipmates binary was found in the verified release archive.\n' > "$extract_log"
    preflight_failure "$extract_log" 'verified archive did not contain an executable shipmates binary' find "$extract_dir" -type f -name shipmates -print -quit
  fi
  printf 'Verified previous-release binary: %s\n' "$RELEASED_BINARY"
}

read_install_receipt() {
  local log_file="$WORK_DIR/install-receipt.log"
  local path=''
  RECEIPT_PATH="$HOME/.shipmates/receipts/claude-code.json"
  if [[ ! -f "$RECEIPT_PATH" ]] || ! jq -e '.harness == "claude-code" and (.files | type == "array" and length > 0)' "$RECEIPT_PATH" >/dev/null 2>&1; then
    printf 'Expected claude-code install receipt was missing or invalid: %s\n' "$RECEIPT_PATH" > "$log_file"
    fail_with_report install "$log_file" 'install did not produce a valid harness receipt' "$RELEASED_BINARY" install --global --harness claude-code
  fi

  RECEIPT_FILES=()
  while IFS= read -r path; do
    if [[ -z "$path" || "$path" == /* || "$path" == *$'\n'* || "/$path/" == *'/../'* || "/$path/" == *'/./'* ]]; then
      printf 'Receipt contained an unsafe or empty owned-file path.\n' > "$log_file"
      fail_with_report install "$log_file" 'install receipt contained an invalid owned-file path' "$RELEASED_BINARY" install --global --harness claude-code
    fi
    RECEIPT_FILES+=("$path")
  done < <(jq -r '.files[].path' "$RECEIPT_PATH")
}

verify_doctor_result() {
  local final_line=''
  final_line=$(awk 'NF { last = $0 } END { print last }' "$LAST_STEP_LOG")
  # `print_report` (src/doctor.rs) prints the summary sentence as its final
  # statement and returns straight after it, so a healthy doctor's last
  # non-empty line IS the whole sentence — exact equality holds on every
  # healthy install. Asserting the opening words instead would let a reworded
  # summary line through on its stable prefix alone.
  if [[ "$final_line" != "$DOCTOR_HEALTHY_LINE" ]]; then
    fail_with_report doctor "$LAST_STEP_LOG" 'doctor did not finish with the healthy summary line' \
      "$BRANCH_BINARY" doctor --global --harness claude-code
  fi
}

verify_uninstall_result() {
  local log_file="$WORK_DIR/uninstall-assertions.log"
  local relative_path=''

  if [[ -e "$RECEIPT_PATH_SNAPSHOT" ]]; then
    printf 'Receipt remained after uninstall: %s\n' "$RECEIPT_PATH_SNAPSHOT" > "$log_file"
    fail_with_report uninstall "$log_file" 'the snapshotted receipt was not removed' \
      "$BRANCH_BINARY" uninstall --global --harness claude-code
  fi
  for relative_path in "${RECEIPT_FILES[@]}"; do
    if [[ -e "$HOME/$relative_path" ]]; then
      printf 'Receipt-owned file remained after uninstall: %s\n' "$HOME/$relative_path" > "$log_file"
      fail_with_report uninstall "$log_file" 'a receipt-owned harness file remained' \
        "$BRANCH_BINARY" uninstall --global --harness claude-code
    fi
  done

  if [[ -f "$HOME/.shipmates/installs.json" ]] \
    && ! jq -e '[.records[]? | select(.harness == "claude-code")] | length == 0' \
      "$HOME/.shipmates/installs.json" >/dev/null 2>&1; then
    printf 'The installs index still references claude-code.\n' > "$log_file"
    fail_with_report uninstall "$log_file" 'the install index still references the harness' \
      "$BRANCH_BINARY" uninstall --global --harness claude-code
  fi
  if [[ -f "$HOME/.shipmates/installs.json" ]]; then
    printf 'The empty %s file is expected index metadata, not an uninstall leak.\n' \
      "$HOME/.shipmates/installs.json"
  fi
}

run_lifecycle() {
  local branch_binary=${BRANCH_BINARY:-}
  local home_parent="$RUNNER_TEMP"

  if [[ -z "$branch_binary" || ! -x "$branch_binary" ]]; then
    local branch_log="$WORK_DIR/branch-binary.log"
    printf 'BRANCH_BINARY must name an executable branch build.\n' > "$branch_log"
    preflight_failure "$branch_log" 'branch binary was not available' "${branch_binary:-<unset>}"
  fi
  if [[ ! -d "$home_parent" ]]; then
    mkdir -p -- "$home_parent"
  fi
  TEST_HOME=$(mktemp -d "$home_parent/release-upgrade-e2e-home.XXXXXX")
  export HOME="$TEST_HOME"

  # The released binary is downloaded third-party code, and the branch binary
  # needs no workflow credentials either: strip them from this process before
  # either runs, so neither inherits GH_TOKEN. `--report` is a separate
  # process that still holds the token for issue filing.
  unset GH_TOKEN GITHUB_TOKEN ACTIONS_RUNTIME_TOKEN ACTIONS_ID_TOKEN_REQUEST_TOKEN

  run_step install -- "$RELEASED_BINARY" install --global --harness claude-code
  read_install_receipt

  run_step update -- "$branch_binary" update --global --harness claude-code
  # Refresh the receipt-owned file snapshot after update.
  read_install_receipt

  run_step doctor -- "$branch_binary" doctor --global --harness claude-code
  verify_doctor_result

  # Snapshot the receipt path immediately before uninstall for exact cleanup checks.
  RECEIPT_PATH_SNAPSHOT=$RECEIPT_PATH
  run_step uninstall -- "$branch_binary" uninstall --global --harness claude-code
  verify_uninstall_result
}

report_issue() {
  local issues=''
  if [[ ! -s "$REPORT_FILE" ]]; then
    printf 'No release-upgrade failure report found; no issue was created.\n'
    return 0
  fi
  if ! issues=$(gh issue list --repo "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}" \
    --state open --search "in:title \"$ISSUE_MARKER\"" --json number --limit 1000); then
    printf 'Could not query open issues for deduplication.\n' >&2
    return 1
  fi
  if ! jq -e 'type == "array"' <<< "$issues" >/dev/null; then
    printf 'GitHub returned an invalid issue-list response; refusing to create a duplicate.\n' >&2
    return 1
  fi
  if jq -e 'length > 0' <<< "$issues" >/dev/null; then
    printf 'An open issue already matches the release-upgrade failure marker.\n'
    return 0
  fi
  gh issue create --repo "$GITHUB_REPOSITORY" --title "$ISSUE_TITLE" --body-file "$REPORT_FILE"
}

create_mock_commands() {
  local mock_bin=$1
  cat > "$mock_bin/gh" <<'MOCK_GH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == release && "${2:-}" == list ]]; then
  if [[ "${MOCK_RELEASE_LIST_FAILS:-0}" != 0 ]]; then
    printf 'simulated gh release list failure\n' >&2
    exit 1
  fi
  tags=${MOCK_RELEASE_TAGS:-}
  if [[ -z "$tags" ]]; then
    tags=$'v1.0.1\nv1.0.0'
  fi
  printf '%s\n' "$tags"
  exit 0
fi
if [[ "${1:-}" == release && "${2:-}" == download ]]; then
  shift 2
  tag=${1:-}
  shift || true
  download_dir=''
  patterns=()
  while (($#)); do
    case "$1" in
      --repo) shift 2 ;;
      --pattern) patterns+=("${2:-}"); shift 2 ;;
      --dir) download_dir=${2:-}; shift 2 ;;
      --clobber) shift ;;
      *) printf 'unexpected gh release download argument: %s\n' "$1" >&2; exit 2 ;;
    esac
  done
  printf '%s\n' "$tag" >> "$MOCK_DOWNLOAD_LOG"
  attempt=0
  if [[ -f "$MOCK_DOWNLOAD_ATTEMPTS" ]]; then
    attempt=$(cat "$MOCK_DOWNLOAD_ATTEMPTS")
  fi
  attempt=$((attempt + 1))
  printf '%s\n' "$attempt" > "$MOCK_DOWNLOAD_ATTEMPTS"
  if (( attempt <= MOCK_DOWNLOAD_FAILS )); then
    printf 'simulated transient download failure\n' >&2
    exit 1
  fi
  [[ ${#patterns[@]} -eq 2 && "${patterns[0]}" == "$MOCK_ASSET" && "${patterns[1]}" == "$MOCK_CHECKSUM_ASSET" ]] || exit 3
  mkdir -p -- "$download_dir"
  cp -- "$MOCK_FIXTURE_DIR/$MOCK_ASSET" "$download_dir/$MOCK_ASSET"
  cp -- "$MOCK_FIXTURE_DIR/$MOCK_CHECKSUM_ASSET" "$download_dir/$MOCK_CHECKSUM_ASSET"
  exit 0
fi
if [[ "${1:-}" == issue && "${2:-}" == list ]]; then
  search=''
  shift 2
  while (($#)); do
    case "$1" in
      --repo) shift 2 ;;
      --search) search=${2:-}; shift 2 ;;
      --state|--json|--limit) shift 2 ;;
      *) printf 'unexpected gh issue list argument: %s\n' "$1" >&2; exit 2 ;;
    esac
  done
  [[ "$search" == 'in:title "[release-upgrade-e2e] linux previous-release upgrade failed"' ]] || exit 4
  count=$(cat "$MOCK_ISSUE_COUNT")
  if (( count > 0 )); then
    printf '[{"number":1}]\n'
  else
    printf '[]\n'
  fi
  printf '%s\n' "$(( $(cat "$MOCK_ISSUE_LIST_CALLS") + 1 ))" > "$MOCK_ISSUE_LIST_CALLS"
  exit 0
fi
if [[ "${1:-}" == issue && "${2:-}" == create ]]; then
  title=''
  body_file=''
  shift 2
  while (($#)); do
    case "$1" in
      --repo) shift 2 ;;
      --title) title=${2:-}; shift 2 ;;
      --body-file) body_file=${2:-}; shift 2 ;;
      *) printf 'unexpected gh issue create argument: %s\n' "$1" >&2; exit 2 ;;
    esac
  done
  [[ "$title" == '[release-upgrade-e2e] linux previous-release upgrade failed' && -s "$body_file" ]] || exit 5
  count=$(cat "$MOCK_ISSUE_COUNT")
  printf '%s\n' "$((count + 1))" > "$MOCK_ISSUE_COUNT"
  printf 'https://example.invalid/issues/1\n'
  exit 0
fi
printf 'unexpected gh command: %s\n' "$*" >&2
exit 2
MOCK_GH

  cat > "$mock_bin/shipmates" <<'MOCK_SHIPMATES'
#!/usr/bin/env bash
set -euo pipefail
command_name=${1:-}
# The lifecycle binaries are executed by run_lifecycle, which must strip the
# workflow's credentials first. Fail loudly if a token reaches this mock.
if [[ -n "${GH_TOKEN:-}" || -n "${GITHUB_TOKEN:-}" ]]; then
  printf 'mock lifecycle command inherited a workflow credential; run_lifecycle must strip it\n' >&2
  exit 9
fi
printf '%s\n' "$command_name" >> "$MOCK_LIFECYCLE_LOG"
if [[ "${2:-}" != --global || "${3:-}" != --harness || "${4:-}" != claude-code ]]; then
  printf 'unexpected lifecycle command arguments: %s\n' "$*" >&2
  exit 2
fi
if [[ "${FAIL_LIFECYCLE_COMMAND:-}" == "$command_name" ]]; then
  printf 'simulated %s failure\n' "$command_name" >&2
  exit 17
fi
case "$command_name" in
  install)
    mkdir -p -- "$HOME/.shipmates/receipts" "$HOME/.claude/agents"
    printf 'mock agent\n' > "$HOME/.claude/agents/mock.md"
    jq -n '{schema_version:1,version:"1.0.0",harness:"claude-code",layout:"skills",roots:[".claude/agents"],files:[{path:".claude/agents/mock.md",sha256:("0" * 64)}]}' \
      > "$HOME/.shipmates/receipts/claude-code.json"
    jq -n --arg root "$HOME" '{schema_version:1,records:[{root:$root,harness:"claude-code",version:"1.0.0",layout:"skills",last_seen:1}]}' \
      > "$HOME/.shipmates/installs.json"
    ;;
  update)
    [[ -f "$HOME/.shipmates/receipts/claude-code.json" ]] || exit 6
    ;;
  doctor)
    [[ -f "$HOME/.shipmates/receipts/claude-code.json" ]] || exit 7
    # The healthy line comes from the caller's DOCTOR_HEALTHY_LINE, so this
    # mock and the assertion read one constant. Whether that constant still
    # matches the binary is `--doctor-control`'s job, not this mock's.
    printf '%s\n' "${DOCTOR_HEALTHY_LINE:?}"
    ;;
  uninstall)
    rm -f -- "$HOME/.claude/agents/mock.md" "$HOME/.shipmates/receipts/claude-code.json"
    jq -n '{schema_version:1,records:[]}' > "$HOME/.shipmates/installs.json"
    ;;
  *) printf 'unexpected lifecycle command: %s\n' "$command_name" >&2; exit 8 ;;
esac
MOCK_SHIPMATES
  chmod +x "$mock_bin/gh" "$mock_bin/shipmates"
}

# Build a PATH directory holding only the named tools plus the utilities the
# reporter itself needs, so a self-test can run the script with a required
# tool genuinely absent.
make_tool_path() {
  local dir=$1
  shift
  local tool=''
  local resolved=''
  mkdir -p -- "$dir"
  for tool in bash dirname basename mkdir mktemp rm cat tail "$@"; do
    resolved=$(command -v "$tool" || true)
    if [[ -n "$resolved" ]]; then
      ln -sf -- "$resolved" "$dir/$tool"
    fi
  done
}

self_test() {
  local self_root
  local fixture_tree
  local mock_bin
  local digest
  local step
  local output_file
  local report_path
  local success_steps
  local current_download
  local doctor_line
  local bash_path

  # The mock shipped by create_mock_commands expands this variable at run time;
  # keep the heredoc delimiter quoted so its `$` references stay literal.
  export DOCTOR_HEALTHY_LINE

  self_root=$(mktemp -d "${RUNNER_TEMP}/release-upgrade-self-test.XXXXXX")
  SELF_TEST_DIR=$self_root
  trap 'cleanup_self_test' RETURN
  fixture_tree="$self_root/release-tree/shipmates-x86_64-unknown-linux-gnu"
  mock_bin="$self_root/mock-bin"
  mkdir -p -- "$fixture_tree" "$mock_bin" "$self_root/fixture-assets"
  create_mock_commands "$mock_bin"

  cat > "$fixture_tree/shipmates" <<'RELEASED_MOCK_BINARY'
#!/usr/bin/env bash
exec "${MOCK_BIN}/shipmates" "$@"
RELEASED_MOCK_BINARY
  chmod +x "$fixture_tree/shipmates"
  tar -cJf "$self_root/fixture-assets/$ASSET" -C "$self_root/release-tree" shipmates-x86_64-unknown-linux-gnu
  if command -v sha256sum >/dev/null 2>&1; then
    digest=$(sha256sum "$self_root/fixture-assets/$ASSET" | cut -d ' ' -f 1)
  else
    digest=$(shasum -a 256 "$self_root/fixture-assets/$ASSET" | cut -d ' ' -f 1)
  fi
  printf '%s  %s\n' "$digest" "$ASSET" > "$self_root/fixture-assets/$CHECKSUM_ASSET"

  export PATH="$mock_bin:$PATH"
  export MOCK_BIN="$mock_bin"
  export MOCK_ASSET="$ASSET"
  export MOCK_CHECKSUM_ASSET="$CHECKSUM_ASSET"
  export MOCK_FIXTURE_DIR="$self_root/fixture-assets"
  export MOCK_DOWNLOAD_LOG="$self_root/download-tags.log"
  export MOCK_DOWNLOAD_ATTEMPTS="$self_root/download-attempts"
  export MOCK_ISSUE_COUNT="$self_root/issue-count"
  export MOCK_ISSUE_LIST_CALLS="$self_root/issue-list-calls"
  export MOCK_LIFECYCLE_LOG="$self_root/lifecycle.log"
  export RUNNER_TEMP="$self_root/runner-temp"
  export GITHUB_REPOSITORY='owner/repository'
  export BRANCH_BINARY="$mock_bin/shipmates"
  # Sentinel credentials: every mock lifecycle command asserts it never sees
  # them, proving run_lifecycle strips the environment before executing a
  # downloaded binary.
  export GH_TOKEN='self-test-sentinel'
  export GITHUB_TOKEN='self-test-sentinel'
  mkdir -p -- "$RUNNER_TEMP"
  printf '0\n' > "$MOCK_ISSUE_COUNT"
  printf '0\n' > "$MOCK_ISSUE_LIST_CALLS"
  export MOCK_DOWNLOAD_FAILS=1
  printf '0\n' > "$MOCK_DOWNLOAD_ATTEMPTS"
  : > "$MOCK_DOWNLOAD_LOG"
  : > "$MOCK_LIFECYCLE_LOG"

  # A release event must skip its own tag, and a transient download is retried.
  export GITHUB_EVENT_NAME='release'
  export GITHUB_REF_NAME='v1.0.1'
  export EVENT_RELEASE_TAG='v1.0.1'
  export REPORT_FILE="$self_root/success-report.md"
  if ! bash "$SCRIPT_PATH" --run > "$self_root/success.log" 2>&1; then
    cat -- "$self_root/success.log" >&2
    printf 'self-test: complete lifecycle unexpectedly failed\n' >&2
    return 1
  fi
  current_download=$(tail -n 1 "$MOCK_DOWNLOAD_LOG")
  [[ "$current_download" == 'v1.0.0' ]] || {
    printf 'self-test: release event did not exclude its own tag\n' >&2
    return 1
  }
  [[ "$(cat "$MOCK_DOWNLOAD_ATTEMPTS")" == '2' ]] || {
    printf 'self-test: bounded download retry was not exercised\n' >&2
    return 1
  }
  success_steps=$(paste -sd ' ' "$MOCK_LIFECYCLE_LOG")
  [[ "$success_steps" == 'install update doctor uninstall' ]] || {
    printf 'self-test: lifecycle command sequence was unexpected: %s\n' "$success_steps" >&2
    return 1
  }
  [[ ! -e "$REPORT_FILE" ]] || {
    printf 'self-test: success unexpectedly produced a failure report\n' >&2
    return 1
  }

  # The success run above is the doctor assertion's control: verify_doctor_result
  # compares doctor's last line for exact equality with DOCTOR_HEALTHY_LINE, so
  # a mock shortened to the assertion's opening words fails this run. Whether
  # the constant itself still matches the binary is `--doctor-control`'s check,
  # run against a real build in the pages workflow.
  doctor_line=$(grep -F -m 1 -- "$DOCTOR_HEALTHY_LINE" "$self_root/success.log" || true)
  [[ -n "$doctor_line" ]] || {
    printf 'self-test: the success path did not exercise the real doctor line: %s\n' \
      "$DOCTOR_HEALTHY_LINE" >&2
    return 1
  }
  [[ "$doctor_line" != 'All shipshape.' ]] || {
    printf 'self-test: the exercised doctor line was cut back to the prefix an exact comparison would accept\n' >&2
    return 1
  }

  # A newest tag with an unexpected shape must be skipped, not fatal: the tag
  # only ever reaches `gh` as a quoted argv.
  export GITHUB_EVENT_NAME='schedule'
  unset EVENT_RELEASE_TAG
  export MOCK_RELEASE_TAGS=$'bad/tag\nv1.0.0'
  export MOCK_DOWNLOAD_FAILS=0
  export REPORT_FILE="$self_root/unsafe-tag-report.md"
  : > "$MOCK_DOWNLOAD_LOG"
  printf '0\n' > "$MOCK_DOWNLOAD_ATTEMPTS"
  if ! bash "$SCRIPT_PATH" --run > "$self_root/unsafe-tag.log" 2>&1; then
    cat -- "$self_root/unsafe-tag.log" >&2
    printf 'self-test: an unsupported release tag was fatal instead of skipped\n' >&2
    return 1
  fi
  grep -Fq 'Skipping release tag with an unsupported format: bad/tag' "$self_root/unsafe-tag.log" || {
    printf 'self-test: the unsupported release tag was not reported as skipped\n' >&2
    return 1
  }
  [[ "$(tail -n 1 "$MOCK_DOWNLOAD_LOG")" == 'v1.0.0' ]] || {
    printf 'self-test: the run did not continue to the next published release\n' >&2
    return 1
  }
  [[ ! -e "$REPORT_FILE" ]] || {
    printf 'self-test: a skipped release tag produced a failure report\n' >&2
    return 1
  }
  unset MOCK_RELEASE_TAGS

  # Every preflight failure — a failed release lookup, a missing required
  # tool, a missing digest verifier — must carry its own `preflight` label and
  # must still leave a report behind, because the separate `file-issue` job
  # reads REPORT_FILE and nothing else.
  assert_preflight_report() {
    local name=$1
    local detail=$2
    local out_file=$3
    local report_file=$4
    if [[ "$(grep -Fxc 'FAILED STEP: preflight' "$out_file")" != '1' ]] \
      || [[ "$(grep -Fxc 'FAILED STEP: preflight' "$report_file")" != '1' ]]; then
      cat -- "$out_file" >&2
      printf 'self-test: %s did not report the preflight label\n' "$name" >&2
      return 1
    fi
    grep -Fq -- "$detail" "$report_file" || {
      printf 'self-test: %s report did not name the failure: %s\n' "$name" "$detail" >&2
      return 1
    }
  }

  export MOCK_RELEASE_LIST_FAILS=1
  export REPORT_FILE="$self_root/preflight-list-report.md"
  if bash "$SCRIPT_PATH" --run > "$self_root/preflight-list.log" 2>&1; then
    printf 'self-test: a failed release lookup was not reported\n' >&2
    return 1
  fi
  assert_preflight_report 'release lookup' 'could not list published releases' \
    "$self_root/preflight-list.log" "$REPORT_FILE" || return 1
  export MOCK_RELEASE_LIST_FAILS=0

  # A missing required tool must reach the reporter, not only stderr.
  bash_path=$(command -v bash)
  make_tool_path "$self_root/no-jq-bin" gh
  export REPORT_FILE="$self_root/preflight-tool-report.md"
  if PATH="$self_root/no-jq-bin" "$bash_path" "$SCRIPT_PATH" --run \
    > "$self_root/preflight-tool.log" 2>&1; then
    printf 'self-test: a missing required tool was not reported\n' >&2
    return 1
  fi
  assert_preflight_report 'missing required tool' 'required command is unavailable: jq' \
    "$self_root/preflight-tool.log" "$REPORT_FILE" || return 1

  # So must a missing SHA-256 verifier.
  make_tool_path "$self_root/no-digest-bin" gh jq tar
  export REPORT_FILE="$self_root/preflight-digest-report.md"
  if PATH="$self_root/no-digest-bin" "$bash_path" "$SCRIPT_PATH" --run \
    > "$self_root/preflight-digest.log" 2>&1; then
    printf 'self-test: a missing digest verifier was not reported\n' >&2
    return 1
  fi
  assert_preflight_report 'missing digest verifier' 'no SHA-256 verifier is installed' \
    "$self_root/preflight-digest.log" "$REPORT_FILE" || return 1

  # Every lifecycle command failure must report exactly its own step label.
  for step in install update doctor uninstall; do
    output_file="$self_root/failure-$step.log"
    report_path="$self_root/failure-$step-report.md"
    : > "$MOCK_LIFECYCLE_LOG"
    : > "$MOCK_DOWNLOAD_LOG"
    printf '0\n' > "$MOCK_DOWNLOAD_ATTEMPTS"
    export MOCK_DOWNLOAD_FAILS=0
    export GITHUB_EVENT_NAME='schedule'
    unset EVENT_RELEASE_TAG
    export GITHUB_REF_NAME='main'
    export FAIL_LIFECYCLE_COMMAND="$step"
    export REPORT_FILE="$report_path"
    if bash "$SCRIPT_PATH" --run > "$output_file" 2>&1; then
      printf 'self-test: %s failure was not reported\n' "$step" >&2
      return 1
    fi
    if [[ "$(grep -Fxc "FAILED STEP: $step" "$output_file")" != '1' ]] \
      || [[ "$(grep -Fxc "FAILED STEP: $step" "$report_path")" != '1' ]]; then
      cat -- "$output_file" >&2
      printf 'self-test: expected exactly one FAILED STEP: %s label\n' "$step" >&2
      return 1
    fi
    grep -Fq "Command: " "$report_path" || {
      printf 'self-test: %s report omitted its command\n' "$step" >&2
      return 1
    }
    grep -Fq 'Relevant log tail:' "$report_path" || {
      printf 'self-test: %s report omitted its log tail\n' "$step" >&2
      return 1
    }
  done
  unset FAIL_LIFECYCLE_COMMAND

  # Reporting the same failure twice must create at most one open issue.
  export REPORT_FILE="$self_root/failure-update-report.md"
  if ! bash "$SCRIPT_PATH" --report > "$self_root/report-first.log" 2>&1; then
    cat -- "$self_root/report-first.log" >&2
    printf 'self-test: first issue report failed\n' >&2
    return 1
  fi
  if ! bash "$SCRIPT_PATH" --report > "$self_root/report-second.log" 2>&1; then
    cat -- "$self_root/report-second.log" >&2
    printf 'self-test: repeated issue report failed\n' >&2
    return 1
  fi
  [[ "$(cat "$MOCK_ISSUE_COUNT")" == '1' && "$(cat "$MOCK_ISSUE_LIST_CALLS")" == '2' ]] || {
    printf 'self-test: repeated report created more than one issue or skipped deduplication\n' >&2
    return 1
  }

  printf 'Self-test passed: release exclusion, bounded retry, unsupported-tag skip, real doctor line, lifecycle sequence, labels install/update/doctor/uninstall, preflight reporting for a failed lookup / missing tool / missing digest verifier, and one-issue deduplication.\n'
}

# Pin DOCTOR_HEALTHY_LINE to the sentence a real build prints. Nothing else in
# this script compares the constant with the binary, so without this a reworded
# doctor summary line would leave the self-test green against a mock that agrees
# with the constant and disagrees with what a captain sees (#571).
doctor_control() {
  local binary=${1:-}
  local control_root=''
  local control_home=''
  local control_target=''
  local install_log=''
  local doctor_log=''
  local definition_site=''
  local status=0
  local last_line=''

  if [[ -z "$binary" ]]; then
    printf 'Doctor control needs the path to a shipmates binary: release-upgrade-e2e.sh --doctor-control target/debug/shipmates\n' >&2
    printf 'Build it first with: cargo build --locked --bin shipmates — this check compares against a real build, never a constant.\n' >&2
    return 64
  fi
  if [[ ! -f "$binary" || ! -x "$binary" ]]; then
    printf 'Doctor control needs an executable shipmates binary; %s is not one.\n' "$binary" >&2
    printf 'Build it first with: cargo build --locked --bin shipmates, then pass target/debug/shipmates.\n' >&2
    return 1
  fi

  mkdir -p -- "$RUNNER_TEMP"
  control_root=$(mktemp -d "$RUNNER_TEMP/release-upgrade-doctor-control.XXXXXX")
  DOCTOR_CONTROL_DIR=$control_root
  trap 'cleanup_doctor_control' RETURN
  control_home="$control_root/home"
  control_target="$control_root/target"
  install_log="$control_root/install.log"
  doctor_log="$control_root/doctor.log"
  mkdir -p -- "$control_home" "$control_target"

  if ! HOME="$control_home" "$binary" install --harness claude-code \
    --dir "$control_target" --with-tools none > "$install_log" 2>&1; then
    cat -- "$install_log" >&2
    printf 'Doctor control: %s install into a throwaway directory failed, so there is no healthy install to read the doctor line from.\n' "$binary" >&2
    return 1
  fi

  if HOME="$control_home" "$binary" doctor --dir "$control_target" > "$doctor_log" 2>&1; then
    status=0
  else
    status=$?
  fi
  if (( status != 0 )); then
    cat -- "$doctor_log" >&2
    printf 'Doctor control: %s doctor exited with status %s on a fresh install; a healthy install exits 0.\n' \
      "$binary" "$status" >&2
    return 1
  fi

  last_line=$(awk 'NF { last = $0 } END { print last }' "$doctor_log")
  if [[ "$last_line" != "$DOCTOR_HEALTHY_LINE" ]]; then
    definition_site=$(grep -n -m 1 '^DOCTOR_HEALTHY_LINE=' "$SCRIPT_PATH" || printf '%s' "$SCRIPT_PATH")
    printf 'Doctor control: DOCTOR_HEALTHY_LINE no longer matches the real binary.\n' >&2
    printf '  expected sentence: %s\n' "$DOCTOR_HEALTHY_LINE" >&2
    printf '  observed last line: %s\n' "$last_line" >&2
    printf '  constant defined at: %s\n' "$definition_site" >&2
    printf '  source of truth: src/doctor.rs — print_report prints the healthy summary as its final line.\n' >&2
    printf 'Update the constant in %s to the sentence src/doctor.rs prints.\n' "$SCRIPT_PATH" >&2
    return 1
  fi

  printf 'Doctor control passed: %s prints %s\n' "$binary" "$DOCTOR_HEALTHY_LINE"
}

run_mode() {
  local report_dir
  local preflight_log

  mkdir -p -- "$RUNNER_TEMP"
  report_dir=$(dirname -- "$REPORT_FILE")
  mkdir -p -- "$report_dir"
  rm -f -- "$REPORT_FILE"
  WORK_DIR=$(mktemp -d "$RUNNER_TEMP/release-upgrade-e2e.XXXXXX")
  trap cleanup_run EXIT

  preflight_log="$WORK_DIR/preflight.log"
  require_tool gh "$preflight_log"
  require_tool jq "$preflight_log"
  require_tool tar "$preflight_log"
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    printf 'Neither sha256sum nor shasum is available.\n' > "$preflight_log"
    preflight_failure "$preflight_log" 'no SHA-256 verifier is installed' \
      sha256sum -c "$WORK_DIR/download/verified.sha256"
  fi

  select_previous_release
  download_and_extract_release
  run_lifecycle
}

main() {
  local mode=${1:---run}
  # Arity is per mode: every mode but --doctor-control takes no further
  # argument, and --doctor-control takes exactly one path. A single "more than
  # one argument" guard rejected the only mode that needs an argument.
  local max_args=1
  case "$mode" in
    --run|--self-test|--report|-h|--help)
      ;;
    --doctor-control)
      max_args=2
      ;;
    *)
      usage >&2
      return 64
      ;;
  esac
  if (($# > max_args)); then
    usage >&2
    return 64
  fi

  case "$mode" in
    --run)
      run_mode
      ;;
    --self-test)
      self_test
      ;;
    --report)
      report_issue
      ;;
    --doctor-control)
      # A missing path is reported by doctor_control itself, with the
      # `cargo build` instruction a bare usage dump would not give.
      doctor_control "${2:-}"
      ;;
    -h|--help)
      usage
      ;;
  esac
}

main "$@"
