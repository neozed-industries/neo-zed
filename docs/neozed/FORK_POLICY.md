# NeoZed Fork Policy

NeoZed is a product fork of Zed, not a continuously rebased downstream mirror.

## Policy

NeoZed maintains its own product roadmap after the chosen upstream base. Upstream Zed is a source of selected fixes, not the default direction of the product.

Do not routinely merge `upstream/main` into NeoZed branches.

## Accepted Upstream Changes

An upstream change may be ported when all are true:

- The issue exists in NeoZed.
- The change is a bug fix, security fix, build fix, crash fix, or data-loss fix.
- The change does not depend on a large post-base feature stack.
- The change can be verified locally.
- The PR documents why the port is needed.

## Rejected Upstream Changes

Do not port upstream changes just because they are new. Reject by default:

- new upstream product features
- large refactors unrelated to a NeoZed issue
- UI behavior changes that conflict with NeoZed direction
- changes that require importing an upstream feature stack only to obtain a small fix

## Upstream Port Process

1. Create an `Upstream port` issue.
2. Link the upstream commit, PR, or release note.
3. Confirm the bug exists in NeoZed or is high-risk enough to preempt.
4. Cherry-pick or manually port the smallest viable change.
5. Run targeted tests and build checks.
6. Update [Upstream base](./UPSTREAM_BASE.md) if a port is accepted.
7. Add the port to the PR run log.

## Conflict Rule

When upstream behavior and NeoZed behavior disagree, NeoZed product intent wins unless the upstream change fixes security, data loss, corruption, or a severe crash.
