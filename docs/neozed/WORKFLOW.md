# NeoZed Workflow

## Decision

Track every NeoZed feature, bug, and upstream port in GitHub issues. Do not rely on chat history as the product record.

Agents should not pick up work automatically unless a later automation layer assigns an issue explicitly. The normal flow is:

1. Create or update a GitHub issue.
2. Prompt Codex or Claude Code with the issue number and this workflow.
3. The agent creates a branch, implements the work, verifies it, updates the ledger, and opens or updates a pull request.
4. The pull request contains the agent run log and proof.
5. Merge only after the issue, ledger, and evidence agree.

## Issue Types

Use these issue templates:

- `NeoZed feature`: product behavior added on top of Zed.
- `NeoZed bug`: bug in NeoZed behavior or a Zed bug that affects NeoZed.
- `Upstream port`: selected Zed upstream fix to consider cherry-picking.

Every issue must include:

- product intent
- acceptance criteria
- non-goals
- expected verification
- screenshots or recordings required, when UI behavior changes

## Branches

Long-lived branches:

- `main`: NeoZed development branch. Feature and bug-fix PRs target this branch.
- `stable/v1.0`: stable release branch founded on Zed `upstream/v1.0.x`. Only selected fixes are backported here.

Use these branch prefixes:

- `feature/<issue-number>-short-name`
- `fix/<issue-number>-short-name`
- `upstream-port/<issue-number>-short-name`
- `docs/<issue-number>-short-name`

Do not use `main` as the stable release branch. `main` is allowed to move with NeoZed development after the Zed 1.0 foundation. Stable releases are cut from `stable/v1.0` and tagged from that branch.

Do not merge upstream `main` directly into NeoZed product branches.

## Stable And Dev Releases

NeoZed uses two release channels:

- Dev: built locally or from `main`.
- Stable: built from `stable/v1.0` and published by tags.

Stable release tags must point at the stable branch:

- `v1.0.0`
- `v1.0.1`
- `v1.0.2`

Do not tag arbitrary feature branches as stable releases.

## Selecting Stable Commits

A commit may be backported from `main` to `stable/v1.0` only when all are true:

- It fixes a serious bug, crash, security issue, data-loss issue, or build/release blocker.
- It has already landed on `main`.
- It is small enough to backport without pulling unrelated feature work.
- It has an issue or PR note explaining why it is safe for stable.
- It has targeted verification evidence.

Use a backport PR into `stable/v1.0`. Prefer `git cherry-pick -x <commit>` so the stable commit records its source.

## Upstream Release Watch

The `upstream-release-watch` workflow checks for new upstream Zed stable tags and opens a review issue when one appears.

The watcher must not merge, cherry-pick, or push upstream code. It only creates review work.

Agents handling those issues must follow [Upstream review](./UPSTREAM_REVIEW.md).

## Agent Start Checklist

Before editing files, the agent must:

1. Read the GitHub issue.
2. Read this directory.
3. Check `git status --short`.
4. Identify unrelated local changes and leave them alone.
5. Inspect relevant code and graphify context before choosing an implementation.
6. Restate the intended scope in the chat or PR notes.

## Implementation Rules

- Keep changes scoped to the issue.
- Do not add opportunistic features.
- Do not perform broad upstream merges.
- Prefer existing Zed patterns over new abstractions.
- Update [Feature ledger](./FEATURE_LEDGER.md) for every product feature.
- Update [Upstream base](./UPSTREAM_BASE.md) only when changing the fork base or accepting an upstream port.

## Verification Evidence

Every pull request must include an agent run log with:

- commands run
- whether each command passed or failed
- screenshots, recordings, or artifact paths when UI behavior changed
- manual test steps performed
- known gaps and why they are acceptable

For UI changes, proof should usually include at least one screenshot. For interaction changes, prefer a screen recording or a clear step-by-step manual verification note.

## Pull Request Rules

The PR must:

- link the issue
- summarize product behavior, not just code changes
- include the agent run log
- include release notes
- update the feature ledger when adding, changing, pausing, or removing a NeoZed feature

## Recommended Daily Flow

Use issues as the queue. Use chat as the executor.

For each task, prompt the agent with:

> Work on issue #123. Follow `docs/neozed/WORKFLOW.md`. Keep the issue and feature ledger updated. Open a PR with proof of verification.

This keeps the durable state in GitHub and the repo, while still letting agents work interactively.
