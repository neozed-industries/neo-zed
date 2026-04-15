use gh_workflow::*;

use crate::tasks::workflows::{
    runners,
    steps::{self, NamedJob, RepositoryTarget, TokenPermissions, named},
    vars::StepOutput,
};

pub fn auto_tag_version_bump() -> Workflow {
    let tag_job = tag_merged_version_bump();

    named::workflow()
        .on(Event::default().pull_request(
            PullRequest::default()
                .add_type(PullRequestType::Closed)
                .add_branch("v*.x"),
        ))
        .add_job(tag_job.name, tag_job.job)
}

fn tag_merged_version_bump() -> NamedJob {
    fn validate_and_tag(token: &StepOutput) -> Step<Run> {
        named::bash(indoc::indoc! {r#"
            # Read version and channel from the merged commit
            version=$(script/get-crate-version zed)
            channel=$(cat crates/zed/RELEASE_CHANNEL)

            echo "Version: $version"
            echo "Channel: $channel"

            # Validate only version-related files were changed in the PR
            CHANGED_FILES=$(gh pr view "$PR_NUMBER" --json files --jq '.files[].path' | sort)
            echo "Changed files in PR:"
            echo "$CHANGED_FILES"

            ALLOWED_PATTERN="^(crates/zed/RELEASE_CHANNEL|crates/zed/Cargo\.toml|Cargo\.lock|Cargo\.toml)$"
            while IFS= read -r file; do
              if [ -z "$file" ]; then continue; fi
              if ! echo "$file" | grep -qE "$ALLOWED_PATTERN"; then
                echo "::error::Unexpected file changed: $file — only version-related files should be changed for auto-tagging"
                exit 1
              fi
            done <<< "$CHANGED_FILES"

            # Determine tag name based on channel
            case "$channel" in
              stable)
                tag="v${version}"
                ;;
              preview)
                tag="v${version}-pre"
                ;;
              *)
                echo "::error::Unexpected RELEASE_CHANNEL: '$channel' (expected 'stable' or 'preview')"
                exit 1
                ;;
            esac

            # Verify tag doesn't already exist
            if git ls-remote --tags origin "refs/tags/${tag}" | grep -q .; then
              echo "::error::Tag ${tag} already exists on remote"
              exit 1
            fi

            echo "Creating and pushing tag: $tag"
            git tag "$tag"
            git push origin "$tag"

            echo "Successfully tagged: $tag"
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
        .add_env(("PR_NUMBER", "${{ github.event.pull_request.number }}"))
    }

    let (authenticate, token) = steps::authenticate_as_zippy()
        .for_repository(RepositoryTarget::current())
        .with_permissions([(TokenPermissions::Contents, Level::Write)])
        .into();

    named::job(
        Job::default()
            .cond(Expression::new(
                "github.repository_owner == 'zed-industries' \
                 && github.event.pull_request.merged == true \
                 && github.event.pull_request.user.login == 'zed-zippy[bot]'",
            ))
            .runs_on(runners::LINUX_SMALL)
            .add_step(authenticate)
            .add_step(
                steps::checkout_repo()
                    .with_token(&token)
                    .with_ref("${{ github.event.pull_request.merge_commit_sha }}"),
            )
            .add_step(validate_and_tag(&token)),
    )
}
