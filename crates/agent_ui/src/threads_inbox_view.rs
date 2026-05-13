use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use acp_thread::{PermissionOptions, SelectedPermissionOutcome};
use agent_client_protocol::schema as acp;
use chrono::{DateTime, Utc};
use editor::Editor;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, ListState, MouseButton,
    Render, SharedString, Subscription, Window, list, prelude::*, px,
};
use menu::{
    Confirm, SelectChild, SelectFirst, SelectLast, SelectNext, SelectParent, SelectPrevious,
};
use settings::Settings as _;
use theme::ActiveTheme;
use ui::{
    ContextMenu, Disclosure, Divider, KeyBinding, Label, LabelSize, PopoverMenu, PopoverMenuHandle,
    ScrollAxes, Scrollbars, Tab, Tooltip, WithScrollbar, prelude::*,
    utils::platform_title_bar_height,
};
use workspace::CloseWindow;
use zed_actions::agents_sidebar::FocusSidebarFilter;
use zed_actions::editor::{MoveDown, MoveUp};

use agent_settings::AgentSettings;

use crate::thread_metadata_store::{ThreadAttentionKind, ThreadId, ThreadMetadata};
use crate::{
    conversation_view::{
        PermissionControlsCallbacks, PermissionControlsParams, PermissionSelection,
        render_permission_controls,
    },
    threads_archive_view::format_history_entry_timestamp,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxAttentionKind {
    Permission,
    Completed,
    Error,
    Interrupted,
}

impl From<ThreadAttentionKind> for InboxAttentionKind {
    fn from(kind: ThreadAttentionKind) -> Self {
        match kind {
            ThreadAttentionKind::Completed => Self::Completed,
            ThreadAttentionKind::Error => Self::Error,
            ThreadAttentionKind::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum InboxFilter {
    All,
    Permission,
    Completed,
    Interrupted,
    Errors,
}

impl InboxFilter {
    fn all() -> [Self; 5] {
        [
            Self::All,
            Self::Permission,
            Self::Completed,
            Self::Interrupted,
            Self::Errors,
        ]
    }

    fn kinds() -> [Self; 4] {
        [
            Self::Permission,
            Self::Completed,
            Self::Interrupted,
            Self::Errors,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Permission => "Permission Requests",
            Self::Completed => "Completed",
            Self::Interrupted => "Interrupted",
            Self::Errors => "Errors",
        }
    }

    fn matches_kind(self, kind: InboxAttentionKind) -> bool {
        match self {
            Self::All => true,
            Self::Permission => kind == InboxAttentionKind::Permission,
            Self::Completed => kind == InboxAttentionKind::Completed,
            Self::Interrupted => kind == InboxAttentionKind::Interrupted,
            Self::Errors => kind == InboxAttentionKind::Error,
        }
    }
}

#[derive(Clone, Debug)]
pub struct InboxAttentionItem {
    pub thread: ThreadMetadata,
    pub kind: InboxAttentionKind,
    pub summary: Option<SharedString>,
    pub timestamp: DateTime<Utc>,
    pub context: Option<SharedString>,
    pub agent_name: Option<SharedString>,
    pub agent_icon: IconName,
    pub agent_icon_from_external_svg: Option<SharedString>,
    pub permission: Option<InboxPermissionItem>,
}

#[derive(Clone, Debug)]
pub struct InboxPermissionItem {
    pub session_id: acp::SessionId,
    pub tool_call_id: acp::ToolCallId,
    pub options: PermissionOptions,
    pub summary: Option<SharedString>,
}

pub enum ThreadsInboxViewEvent {
    Activate {
        thread: ThreadMetadata,
    },
    PermissionOutcome {
        session_id: acp::SessionId,
        tool_call_id: acp::ToolCallId,
        outcome: SelectedPermissionOutcome,
    },
}

impl EventEmitter<ThreadsInboxViewEvent> for ThreadsInboxView {}

pub struct ThreadsInboxView {
    focus_handle: FocusHandle,
    list_state: ListState,
    source_items: Vec<InboxAttentionItem>,
    items: Vec<InboxAttentionItem>,
    selection: Option<usize>,
    hovered_index: Option<usize>,
    expanded_thread_ids: HashSet<ThreadId>,
    permission_selections: HashMap<acp::ToolCallId, PermissionSelection>,
    permission_dropdown_handle: PopoverMenuHandle<ContextMenu>,
    filter_editor: Entity<Editor>,
    inbox_filters: HashSet<InboxFilter>,
    filter_menu_handle: PopoverMenuHandle<ContextMenu>,
    _subscriptions: Vec<Subscription>,
}

impl ThreadsInboxView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let filter_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Search inbox…", window, cx);
            editor
        });

        let filter_editor_subscription =
            cx.subscribe(&filter_editor, |this: &mut Self, _, event, cx| {
                if let editor::EditorEvent::BufferEdited = event {
                    this.update_items(cx);
                }
            });

        let filter_focus_handle = filter_editor.read(cx).focus_handle(cx);
        cx.on_focus_in(
            &filter_focus_handle,
            window,
            |this: &mut Self, _window, cx| {
                if this.selection.is_some() {
                    this.selection = None;
                    cx.notify();
                }
            },
        )
        .detach();

        cx.on_focus_out(&focus_handle, window, |this: &mut Self, _, _window, cx| {
            this.selection = None;
            cx.notify();
        })
        .detach();

        let mut this = Self {
            focus_handle,
            list_state: ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
            source_items: Vec::new(),
            items: Vec::new(),
            selection: None,
            hovered_index: None,
            expanded_thread_ids: HashSet::default(),
            permission_selections: HashMap::default(),
            permission_dropdown_handle: PopoverMenuHandle::default(),
            filter_editor,
            inbox_filters: HashSet::default(),
            filter_menu_handle: PopoverMenuHandle::default(),
            _subscriptions: vec![filter_editor_subscription],
        };
        this.update_items(cx);
        this
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn focus_filter_editor(&self, window: &mut Window, cx: &mut App) {
        self.filter_editor
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    }

    pub fn is_filter_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.filter_editor
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    pub fn set_items(&mut self, items: Vec<InboxAttentionItem>, cx: &mut Context<Self>) {
        self.source_items = items;
        self.update_items(cx);
    }

    fn update_items(&mut self, cx: &mut Context<Self>) {
        let query = self.filter_editor.read(cx).text(cx).to_lowercase();
        let mut items = self.source_items.clone();
        items.sort_by_key(|item| Reverse(item.timestamp));

        let saved_scroll = self.list_state.logical_scroll_top();

        self.items = items
            .into_iter()
            .filter(|item| inbox_filters_match(&self.inbox_filters, item.kind))
            .filter(|item| inbox_search_text(item).contains(&query))
            .collect();
        self.selection = self
            .selection
            .filter(|selection| *selection < self.items.len());
        self.hovered_index = self
            .hovered_index
            .filter(|hovered_index| *hovered_index < self.items.len());
        self.expanded_thread_ids.retain(|thread_id| {
            self.items.iter().any(|item| {
                item.kind == InboxAttentionKind::Permission && item.thread.thread_id == *thread_id
            })
        });
        self.permission_selections.retain(|tool_call_id, _| {
            self.items.iter().any(|item| {
                item.permission
                    .as_ref()
                    .is_some_and(|permission| permission.tool_call_id == *tool_call_id)
            })
        });
        self.list_state.reset(self.items.len());
        self.list_state.scroll_to(saved_scroll);
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        let next = self.selection.map_or(0, |selection| selection + 1);
        if next < self.items.len() {
            self.selection = Some(next);
            self.list_state.scroll_to_reveal_item(next);
            cx.notify();
        }
    }

    fn select_previous(&mut self, _: &SelectPrevious, window: &mut Window, cx: &mut Context<Self>) {
        match self.selection {
            Some(selection) if selection > 0 => {
                let previous = selection - 1;
                self.selection = Some(previous);
                self.list_state.scroll_to_reveal_item(previous);
                cx.notify();
            }
            Some(_) => {
                self.selection = None;
                self.focus_filter_editor(window, cx);
                cx.notify();
            }
            None if !self.items.is_empty() => {
                let previous = self.items.len() - 1;
                self.selection = Some(previous);
                self.list_state.scroll_to_reveal_item(previous);
                cx.notify();
            }
            None => {}
        }
    }

    fn editor_move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        self.select_next(&SelectNext, window, cx);
        if self.selection.is_some() {
            self.focus_handle.focus(window, cx);
        }
    }

    fn editor_move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        self.select_previous(&SelectPrevious, window, cx);
        if self.selection.is_some() {
            self.focus_handle.focus(window, cx);
        }
    }

    fn select_first(&mut self, _: &SelectFirst, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.items.is_empty() {
            self.selection = Some(0);
            self.list_state.scroll_to_reveal_item(0);
            cx.notify();
        }
    }

    fn select_last(&mut self, _: &SelectLast, _window: &mut Window, cx: &mut Context<Self>) {
        let last = self.items.len().checked_sub(1);
        if let Some(last) = last {
            self.selection = Some(last);
            self.list_state.scroll_to_reveal_item(last);
            cx.notify();
        }
    }

    fn confirm(&mut self, _: &Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection else {
            return;
        };
        self.activate_item_at(selection, cx);
    }

    fn select_child(&mut self, _: &SelectChild, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection else {
            return;
        };
        let Some(item) = self.items.get(selection) else {
            return;
        };
        if item.kind == InboxAttentionKind::Permission
            && self.expanded_thread_ids.insert(item.thread.thread_id)
        {
            cx.notify();
        }
    }

    fn select_parent(&mut self, _: &SelectParent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection else {
            return;
        };
        let Some(item) = self.items.get(selection) else {
            return;
        };
        if self.expanded_thread_ids.remove(&item.thread.thread_id) {
            cx.notify();
        }
    }

    fn toggle_permission_expanded(&mut self, index: usize, cx: &mut Context<Self>) {
        let item = self
            .items
            .get(index)
            .map(|item| (item.thread.thread_id, item.kind));
        if toggle_expanded_permission_thread_id(&mut self.expanded_thread_ids, item) {
            cx.notify();
        }
    }

    fn activate_item_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.items.get(index) {
            cx.emit(ThreadsInboxViewEvent::Activate {
                thread: item.thread.clone(),
            });
        }
    }

    fn emit_permission_outcome(
        &mut self,
        permission: InboxPermissionItem,
        outcome: SelectedPermissionOutcome,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ThreadsInboxViewEvent::PermissionOutcome {
            session_id: permission.session_id,
            tool_call_id: permission.tool_call_id,
            outcome,
        });
    }

    fn reset_filter_editor_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_editor.update(cx, |editor, cx| {
            editor.set_text("", window, cx);
        });
    }

    fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_query = !self.filter_editor.read(cx).text(cx).is_empty();
        let sidebar_on_left = matches!(
            AgentSettings::get_global(cx).sidebar_side(),
            settings::SidebarSide::Left
        );
        let sidebar_on_right = !sidebar_on_left;
        let not_fullscreen = !window.is_fullscreen();
        let traffic_lights = cfg!(target_os = "macos") && not_fullscreen && sidebar_on_left;
        let left_window_controls = !cfg!(target_os = "macos") && not_fullscreen && sidebar_on_left;
        let right_window_controls =
            !cfg!(target_os = "macos") && not_fullscreen && sidebar_on_right;
        let header_height = platform_title_bar_height(window);
        let show_focus_keybinding =
            self.selection.is_some() && !self.filter_editor.focus_handle(cx).is_focused(window);

        h_flex()
            .h(header_height)
            .mt_px()
            .pb_px()
            .when(left_window_controls, |this| {
                this.children(Self::render_left_window_controls(window, cx))
            })
            .map(|this| {
                if traffic_lights {
                    this.pl(px(ui::utils::TRAFFIC_LIGHT_PADDING))
                } else if !left_window_controls {
                    this.pl_1p5()
                } else {
                    this
                }
            })
            .when(!right_window_controls, |this| this.pr_1p5())
            .gap_1()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .when(traffic_lights, |this| {
                this.child(Divider::vertical().color(ui::DividerColor::Border))
            })
            .child(
                h_flex()
                    .ml_1()
                    .min_w_0()
                    .w_full()
                    .gap_1()
                    .child(
                        Icon::new(IconName::MagnifyingGlass)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(self.filter_editor.clone()),
            )
            .when(show_focus_keybinding, |this| {
                this.child(KeyBinding::for_action(&FocusSidebarFilter, cx))
            })
            .when(has_query, |this| {
                this.child(
                    IconButton::new("clear-filter", IconName::Close)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Clear Search"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reset_filter_editor_text(window, cx);
                            this.update_items(cx);
                        })),
                )
            })
            .when(right_window_controls, |this| {
                this.children(Self::render_right_window_controls(window, cx))
            })
    }

    fn render_left_window_controls(window: &Window, cx: &mut App) -> Option<AnyElement> {
        platform_title_bar::render_left_window_controls(
            cx.button_layout(),
            Box::new(CloseWindow),
            window,
        )
    }

    fn render_right_window_controls(window: &Window, cx: &mut App) -> Option<AnyElement> {
        platform_title_bar::render_right_window_controls(
            cx.button_layout(),
            Box::new(CloseWindow),
            window,
        )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let total_count = self.source_items.len();
        let visible_count = self.items.len();
        let is_filtered =
            !self.inbox_filters.is_empty() || !self.filter_editor.read(cx).text(cx).is_empty();
        let count_text = inbox_count_text(visible_count, total_count, is_filtered);

        h_flex()
            .mt_px()
            .pl_2p5()
            .pr_1p5()
            .h(Tab::content_height(cx))
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(Label::new("Inbox").size(LabelSize::Small))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Label::new(count_text)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(self.render_filter_menu(cx)),
            )
    }

    fn render_filter_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let selected_filters = self.inbox_filters.clone();

        PopoverMenu::new("inbox-filter-menu")
            .trigger_with_tooltip(
                IconButton::new("inbox-filter-menu-button", IconName::Sliders)
                    .icon_size(IconSize::Small)
                    .toggle_state(!selected_filters.is_empty()),
                Tooltip::text("Filter Inbox"),
            )
            .anchor(gpui::Anchor::TopRight)
            .offset(gpui::Point {
                x: px(0.0),
                y: px(2.0),
            })
            .with_handle(self.filter_menu_handle.clone())
            .menu(move |window, cx| {
                let this = this.clone();
                let selected_filters = selected_filters.clone();
                Some(ContextMenu::build(window, cx, move |menu, _window, _cx| {
                    InboxFilter::all().into_iter().fold(menu, |menu, filter| {
                        let is_selected = if filter == InboxFilter::All {
                            selected_filters.is_empty()
                        } else {
                            selected_filters.contains(&filter)
                        };
                        menu.toggleable_entry(
                            filter.label(),
                            is_selected,
                            IconPosition::Start,
                            None,
                            {
                                let this = this.clone();
                                move |_window, cx| {
                                    this.update(cx, |view, cx| {
                                        toggle_inbox_filter(&mut view.inbox_filters, filter);
                                        view.update_items(cx);
                                    })
                                    .ok();
                                }
                            },
                        )
                    })
                }))
            })
    }

    fn render_list_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(item) = self.items.get(index) else {
            return div().into_any_element();
        };
        let is_selected = self.selection == Some(index);
        let is_hovered = self.hovered_index == Some(index);
        let has_disclosure = item.kind == InboxAttentionKind::Permission;
        let is_expanded =
            has_disclosure && self.expanded_thread_ids.contains(&item.thread.thread_id);

        v_flex()
            .id(("inbox-entry", index))
            .w_full()
            .gap_1()
            .p_2()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .when(is_selected, |this| {
                this.bg(cx.theme().colors().element_selected)
            })
            .when(!is_selected && is_hovered, |this| {
                this.bg(cx.theme().colors().element_hover)
            })
            .on_hover(cx.listener(move |this, is_hovered: &bool, _window, cx| {
                let previously_hovered = this.hovered_index;
                this.hovered_index = if *is_hovered {
                    Some(index)
                } else {
                    previously_hovered.filter(|&hovered_index| hovered_index != index)
                };
                if this.hovered_index != previously_hovered {
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.activate_item_at(index, cx);
            }))
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(
                        Label::new(attention_reason(item.kind))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Label::new(format_history_entry_timestamp(item.timestamp))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .when(has_disclosure, |this| {
                                this.child(
                                    div()
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .child(
                                            Disclosure::new(
                                                ("inbox-entry-disclosure", index),
                                                is_expanded,
                                            )
                                            .on_click(
                                                cx.listener(move |this, _event, _window, cx| {
                                                    this.toggle_permission_expanded(index, cx);
                                                }),
                                            ),
                                        ),
                                )
                            }),
                    ),
            )
            .child(
                Label::new(item.thread.display_title())
                    .size(LabelSize::Small)
                    .truncate(),
            )
            .when_some(row_summary(item), |this, summary| {
                this.child(
                    Label::new(summary)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .min_w_0()
                    .child(
                        Icon::new(item.agent_icon)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .when_some(item.agent_name.clone(), |this, agent_name| {
                        this.child(
                            Label::new(agent_name)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        )
                    })
                    .when_some(item.context.clone(), |this, context| {
                        this.child(
                            Label::new(context)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        )
                    }),
            )
            .when(is_expanded, |this| {
                this.child(self.render_permission_controls(
                    item.permission.clone(),
                    index,
                    window,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn render_permission_controls(
        &self,
        permission: Option<InboxPermissionItem>,
        index: usize,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(permission) = permission else {
            return Label::new("Open thread to respond")
                .size(LabelSize::XSmall)
                .color(Color::Muted)
                .into_any_element();
        };

        let selection = self
            .permission_selections
            .get(&permission.tool_call_id)
            .cloned();
        render_permission_controls(
            PermissionControlsParams {
                session_id: permission.session_id.clone(),
                tool_call_id: permission.tool_call_id.clone(),
                options: permission.options,
                selection,
                entry_ix: index,
                is_first: false,
                show_key_bindings: false,
                focus_handle: self.focus_handle.clone(),
                dropdown_handle: self.permission_dropdown_handle.clone(),
                border_color: cx.theme().colors().border,
                callbacks: PermissionControlsCallbacks {
                    authorize: Rc::new(
                        |this: &mut Self, session_id, tool_call_id, outcome, _window, cx| {
                            let Some(permission) = this
                                .items
                                .iter()
                                .filter_map(|item| item.permission.clone())
                                .find(|permission| {
                                    permission.session_id == session_id
                                        && permission.tool_call_id == tool_call_id
                                })
                            else {
                                return;
                            };
                            this.emit_permission_outcome(permission, outcome, cx);
                        },
                    ),
                    set_selection: Rc::new(|this, tool_call_id, selection, cx| {
                        this.permission_selections.insert(tool_call_id, selection);
                        cx.notify();
                    }),
                    get_selection: Rc::new(|this, tool_call_id| {
                        this.permission_selections.get(tool_call_id).cloned()
                    }),
                },
            },
            cx,
        )
        .into_any_element()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn expand_selected_permission_row_for_test(&mut self, cx: &mut Context<Self>) {
        let index = self.selection.or_else(|| {
            self.items
                .iter()
                .position(|item| item.kind == InboxAttentionKind::Permission)
        });
        let Some(index) = index else {
            return;
        };
        self.selection = Some(index);
        let item = self
            .items
            .get(index)
            .map(|item| (item.thread.thread_id, item.kind));
        if let Some((thread_id, InboxAttentionKind::Permission)) = item {
            self.expanded_thread_ids.insert(thread_id);
            cx.notify();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn select_first_permission_option_for_test(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.selection else {
            return;
        };
        let Some(permission) = self
            .items
            .get(index)
            .and_then(|item| item.permission.clone())
        else {
            return;
        };
        let Some(outcome) = first_permission_outcome(&permission.options) else {
            return;
        };
        self.emit_permission_outcome(permission, outcome, cx);
    }
}

impl Focusable for ThreadsInboxView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ThreadsInboxView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_empty = self.items.is_empty();
        let has_query = !self.filter_editor.read(cx).text(cx).is_empty();

        let content = if is_empty {
            v_flex()
                .flex_1()
                .justify_center()
                .items_center()
                .gap_1()
                .child(
                    Label::new(if has_query {
                        "No inbox items match your search"
                    } else {
                        "All caught up"
                    })
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .when(!has_query, |this| {
                    this.child(
                        Label::new("No threads need attention")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                })
                .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .overflow_hidden()
                .child(
                    list(
                        self.list_state.clone(),
                        cx.processor(Self::render_list_entry),
                    )
                    .flex_1()
                    .size_full(),
                )
                .custom_scrollbars(
                    Scrollbars::new(ScrollAxes::Vertical).tracked_scroll_handle(&self.list_state),
                    window,
                    cx,
                )
                .into_any_element()
        };

        v_flex()
            .key_context("ThreadsInboxView")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::editor_move_down))
            .on_action(cx.listener(Self::editor_move_up))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::select_child))
            .on_action(cx.listener(Self::select_parent))
            .on_action(cx.listener(Self::confirm))
            .size_full()
            .child(self.render_header(window, cx))
            .child(self.render_toolbar(cx))
            .child(content)
    }
}

fn inbox_search_text(item: &InboxAttentionItem) -> String {
    let summary = item
        .summary
        .as_ref()
        .map(|summary| summary.as_ref())
        .unwrap_or_default();
    let permission_summary = item
        .permission
        .as_ref()
        .and_then(|permission| permission.summary.as_ref())
        .map(|summary| summary.as_ref())
        .unwrap_or_default();
    let context = item
        .context
        .as_ref()
        .map(|context| context.as_ref())
        .unwrap_or_default();
    let agent_name = item
        .agent_name
        .as_ref()
        .map(|agent_name| agent_name.as_ref())
        .unwrap_or_default();
    format!(
        "{} {} {summary} {permission_summary} {context} {agent_name}",
        item.thread.display_title(),
        attention_reason(item.kind)
    )
    .to_lowercase()
}

fn inbox_count_text(visible_count: usize, total_count: usize, is_filtered: bool) -> String {
    let total_count_text = if total_count == 1 {
        "1 item".to_string()
    } else {
        format!("{total_count} items")
    };
    if is_filtered {
        format!("{visible_count} of {total_count_text}")
    } else {
        total_count_text
    }
}

fn inbox_filters_match(filters: &HashSet<InboxFilter>, kind: InboxAttentionKind) -> bool {
    filters.is_empty() || filters.iter().any(|filter| filter.matches_kind(kind))
}

fn toggle_inbox_filter(filters: &mut HashSet<InboxFilter>, filter: InboxFilter) {
    if filter == InboxFilter::All {
        filters.clear();
        return;
    }

    if !filters.insert(filter) {
        filters.remove(&filter);
    }

    if InboxFilter::kinds()
        .into_iter()
        .all(|filter| filters.contains(&filter))
    {
        filters.clear();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn first_permission_outcome(options: &PermissionOptions) -> Option<SelectedPermissionOutcome> {
    crate::conversation_view::resolve_outcome_from_selection(options, None, true)
}

fn toggle_expanded_permission_thread_id(
    expanded_thread_ids: &mut HashSet<ThreadId>,
    item: Option<(ThreadId, InboxAttentionKind)>,
) -> bool {
    let Some((thread_id, kind)) = item else {
        return false;
    };
    if kind != InboxAttentionKind::Permission {
        return false;
    }
    if !expanded_thread_ids.insert(thread_id) {
        expanded_thread_ids.remove(&thread_id);
    }
    true
}

fn attention_reason(kind: InboxAttentionKind) -> &'static str {
    match kind {
        InboxAttentionKind::Permission => "Permission needed",
        InboxAttentionKind::Completed => "Completed in background",
        InboxAttentionKind::Error => "Stopped with error",
        InboxAttentionKind::Interrupted => "Interrupted",
    }
}

fn row_summary(item: &InboxAttentionItem) -> Option<SharedString> {
    match item.kind {
        InboxAttentionKind::Permission => item
            .permission
            .as_ref()
            .and_then(|permission| permission.summary.clone()),
        InboxAttentionKind::Interrupted => Some("Open to continue".into()),
        InboxAttentionKind::Error | InboxAttentionKind::Completed => item.summary.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::thread_metadata_store::WorktreePaths;
    use agent::ZED_AGENT_ID;
    use workspace::PathList;

    #[test]
    fn inbox_filter_matches_expected_attention_kinds() {
        let mut filters = HashSet::default();
        assert!(inbox_filters_match(
            &filters,
            InboxAttentionKind::Permission
        ));

        toggle_inbox_filter(&mut filters, InboxFilter::Permission);
        toggle_inbox_filter(&mut filters, InboxFilter::Errors);
        assert!(inbox_filters_match(
            &filters,
            InboxAttentionKind::Permission
        ));
        assert!(inbox_filters_match(&filters, InboxAttentionKind::Error));
        assert!(!inbox_filters_match(
            &filters,
            InboxAttentionKind::Completed
        ));

        toggle_inbox_filter(&mut filters, InboxFilter::Permission);
        assert!(!inbox_filters_match(
            &filters,
            InboxAttentionKind::Permission
        ));
        assert!(inbox_filters_match(&filters, InboxAttentionKind::Error));

        toggle_inbox_filter(&mut filters, InboxFilter::All);
        assert!(filters.is_empty());
        assert!(inbox_filters_match(&filters, InboxAttentionKind::Completed));
    }

    #[test]
    fn selecting_every_inbox_kind_returns_to_all_filter() {
        let mut filters = HashSet::default();
        for filter in InboxFilter::kinds() {
            toggle_inbox_filter(&mut filters, filter);
        }
        assert!(filters.is_empty());
    }

    #[test]
    fn toggles_expansion_only_for_permission_items() {
        let permission_item = inbox_item(InboxAttentionKind::Permission);
        let completed_item = inbox_item(InboxAttentionKind::Completed);
        let mut expanded_thread_ids = HashSet::default();

        assert!(toggle_expanded_permission_thread_id(
            &mut expanded_thread_ids,
            Some((permission_item.thread.thread_id, permission_item.kind))
        ));
        assert!(expanded_thread_ids.contains(&permission_item.thread.thread_id));

        assert!(toggle_expanded_permission_thread_id(
            &mut expanded_thread_ids,
            Some((permission_item.thread.thread_id, permission_item.kind))
        ));
        assert!(!expanded_thread_ids.contains(&permission_item.thread.thread_id));

        assert!(!toggle_expanded_permission_thread_id(
            &mut expanded_thread_ids,
            Some((completed_item.thread.thread_id, completed_item.kind))
        ));
        assert!(expanded_thread_ids.is_empty());
    }

    #[test]
    fn inbox_search_matches_context_agent_and_permission_summary() {
        let mut item = inbox_item(InboxAttentionKind::Permission);
        item.context = Some("neo-zed".into());
        item.agent_name = Some("Neozed Agent".into());
        item.permission = Some(InboxPermissionItem {
            session_id: acp::SessionId::new("session-search"),
            tool_call_id: acp::ToolCallId::new("tool-search"),
            options: PermissionOptions::Flat(vec![acp::PermissionOption::new(
                acp::PermissionOptionId::new("allow-search"),
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            )]),
            summary: Some("Edit src/main.rs".into()),
        });

        let search_text = inbox_search_text(&item);
        assert!(search_text.contains("permission needed"));
        assert!(search_text.contains("neo-zed"));
        assert!(search_text.contains("neozed agent"));
        assert!(search_text.contains("edit src/main.rs"));
    }

    #[test]
    fn inbox_count_text_mentions_visible_and_total_when_filtered() {
        assert_eq!(inbox_count_text(3, 3, false), "3 items");
        assert_eq!(inbox_count_text(1, 3, true), "1 of 3 items");
        assert_eq!(inbox_count_text(1, 1, true), "1 of 1 item");
    }

    #[test]
    fn inbox_reason_copy_matches_attention_spec() {
        assert_eq!(
            attention_reason(InboxAttentionKind::Permission),
            "Permission needed"
        );
        assert_eq!(
            attention_reason(InboxAttentionKind::Interrupted),
            "Interrupted"
        );
        let item = inbox_item(InboxAttentionKind::Interrupted);
        assert_eq!(row_summary(&item).as_deref(), Some("Open to continue"));
    }

    fn inbox_item(kind: InboxAttentionKind) -> InboxAttentionItem {
        InboxAttentionItem {
            thread: ThreadMetadata {
                thread_id: crate::thread_metadata_store::ThreadId::new(),
                session_id: None,
                agent_id: ZED_AGENT_ID.clone(),
                title: Some("Test Thread".into()),
                updated_at: Utc::now(),
                created_at: None,
                interacted_at: None,
                worktree_paths: WorktreePaths::from_folder_paths(&PathList::default()),
                remote_connection: None,
                attention: None,
                last_known_status: None,
                archived: false,
            },
            kind,
            summary: None,
            timestamp: Utc::now(),
            context: None,
            agent_name: None,
            agent_icon: IconName::ZedAgent,
            agent_icon_from_external_svg: None,
            permission: None,
        }
    }
}
