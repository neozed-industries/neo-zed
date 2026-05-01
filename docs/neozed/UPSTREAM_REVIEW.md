# Upstream Review

NeoZed reviews upstream Zed stable releases on release events, not on every upstream branch update.

## Last Reviewed Upstream Stable

- Zed tag: `v1.0.0`
- Upstream commit: `5ec84a926ef83865afb92d2a3d1ca3b419572cf9`
- Reviewed on: `2026-05-01`
- NeoZed issue: N/A

## Review Policy

The upstream release watcher opens an issue when Zed publishes a newer stable tag. It must not cherry-pick, merge, or push code automatically.

When reviewing a new upstream stable tag:

1. Compare the new Zed stable tag against the last reviewed tag.
2. List candidate commits.
3. Classify each commit as `take`, `skip`, or `needs manual review`.
4. Cherry-pick accepted commits into `main` first.
5. Verify on `main`.
6. Backport eligible fixes to `stable/v1.0` only when they meet the stable backport policy in [Workflow](./WORKFLOW.md).
7. Update this file after the review is complete.

Accept by default only:

- bug fixes that affect NeoZed
- security fixes
- crash fixes
- data-loss fixes
- build or release fixes
- documentation changes that still apply to NeoZed

Do not automatically pull fixes for upstream features that NeoZed does not ship.

## Review History

| Zed tag | Upstream commit | NeoZed issue | Decision summary | Reviewed on |
| --- | --- | --- | --- | --- |
| `v1.0.0` | `5ec84a926ef83865afb92d2a3d1ca3b419572cf9` | N/A | Foundation baseline. | `2026-05-01` |
