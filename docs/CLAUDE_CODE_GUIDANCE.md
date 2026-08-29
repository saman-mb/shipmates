# Mastering Claude Code in 30 minutes
**Speaker: Boris Cherny** (Member of Technical Staff, Anthropic; creator of Claude Code)

---

## 1. Environment & Setup
- Run `terminal setup` — provides Shift+Enter for new lines.
- Customize the set of allowed tools so you are not prompted for permissions on every execution.
- Dictation can be useful to generate verbose, detailed prompts. Speak to Claude Code like another engineer.

## 2. Codebase Q&A (Onboarding & Discovery)
- **Start with Q&A**: Begin by asking questions about the codebase before using complex tools or editing code.
- **Git History Q&A**: Ask about git history (e.g., "Why does this function have these arguments?", "Who introduced this pattern?"). Claude Code can automatically look through Git logs and issues to explain.
- **No Indexing**: There is no remote indexing; code stays local, and generative models are not trained on the code.

## 3. Editing Code & Planning
- **Plan First**: Before having Claude write code, ask it to brainstorm and make a plan. For example: *"before you write code, brainstorm ideas, make a plan, run it by me, and ask for approval."*
- **Git Actions**: Incantations like *"commit, push, PR"* are understood. Claude looks at git logs to match commit message styles automatically.

## 4. Custom Tools & Context
- **CLI --help Usage**: Tell Claude about a local CLI tool and instruct it to run `--help` to learn how to use it.
- **CLAUDE.md / Project Instructions**: Use a project instructions file in the root of your repository to provide persistent project context, code styling rules, and development guidelines for the model to follow.
