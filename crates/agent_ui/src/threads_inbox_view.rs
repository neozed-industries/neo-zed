use std::cmp::Reverse;

use editor::Editor;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, ListState, Render,
    SharedString, Subscription, Window, list, prelude::*, px,
};
use menu::{Confirm, SelectNext, SelectPrevious};
use theme::ActiveTheme;
use ui::{Label, LabelSize, ScrollAxes, Scrollbars, WithScrollbar, prelude::*};
use zed_actions::editor::{MoveDown, MoveUp};

use crate::thread_metadata_store::{ThreadAttentionKind, ThreadMetadata, ThreadMetadataStore};

#[derive(Clone)]
enum InboxListItem {
    Entry { thread: ThreadMetadata },
}

pub enum ThreadsInboxViewEvent {
    Close,
    Activate { thread: ThreadMetadata },
}

impl EventEmitter<ThreadsInboxViewEvent> for ThreadsInboxView {}

pub struct ThreadsInboxView {
    focus_handle: FocusHandle,
    list_state: ListState,
    items: Vec<InboxListItem>,
    selection: Option<usize>,
    filter_editor: Entity<Editor>,
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
            |this: &mut Self, _, cx| {
                this.update_items(cx);
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
            items: Vec::new(),
            selection: None,
            filter_editor,
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

    fn update_items(&mut self, cx: &mut Context<Self>) {
        let query = self.filter_editor.read(cx).text(cx).to_lowercase();
        let mut threads = ThreadMetadataStore::global(cx)
            .read(cx)
            .attention_entries()
            .cloned()
            .collect::<Vec<_>>();
        threads
            .sort_by_key(|thread| Reverse(thread.attention.as_ref().map(|attention| attention.at)));

        self.items = threads
            .into_iter()
            .filter(|thread| inbox_search_text(thread).contains(&query))
            .map(|thread| InboxListItem::Entry { thread })
            .collect();
        self.selection = self
            .selection
            .filter(|selection| *selection < self.items.len());
        self.list_state
            .splice(0..self.list_state.item_count(), self.items.len());
        cx.notify();
    }

    pub fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
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

    pub fn confirm(&mut self, _: &Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection else {
            return;
        };
        let Some(InboxListItem::Entry { thread }) = self.items.get(selection) else {
            return;
        };
        cx.emit(ThreadsInboxViewEvent::Activate {
            thread: thread.clone(),
        });
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let item_count = ThreadMetadataStore::global(cx)
            .read(cx)
            .attention_entries()
            .count();
        let count_text = if item_count == 1 {
            "1 item".to_string()
        } else {
            format!("{item_count} items")
        };

        v_flex()
            .gap_2()
            .p_2()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                h_flex()
                    .justify_between()
                    .child(Label::new("Inbox").size(LabelSize::Small))
                    .child(
                        Label::new(count_text)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(self.filter_editor.clone())
    }

    fn render_list_entry(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(InboxListItem::Entry { thread }) = self.items.get(index) else {
            return div().into_any_element();
        };
        let is_selected = self.selection == Some(index);
        let Some(attention) = thread.attention.as_ref() else {
            return div().into_any_element();
        };

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
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(
                        Label::new(attention_reason(attention.kind))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(format_history_entry_timestamp(attention.at))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                Label::new(thread.display_title())
                    .size(LabelSize::Small)
                    .truncate(),
            )
            .when_some(attention.summary.clone(), |this, summary| {
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
            .on_action(cx.listener(Self::confirm))
            .size_full()
            .child(self.render_header(cx))
            .child(content)
    }
}

fn inbox_search_text(thread: &ThreadMetadata) -> String {
    let reason = thread
        .attention
        .as_ref()
        .map(|attention| attention_reason(attention.kind))
        .unwrap_or_default();
    let summary = thread
        .attention
        .as_ref()
        .and_then(|attention| attention.summary.as_ref())
        .map(|summary| summary.as_ref())
        .unwrap_or_default();
    format!("{} {reason} {summary}", thread.display_title()).to_lowercase()
}

fn attention_reason(kind: ThreadAttentionKind) -> &'static str {
    match kind {
        ThreadAttentionKind::Completed => "Completed in background",
        ThreadAttentionKind::Error => "Stopped with error",
        ThreadAttentionKind::Interrupted => "Interrupted",
    }
}

fn format_history_entry_timestamp(timestamp: chrono::DateTime<chrono::Utc>) -> SharedString {
    timestamp.format("%b %-d, %-I:%M %p").to_string().into()
}
