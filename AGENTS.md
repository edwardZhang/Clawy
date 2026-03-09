# AGENTS.md

## Cursor Cloud specific instructions

### Overview

Clawy is a cross-platform **Tauri desktop app** (React 19 + Vite + TypeScript, Rust core) providing a GUI for the OpenClaw AI agent runtime. It uses pnpm as its package manager (pinned version in `package.json`'s `packageManager` field).

### Quick reference

Standard dev commands are in `package.json` scripts and `README.md`. Key ones:

| Task | Command |
|------|---------|
| Install deps + download uv | `pnpm run init` |
| Dev server (Vite + Tauri) | `pnpm dev` |
| Lint (ESLint, auto-fix) | `pnpm run lint` |
| Type check | `pnpm run typecheck` |
| Unit tests (Vitest) | `pnpm test` |
| Build frontend only | `pnpm run build:vite` |
| Build desktop app | `pnpm run build:tauri` |

### Non-obvious caveats

- **pnpm version**: The exact pnpm version is pinned via `packageManager` in `package.json`. Use `corepack enable && corepack prepare` to activate the correct version before installing.
- **Tauri on headless Linux**: The dbus errors (`Failed to connect to the bus`) are expected and harmless in a headless/cloud environment. The app still runs fine with `$DISPLAY` set (e.g., `:1` via Xvfb/VNC).
- **`pnpm run lint` race condition**: If `pnpm run uv:download` was recently run, ESLint may fail with `ENOENT: no such file or directory, scandir '/workspace/temp_uv_extract'` because the temp directory was created and removed during download. Simply re-run lint after the download script finishes.
- **Build scripts warning**: `pnpm install` may warn about ignored build scripts for `@discordjs/opus` and `koffi`. These are optional messaging-channel dependencies and the warnings are safe to ignore.
- **`pnpm run init`**: This is a convenience script that runs `pnpm install` followed by `pnpm run uv:download`. Either run `pnpm run init` or run the two steps separately.
- **Gateway startup**: When running `pnpm dev`, the OpenClaw Gateway process starts automatically on port 18789. It takes ~10-30 seconds to become ready. Gateway readiness is not required for UI development—the app functions without it (shows "connecting" state).
- **No database**: The app uses JSON files under the app data directory plus OS-native secure storage. No database setup is needed.
- **AI Provider keys**: Actual AI chat requires at least one provider API key configured via Settings > AI Providers. The app is fully navigable and testable without keys.
- **Token usage history implementation**: Dashboard token usage history is not parsed from console logs. It reads OpenClaw session transcript `.jsonl` files under the local OpenClaw config directory, extracts assistant messages with `message.usage`, and aggregates fields such as input/output/cache/total tokens and cost from those structured records.

## Default development workflow

- For any task that changes code, tests, build logic, packaging, docs, or configuration, you must follow the Trello + GitHub workflow skill at `/Users/mi/.codex/skills/trello-github-codex-workflow/SKILL.md`.
- The default flow in this repository is:
  1. Create a Trello card for the task.
  2. Move the card to `Inprogress`.
  3. Create a `codex/` feature branch.
  4. Implement the change in the current Codex session.
  5. Run the smallest relevant validation.
  6. Commit, push, and create a GitHub PR targeting `develop`.
  7. Merge the PR into `develop`.
  8. Move the Trello card to `Done`.
- Do not skip this workflow unless the user explicitly says not to create cards, not to create PRs, or not to follow the workflow.
- Never merge workflow-driven changes directly to `main`.

## Release packaging rules

- The main repository checkout at `/Users/mi/Projects/ClawX` is the only allowed source for final release packaging.
- Do not develop directly in the main checkout. Use `git worktree` plus `codex/` branches for task work, then merge into `develop`.
- Only package from a clean `develop` checkout that is fully synced to `origin/develop`.
- If the main checkout has uncommitted changes, untracked source/config files, or is ahead/behind/diverged from `origin/develop`, final packaging must be blocked until it is reconciled.
- Any local change that should appear in a release must be merged into `develop` before packaging. Do not package from ad hoc worktrees or detached checkouts as a workaround.
- `package`, `package:full`, `package:mac`, and all `package:mac:dmg*` commands must run release preflight first and fail fast if the repository is not in a known final state.

## Skills

### Available skills

- `trello-github-codex-workflow`: Default development workflow for this repository. Use it for any engineering task that should be tracked in Trello and merged through GitHub into `develop`. File: `/Users/mi/.codex/skills/trello-github-codex-workflow/SKILL.md`
