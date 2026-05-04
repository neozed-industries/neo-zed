# Feature Ledger

This ledger tracks NeoZed product behavior added on top of Zed.

Update this file whenever a feature is proposed, started, shipped, paused, abandoned, or removed.

## Status Values

- `proposed`: accepted for investigation, not implemented
- `in-progress`: implementation branch or PR exists
- `shipped`: merged into NeoZed main
- `paused`: intentionally stopped but may resume
- `abandoned`: intentionally stopped and should not be resumed without a new issue
- `removed`: previously shipped, later removed

## Features

| Feature | Issue | Status | Owner | Branch or PR | User-visible behavior | Verification proof | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| NeoZed brand identity | #3 | in-progress | agent | `feature/3-complete-brand-identity` | Neo Zed app identity, default NeoZed themes, package metadata, local app/data/credential/pasteboard/IPC namespaces, project-local `.neozed/*` config, local runtime `NEOZED_*` environment overrides, public/internal `neozed://` and `neozed-cli://` URL handling with legacy `zed://` accepted only as explicit compatibility input, `neozed` terminal command, and visible onboarding/menu/update/settings labels replace upstream Zed branding where scoped. | `cargo fmt --all -- --check`; `git diff --check`; `cargo test -p zed_credentials_provider`; `cargo check --workspace --all-targets`; `cargo check -p cli -p zed -p settings -p json_schema_store -p project -p settings_ui`; `./script/clippy -p cli -p client -p install_cli -p release_channel -p paths -p util -p remote -p remote_server -p auto_update -p zed_credentials_provider -p zed -p settings_content -p theme -p project -p agent -p workspace -p ui -p debugger_ui -p tasks_ui -p gpui_wgpu -p zlog -p gpui_util -p feature_flags -p language -p git -p crashes -p system_specs -p edit_prediction_cli -p settings -p json_schema_store -p settings_ui`; `script/bundle-mac -d -i -o` compiled and then hit local `cargo bundle` `Term(ColorOutOfRange)` panic; `TERM= script/bundle-mac -d -i -o` installed and opened `/Applications/Neo Zed.app`; `/Applications/Neo Zed.app/Contents/MacOS/cli --version` printed `Neo Zed 1.0.0 - /Applications/Neo Zed.app`; screenshots in `docs/neozed/proofs/issue-3/`; `graphify update .` rebuilt graph files, then exited with a graphify tip-printing `NameError`. | Cloud/server/web URLs, cloud release asset lookup names, task-template `$ZED_*` variables, wire headers, and upstream/provenance/package-manager references are intentionally preserved. |
| Browser annotation ingestion | TBD | removed | agent | PR #1 | None; browser annotation extension, IPC host/protocol, deep-link pairing, and agent-panel ingestion were removed. | `cargo check -p agent_ui -p zed`; `git diff --check`; `rg -n "BrowserAnnotation\|browser_annotation\|browser annotation\|Browser annotation\|browser-annotation\|chromium_annotation"` | Removed per maintainer request. |

## Do Not Resurrect

Use this section for ideas that agents should not rediscover and reimplement.

| Feature or approach | Decision issue | Reason |
| --- | --- | --- |
