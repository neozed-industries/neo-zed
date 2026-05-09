use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use editor::Editor;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, ListState, Render,
    SharedString, Subscription, Window, list, prelude::*, px,
};
use menu::{Confirm, SelectFirst, SelectLast, SelectNext, SelectPrevious};
use settings::Settings as _;
use theme::ActiveTheme;
use ui::{
    ContextMenu, Divider, KeyBinding, Label, LabelSize, PopoverMenu, PopoverMenuHandle, ScrollAxes,
    Scrollbars, Tab, Tooltip, WithScrollbar, prelude::*, utils::platform_title_bar_height,
};
use workspace::CloseWindow;
use zed_actions::agents_sidebar::FocusSidebarFilter;
use zed_actions::editor::{MoveDown, MoveUp};

use agent_settings::AgentSettings;

use crate::thread_metadata_store::{ThreadAttentionKind, ThreadMetadata, ThreadMetadataStore};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Permission => "Permission Requests",
            Self::Completed => "Completed",
            Self::Interrupted => "Interrupted",
            Self::Errors => "Errors",
        }
    }

    fn matches(self, kind: InboxAttentionKind) -> bool {
        match self {
            Self::All => true,
            Self::Permission => kind == InboxAttentionKind::Permission,
            Self::Completed => kind == InboxAttentionKind::Completed,
            Self::Interrupted => kind == InboxAttentionKind::Interrupted,
            Self::Errors => kind == InboxAttentionKind::Error,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InboxAttentionItem {
    pub thread: ThreadMetadata,
    pub kind: InboxAttentionKind,
    pub summary: Option<SharedString>,
    pub timestamp: DateTime<Utc>,
}

pub enum ThreadsInboxViewEvent {
    Close,
    RefreshItems,
    Activate { thread: ThreadMetadata },
}

impl EventEmitter<ThreadsInboxViewEvent> for ThreadsInboxView {}

pub struct ThreadsInboxView {
    focus_handle: FocusHandle,
    list_state: ListState,
    source_items: Vec<InboxAttentionItem>,
    items: Vec<InboxAttentionItem>,
    selection: Option<usize>,
    hovered_index: Option<usize>,
    filter_editor: Entity<Editor>,
    inbox_filter: InboxFilter,
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

        let thread_metadata_store_subscription = cx.observe(
            &ThreadMetadataStore::global(cx),
            |_this: &mut Self, _, cx| {
                cx.emit(ThreadsInboxViewEvent::RefreshItems);
            },
        );

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
            filter_editor,
            inbox_filter: InboxFilter::All,
            filter_menu_handle: PopoverMenuHandle::default(),
            _subscriptions: vec![
                filter_editor_subscription,
                thread_metadata_store_subscription,
            ],
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
            .filter(|item| self.inbox_filter.matches(item.kind))
            .filter(|item| inbox_search_text(item).contains(&query))
            .collect();
        self.selection = self
            .selection
            .filter(|selection| *selection < self.items.len());
        self.hovered_index = self
            .hovered_index
            .filter(|hovered_index| *hovered_index < self.items.len());
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

    fn activate_item_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.items.get(index) {
            cx.emit(ThreadsInboxViewEvent::Activate {
                thread: item.thread.clone(),
            });
        }
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
        let total_count_text = if total_count == 1 {
            "1 item".to_string()
        } else {
            format!("{total_count} items")
        };
        let count_text = if self.inbox_filter == InboxFilter::All {
            total_count_text
        } else {
            format!("{visible_count} of {total_count_text}")
        };

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
        let selected_filter = self.inbox_filter;

        PopoverMenu::new("inbox-filter-menu")
            .trigger_with_tooltip(
                IconButton::new("inbox-filter-menu-button", IconName::Sliders)
                    .icon_size(IconSize::Small)
                    .toggle_state(selected_filter != InboxFilter::All),
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
                Some(ContextMenu::build(window, cx, move |menu, _window, _cx| {
                    InboxFilter::all().into_iter().fold(menu, |menu, filter| {
                        menu.toggleable_entry(
                            filter.label(),
                            selected_filter == filter,
                            IconPosition::Start,
                            None,
                            {
                                let this = this.clone();
                                move |_window, cx| {
                                    this.update(cx, |view, cx| {
                                        view.inbox_filter = filter;
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(item) = self.items.get(index) else {
            return div().into_any_element();
        };
        let is_selected = self.selection == Some(index);
        let is_hovered = self.hovered_index == Some(index);

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
                        Label::new(format_inbox_timestamp(item.timestamp))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                Label::new(item.thread.display_title())
                    .size(LabelSize::Small)
                    .truncate(),
            )
            .when_some(item.summary.clone(), |this, summary| {
                this.child(
                    Label::new(summary)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                )
            })
            .into_any_element()
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
            let message = if has_query {
                "No inbox items match your search"
            } else {
                "All caught up"
            };
            v_flex()
                .flex_1()
                .justify_center()
                .items_center()
                .child(
                    Label::new(message)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
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
            .on_action(cx.listener(Self::confirm))
            .size_full()
            .child(self.render_header(window, cx))
            .when(!has_query, |this| this.child(self.render_toolbar(cx)))
            .child(content)
    }
}

fn inbox_search_text(item: &InboxAttentionItem) -> String {
    let summary = item
        .summary
        .as_ref()
        .map(|summary| summary.as_ref())
        .unwrap_or_default();
    format!(
        "{} {} {summary}",
        item.thread.display_title(),
        attention_reason(item.kind)
    )
    .to_lowercase()
}

fn attention_reason(kind: InboxAttentionKind) -> &'static str {
    match kind {
        InboxAttentionKind::Permission => "Waiting for permission",
        InboxAttentionKind::Completed => "Completed in background",
        InboxAttentionKind::Error => "Stopped with error",
        InboxAttentionKind::Interrupted => "Interrupted",
    }
}

fn format_inbox_timestamp(timestamp: chrono::DateTime<chrono::Utc>) -> SharedString {
    timestamp.format("%b %-d, %-I:%M %p").to_string().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_filter_matches_expected_attention_kinds() {
        assert!(InboxFilter::All.matches(InboxAttentionKind::Permission));
        assert!(InboxFilter::Permission.matches(InboxAttentionKind::Permission));
        assert!(!InboxFilter::Permission.matches(InboxAttentionKind::Completed));
        assert!(InboxFilter::Completed.matches(InboxAttentionKind::Completed));
        assert!(InboxFilter::Interrupted.matches(InboxAttentionKind::Interrupted));
        assert!(InboxFilter::Errors.matches(InboxAttentionKind::Error));
        assert!(!InboxFilter::Errors.matches(InboxAttentionKind::Interrupted));
    }
}
