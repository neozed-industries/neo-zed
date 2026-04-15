use gh_workflow::*;
use serde_json::json;

use crate::tasks::workflows::{
    runners,
    steps::{self, NamedJob, RepositoryTarget, TokenPermissions, named},
    vars::{StepOutput, WorkflowInput},
};

pub fn bump_zed_version() -> Workflow {
    let bump_type = WorkflowInput::string("bump_type", None)
        .description("Version bump type: major, minor, or patch");
    let patch_channel = WorkflowInput::string("patch_channel", Some("both".to_string()))
        .description("For patch bumps only: preview, stable, or both");

    let plan = plan_version_bump(&bump_type, &patch_channel);
    let execute = execute_version_bump(&plan);

    named::workflow()
        .run_name(format!("bump_zed_version ({bump_type})"))
        .on(Event::default().workflow_dispatch(
            WorkflowDispatch::default()
                .add_input(bump_type.name, bump_type.input())
                .add_input(patch_channel.name, patch_channel.input()),
        ))
        .concurrency(Concurrency::new(Expression::new("bump-zed-version")))
        .add_job(plan.name, plan.job)
        .add_job(execute.name, execute.job)
}

fn plan_version_bump(bump_type: &WorkflowInput, patch_channel: &WorkflowInput) -> NamedJob {
    fn compute_matrix(bump_type: &WorkflowInput, patch_channel: &WorkflowInput) -> Step<Run> {
        named::bash(indoc::indoc! {r#"
            RELEASE_API="https://cloud.zed.dev/releases"

            # Validate inputs
            case "$BUMP_TYPE" in
              major|minor|patch) ;;
              *)
                echo "::error::Invalid bump_type: '$BUMP_TYPE' (must be major, minor, or patch)"
                exit 1
                ;;
            esac

            if [ "$BUMP_TYPE" = "patch" ]; then
              case "$PATCH_CHANNEL" in
                preview|stable|both) ;;
                *)
                  echo "::error::Invalid patch_channel: '$PATCH_CHANNEL' (must be preview, stable, or both)"
                  exit 1
                  ;;
              esac
            fi

            # Resolve the current preview and stable versions from the release API
            # so we don't rely on arithmetic that breaks across major version bumps.
            version_branch_for_channel() {
              local channel="$1"
              local version
              version=$(curl -fsSL "${RELEASE_API}/${channel}/latest/asset?asset=zed&os=macos&arch=aarch64" | jq -r '.version')
              if [ -z "$version" ] || [ "$version" = "null" ]; then
                echo "::error::Failed to resolve latest ${channel} version from API" >&2
                return 1
              fi
              local major minor
              major=$(echo "$version" | cut -d. -f1)
              minor=$(echo "$version" | cut -d. -f2)
              echo "v${major}.${minor}.x"
            }

            preview_branch=$(version_branch_for_channel preview)
            stable_branch=$(version_branch_for_channel stable)
            echo "Resolved preview branch: $preview_branch"
            echo "Resolved stable branch:  $stable_branch"

            # Read current version from main
            version=$(script/get-crate-version zed)
            major=$(echo "$version" | cut -d. -f1)
            minor=$(echo "$version" | cut -d. -f2)

            # Verify main is in dev/nightly state
            channel=$(cat crates/zed/RELEASE_CHANNEL)
            if [ "$channel" != "dev" ] && [ "$channel" != "nightly" ]; then
              echo "::error::RELEASE_CHANNEL on main must be 'dev' or 'nightly', found: '$channel'"
              exit 1
            fi

            if [ "$BUMP_TYPE" = "patch" ]; then
              matrix="[]"
              if [ "$PATCH_CHANNEL" = "preview" ] || [ "$PATCH_CHANNEL" = "both" ]; then
                matrix=$(echo "$matrix" | jq -c \
                  --arg checkout_ref "$preview_branch" \
                  --arg target_branch "$preview_branch" \
                  '. + [{"task": "bump-patch", "checkout_ref": $checkout_ref, "target_branch": $target_branch, "bump": "patch"}]')
              fi
              if [ "$PATCH_CHANNEL" = "stable" ] || [ "$PATCH_CHANNEL" = "both" ]; then
                matrix=$(echo "$matrix" | jq -c \
                  --arg checkout_ref "$stable_branch" \
                  --arg target_branch "$stable_branch" \
                  '. + [{"task": "bump-patch", "checkout_ref": $checkout_ref, "target_branch": $target_branch, "bump": "patch"}]')
              fi
            else
              # For major/minor bumps: bump main, create new preview branch, promote old preview to stable
              # The new preview branch is derived from the version currently on main.
              # The current preview branch (from the API) gets promoted to stable.
              new_preview_branch="v${major}.${minor}.x"

              matrix=$(jq -nc \
                --arg bump "$BUMP_TYPE" \
                --arg new_preview_branch "$new_preview_branch" \
                --arg old_preview_branch "$preview_branch" \
                '[
                  {"task": "bump-main", "checkout_ref": "main", "target_branch": "main", "bump": $bump},
                  {"task": "create-preview-branch", "checkout_ref": "main", "target_branch": $new_preview_branch, "bump": ""},
                  {"task": "promote-stable", "checkout_ref": $old_preview_branch, "target_branch": $old_preview_branch, "bump": ""}
                ]')
            fi

            echo "matrix=$matrix" >> "$GITHUB_OUTPUT"
            echo "Computed matrix:"
            echo "$matrix" | jq .
        "#})
        .id("compute-matrix")
        .add_env(("BUMP_TYPE", bump_type.to_string()))
        .add_env(("PATCH_CHANNEL", patch_channel.to_string()))
    }

    let (authenticate, token) = steps::authenticate_as_zippy().into();
    let step = compute_matrix(bump_type, patch_channel);
    let output = StepOutput::new(&step, "matrix");

    named::job(
        Job::default()
            .cond(Expression::new(
                "github.repository_owner == 'zed-industries'",
            ))
            .runs_on(runners::LINUX_SMALL)
            .add_step(authenticate)
            .add_step(steps::checkout_repo().with_token(&token))
            .add_step(step)
            .outputs([("matrix".to_owned(), output.to_string())]),
    )
}

fn execute_version_bump(plan: &NamedJob) -> NamedJob {
    fn run_version_bump(token: &StepOutput) -> Step<Run> {
        named::bash(indoc::indoc! {r#"
            echo "Task: $TASK"
            echo "Checkout ref: $CHECKOUT_REF"
            echo "Target branch: $TARGET_BRANCH"
            echo "Bump type: $BUMP"

            install_cargo_set_version() {
              which cargo-set-version > /dev/null || cargo install cargo-edit -f --no-default-features --features "set-version"
            }

            case "$TASK" in
              bump-main)
                install_cargo_set_version
                new_version="$(cargo set-version -p zed --bump "$BUMP" 2>&1 | sed 's/.* //')"

                pr_branch="bump-zed-to-${new_version}"
                git checkout -b "$pr_branch"
                git commit -am "Bump Zed to v${new_version}"
                git push origin "$pr_branch"

                printf "Bump Zed version to v%s on main.\n\nRelease Notes:\n\n- N/A" "$new_version" | \
                  gh pr create \
                    --title "Bump Zed to v${new_version}" \
                    --body-file - \
                    --base "$TARGET_BRANCH" \
                    --head "$pr_branch"
                ;;

              create-preview-branch)
                # Push the new version branch from main's current HEAD
                git push origin "HEAD:refs/heads/${TARGET_BRANCH}"

                pr_branch="set-channel-${TARGET_BRANCH}-preview"
                git checkout -b "$pr_branch"
                printf 'preview' > crates/zed/RELEASE_CHANNEL
                git commit -am "${TARGET_BRANCH} preview"
                git push origin "$pr_branch"

                printf "Set RELEASE_CHANNEL to preview on %s.\n\nRelease Notes:\n\n- N/A" "$TARGET_BRANCH" | \
                  gh pr create \
                    --title "${TARGET_BRANCH} preview" \
                    --body-file - \
                    --base "$TARGET_BRANCH" \
                    --head "$pr_branch"
                ;;

              promote-stable)
                pr_branch="promote-${TARGET_BRANCH}-to-stable"
                git checkout -b "$pr_branch"
                printf 'stable' > crates/zed/RELEASE_CHANNEL
                git commit -am "${TARGET_BRANCH} stable"
                git push origin "$pr_branch"

                printf "Promote %s to stable.\n\nRelease Notes:\n\n- N/A" "$TARGET_BRANCH" | \
                  gh pr create \
                    --title "${TARGET_BRANCH} stable" \
                    --body-file - \
                    --base "$TARGET_BRANCH" \
                    --head "$pr_branch"
                ;;

              bump-patch)
                install_cargo_set_version
                new_version="$(cargo set-version -p zed --bump patch 2>&1 | sed 's/.* //')"
                channel=$(cat crates/zed/RELEASE_CHANNEL)

                pr_branch="bump-${TARGET_BRANCH}-to-${new_version}"
                git checkout -b "$pr_branch"
                git commit -am "Bump Zed to v${new_version} (${channel})"
                git push origin "$pr_branch"

                printf "Bump Zed to v%s on %s (%s).\n\nRelease Notes:\n\n- N/A" "$new_version" "$TARGET_BRANCH" "$channel" | \
                  gh pr create \
                    --title "Bump Zed to v${new_version} (${channel})" \
                    --body-file - \
                    --base "$TARGET_BRANCH" \
                    --head "$pr_branch"
                ;;

              *)
                echo "::error::Unknown task: $TASK"
                exit 1
                ;;
            esac
        "#})
        .add_env(("GIT_COMMITTER_NAME", "Zed Zippy"))
        .add_env((
            "GIT_COMMITTER_EMAIL",
            "234243425+zed-zippy[bot]@users.noreply.github.com",
        ))
        .add_env(("GIT_AUTHOR_NAME", "Zed Zippy"))
        .add_env((
            "GIT_AUTHOR_EMAIL",
            "234243425+zed-zippy[bot]@users.noreply.github.com",
        ))
        .add_env(("GITHUB_TOKEN", token))
        .add_env(("TASK", "${{ matrix.task }}"))
        .add_env(("CHECKOUT_REF", "${{ matrix.checkout_ref }}"))
        .add_env(("TARGET_BRANCH", "${{ matrix.target_branch }}"))
        .add_env(("BUMP", "${{ matrix.bump }}"))
    }

    let (authenticate, token) = steps::authenticate_as_zippy()
        .for_repository(RepositoryTarget::current())
        .with_permissions([
            (TokenPermissions::Contents, Level::Write),
            (TokenPermissions::PullRequests, Level::Write),
        ])
        .into();

    named::job(
        Job::default()
            .needs(vec![plan.name.clone()])
            .cond(Expression::new(format!(
                "needs.{}.outputs.matrix != '[]'",
                plan.name
            )))
            .runs_on(runners::LINUX_XL)
            .strategy(Strategy::default().fail_fast(false).matrix(json!({
                "include": format!("${{{{ fromJson(needs.{}.outputs.matrix) }}}}", plan.name)
            })))
            .add_step(authenticate)
            .add_step(
                steps::checkout_repo()
                    .with_token(&token)
                    .with_ref("${{ matrix.checkout_ref }}"),
            )
            .add_step(run_version_bump(&token)),
    )
}
