# Upstream Base

This file records the fork boundary and accepted upstream ports.

## Foundation

- Product fork: NeoZed
- Upstream repository: `https://github.com/zed-industries/zed`
- Fork repository: `https://github.com/neozed-industries/neo-zed`
- Target upstream foundation line: `upstream/v1.0.x`
- Remote `v1.0.0` / `upstream/v1.0.x` commit at merge time: `5ec84a926ef83865afb92d2a3d1ca3b419572cf9`
- Local `v1.0.0` tag observed before merge: `68ff700b0426aa99bd5251a255330f423d26e5bd`
- Current observed `main` commit at setup time: `d60b55675b7a8a06d8a0ac35cd9e59cf2acb1d77`
- Current observed working branch at setup time: `browser-annotation-ingestion`

## Base Adoption Status

Status: `main` was fast-forwarded to `upstream/v1.0.x`.

Merge commit: `984d86e0ed`.

The local `v1.0.0` tag differed from the remote `v1.0.0` ref during setup. The 1.0 merge used `upstream/v1.0.x` instead of the stale local tag so the merged code matches the upstream 1.0 line.

## Release Branch Policy

- `main` is the NeoZed development branch after the Zed 1.0 foundation.
- `stable/v1.0` is the stable release branch and should be created from the Zed 1.0 foundation.
- Stable tags such as `v1.0.0`, `v1.0.1`, and `v1.0.2` must point at commits on `stable/v1.0`.
- New features land on `main`.
- Only selected fixes are backported from `main` to `stable/v1.0`.

## Accepted Upstream Ports

| Date | NeoZed issue | Upstream source | Commit(s) | Reason | Verification |
| --- | --- | --- | --- | --- | --- |

## Rejected Upstream Ports

| Date | NeoZed issue | Upstream source | Reason rejected |
| --- | --- | --- | --- |
