use collections::HashMap;
use feature_flags::{FeatureFlag, FeatureFlagAppExt};

struct GitJobDebugViewFeatureFlag;

impl FeatureFlag for GitJobDebugViewFeatureFlag {
    const NAME: &'static str = "git-job-debug-view";

    fn enabled_for_staff() -> bool {
        true
    }
}
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Task, Window, actions,
};
use project::{Project, git_store::GitStoreEvent};
use ui::{Label, LabelCommon, LabelSize, div, h_flex, prelude::FluentBuilder, v_flex};
use workspace::{Item, SplitDirection, Workspace};

actions!(dev, [OpenGitJobDebugView]);

pub fn init(cx: &mut App) {
    cx.observe_new(move |workspace: &mut Workspace, _, _| {
        workspace.register_action(move |workspace, _: &OpenGitJobDebugView, window, cx| {
            if !cx.has_flag::<GitJobDebugViewFeatureFlag>() {
                return;
            }
            let project = workspace.project().clone();
            let view = cx.new(|cx| GitJobDebugView::new(project, window, cx));
            workspace.split_item(SplitDirection::Right, Box::new(view), window, cx)
        });
    })
    .detach();
}

pub struct GitJobDebugView {
    project: Entity<Project>,
    focus_handle: FocusHandle,
    _git_store_subscription: Subscription,
    queue_observations: HashMap<EntityId, Subscription>,
}

impl GitJobDebugView {
    fn new(project: Entity<Project>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let git_store = project.read(cx).git_store().clone();
        let git_store_subscription =
            cx.subscribe(&git_store, |this: &mut Self, _, event, cx| match event {
                GitStoreEvent::RepositoryAdded | GitStoreEvent::RepositoryRemoved(_) => {
                    this.observe_queues(cx);
                }
                _ => {}
            });

        let mut this = Self {
            project,
            focus_handle: cx.focus_handle(),
            _git_store_subscription: git_store_subscription,
            queue_observations: HashMap::default(),
        };
        this.observe_queues(cx);
        this
    }

    fn observe_queues(&mut self, cx: &mut Context<Self>) {
        self.queue_observations.clear();
        let queues: Vec<_> = self
            .project
            .read(cx)
            .repositories(cx)
            .values()
            .filter_map(|repo| repo.read(cx).job_queue().upgrade())
            .collect();
        for queue in queues {
            let id = queue.entity_id();
            let subscription = cx.observe(&queue, |_, _, cx| cx.notify());
            self.queue_observations.insert(id, subscription);
        }
        cx.notify();
    }
}

impl Render for GitJobDebugView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let repositories = self.project.read(cx).repositories(cx).clone();

        let mut repo_sections = Vec::new();

        for (repo_id, repo_entity) in &repositories {
            let repo = repo_entity.read(cx);
            let current_job = repo.current_job(cx);
            let repo_path: SharedString = repo
                .work_directory_abs_path
                .to_string_lossy()
                .to_string()
                .into();

            let mut rows = Vec::new();

            if let Some(job_info) = current_job {
                rows.push(
                    h_flex()
                        .gap_4()
                        .child(
                            Label::new("▶ running")
                                .size(LabelSize::Small)
                                .color(ui::Color::Success),
                        )
                        .child(Label::new(job_info.message.clone()).size(LabelSize::Small))
                        .into_any_element(),
                );
            }

            if let Some(queue) = repo.job_queue().upgrade() {
                let queue = queue.read(cx);
                for job in queue.jobs() {
                    let location = job.source_location();
                    let location_str: SharedString = format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                    .into();

                    rows.push(
                        h_flex()
                            .gap_4()
                            .child(
                                Label::new("◻ queued")
                                    .size(LabelSize::Small)
                                    .color(ui::Color::Muted),
                            )
                            .child(Label::new(job.name().clone()).size(LabelSize::Small))
                            .when_some(job.key(), |this, key| {
                                this.child(
                                    Label::new(key.to_string())
                                        .size(LabelSize::Small)
                                        .color(ui::Color::Muted),
                                )
                            })
                            .child(
                                Label::new(location_str)
                                    .size(LabelSize::Small)
                                    .color(ui::Color::Muted),
                            )
                            .into_any_element(),
                    );
                }
            }

            repo_sections.push(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new(format!("Repository {} ({})", repo_id.0, repo_path))
                            .size(LabelSize::Default),
                    )
                    .when(rows.is_empty(), |this| {
                        this.child(
                            Label::new("  (idle)")
                                .size(LabelSize::Small)
                                .color(ui::Color::Muted),
                        )
                    })
                    .children(rows)
                    .into_any_element(),
            );
        }

        div()
            .flex_1()
            .size_full()
            .p_4()
            .track_focus(&self.focus_handle)
            .child(
                v_flex()
                    .gap_4()
                    .child(Label::new("Git Job Queue Debug View"))
                    .when(repo_sections.is_empty(), |this| {
                        this.child(
                            Label::new("No repositories")
                                .size(LabelSize::Small)
                                .color(ui::Color::Muted),
                        )
                    })
                    .children(repo_sections),
            )
    }
}

impl EventEmitter<()> for GitJobDebugView {}

impl Focusable for GitJobDebugView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for GitJobDebugView {
    type Event = ();

    fn to_item_events(_: &Self::Event, _: &mut dyn FnMut(workspace::item::ItemEvent)) {}

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Git Job Queue".into()
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        None
    }

    fn clone_on_split(
        &self,
        _: Option<workspace::WorkspaceId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Option<Entity<Self>>>
    where
        Self: Sized,
    {
        let project = self.project.clone();
        Task::ready(Some(cx.new(|cx| Self::new(project, window, cx))))
    }
}
