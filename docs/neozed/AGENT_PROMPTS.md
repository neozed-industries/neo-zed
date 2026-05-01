# Agent Prompts

Use these prompts with Codex or Claude Code. Replace placeholders before sending.

## Feature or Bug Work

```text
Work on GitHub issue #ISSUE_NUMBER in this repository.

Follow docs/neozed/WORKFLOW.md before editing files.
Read docs/neozed/FORK_POLICY.md, docs/neozed/FEATURE_LEDGER.md, and the issue.
Keep the implementation scoped to the issue.
Target feature and ordinary bug-fix PRs at `main`, not `stable/v1.0`.
Update docs/neozed/FEATURE_LEDGER.md if this changes NeoZed product behavior.
Run the relevant build/test checks.
For UI changes, provide screenshot or screen recording proof.
Open or update a PR with the completed agent run log from docs/neozed/AGENT_RUN_LOG_TEMPLATE.md.
Do not merge upstream/main.
Do not revert unrelated local changes.
```

## Upstream Port Work

```text
Evaluate GitHub issue #ISSUE_NUMBER as an upstream port.

Follow docs/neozed/WORKFLOW.md, docs/neozed/FORK_POLICY.md, and docs/neozed/UPSTREAM_REVIEW.md.
Do not merge upstream/main.
Only cherry-pick or manually port the smallest fix if the bug exists in NeoZed or is high-risk.
Update docs/neozed/UPSTREAM_BASE.md with accepted or rejected upstream port details.
Run targeted verification.
Open or update a PR with the completed agent run log from docs/neozed/AGENT_RUN_LOG_TEMPLATE.md.
```

## Upstream Release Review

```text
Review GitHub issue #ISSUE_NUMBER for a new upstream Zed stable release.

Follow docs/neozed/WORKFLOW.md, docs/neozed/FORK_POLICY.md, and docs/neozed/UPSTREAM_REVIEW.md.
Classify each candidate commit as take / skip / needs manual review.
Do not merge upstream branches.
Do not auto-cherry-pick the full release.
Cherry-pick accepted commits into main first.
Backport to stable/v1.0 only when the stable backport policy is met.
Run targeted verification.
Update docs/neozed/UPSTREAM_REVIEW.md after review completion.
Open or update PRs with completed agent run logs.
```

## Stable Backport Work

```text
Backport GitHub issue #ISSUE_NUMBER to stable.

Follow docs/neozed/WORKFLOW.md and docs/neozed/FORK_POLICY.md.
Confirm the fix has already landed on main.
Target the PR at stable/v1.0.
Use git cherry-pick -x when possible so the stable commit records its source.
Backport only serious bug, crash, security, data-loss, or build/release fixes.
Do not include new feature work.
Run targeted verification and include proof in the PR agent run log.
```

## PR Cleanup

```text
Clean up the current PR for review.

Follow docs/neozed/WORKFLOW.md.
Verify the PR links its issue, includes the agent run log, documents commands run, includes UI proof if applicable, and updates the feature ledger or upstream base when required.
Verify the PR targets `main` unless it is explicitly a stable backport targeting `stable/v1.0`.
Run final targeted checks and report any gaps.
```
