## What this PR does

_Describe the changes you've made in this PR here._

## Motivation and Context

_Why is this change required? What problem does it solve? If it fixes an open issue, please link to the issue here._

## Affected Harnesses

_Please check all harnesses that this change affects, or "All" if it's a general change._

- [ ] All
- [ ] claude-code
- [ ] opencode
- [ ] antigravity
- [ ] codex
- [ ] cursor
- [ ] github-copilot
- [ ] windsurf

## How Has This Been Tested?

_Please describe the tests that you ran to verify your changes._

## Checklist:

- [ ] My code follows the principles outlined in `AGENTS.md` and `CONTRIBUTING.md`.
- [ ] I have verified the behaviour end to end.
- [ ] I have added/updated tests to cover my changes.
- [ ] All new and existing tests passed (`cargo test`).
- [ ] I have regenerated the payload digests for any affected harnesses (`cargo run -- build --target <target> --update`).
- [ ] If my change affects documentation or web assets, I have run `python3 tools/gen_command_pages.py`.
