use std::rc::Rc;

use acp_thread::{
    PermissionOptionChoice, PermissionOptions, PermissionPattern, SelectedPermissionOutcome,
};
use agent_client_protocol::schema as acp;
use gpui::{Action, AnyElement, Context, Div, FocusHandle, Hsla, SharedString, Window};
use ui::{
    Button, ButtonStyle, Color, ContextMenu, Icon, IconName, IconPosition, IconSize, KeyBinding,
    LabelSize, PopoverMenu, PopoverMenuHandle, prelude::*, rems_from_px,
};
use util::ResultExt;

use crate::{AllowAlways, AllowOnce, OpenPermissionDropdown, RejectOnce};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PermissionSelection {
    Choice(usize),
    SelectedPatterns(Vec<usize>),
}

impl PermissionSelection {
    pub(crate) fn choice_index(&self) -> Option<usize> {
        match self {
            Self::Choice(index) => Some(*index),
            Self::SelectedPatterns(_) => None,
        }
    }

    pub(crate) fn is_pattern_checked(&self, index: usize) -> bool {
        match self {
            Self::SelectedPatterns(checked) => checked.contains(&index),
            _ => false,
        }
    }

    pub(crate) fn has_any_checked_patterns(&self) -> bool {
        match self {
            Self::SelectedPatterns(checked) => !checked.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn toggle_pattern(&mut self, index: usize) {
        if let Self::SelectedPatterns(checked) = self {
            if let Some(position) = checked
                .iter()
                .position(|&checked_index| checked_index == index)
            {
                checked.swap_remove(position);
            } else {
                checked.push(index);
            }
        }
    }
}

type AuthorizePermission<T> = Rc<
    dyn Fn(
        &mut T,
        acp::SessionId,
        acp::ToolCallId,
        SelectedPermissionOutcome,
        &mut Window,
        &mut Context<T>,
    ),
>;
type SetPermissionSelection<T> =
    Rc<dyn Fn(&mut T, acp::ToolCallId, PermissionSelection, &mut Context<T>)>;
type GetPermissionSelection<T> = Rc<dyn Fn(&T, &acp::ToolCallId) -> Option<PermissionSelection>>;

pub(crate) struct PermissionControlsCallbacks<T: 'static> {
    pub(crate) authorize: AuthorizePermission<T>,
    pub(crate) set_selection: SetPermissionSelection<T>,
    pub(crate) get_selection: GetPermissionSelection<T>,
}

impl<T: 'static> Clone for PermissionControlsCallbacks<T> {
    fn clone(&self) -> Self {
        Self {
            authorize: self.authorize.clone(),
            set_selection: self.set_selection.clone(),
            get_selection: self.get_selection.clone(),
        }
    }
}

pub(crate) struct PermissionControlsParams<T: 'static> {
    pub(crate) session_id: acp::SessionId,
    pub(crate) tool_call_id: acp::ToolCallId,
    pub(crate) options: PermissionOptions,
    pub(crate) selection: Option<PermissionSelection>,
    pub(crate) entry_ix: usize,
    pub(crate) is_first: bool,
    pub(crate) show_key_bindings: bool,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) dropdown_handle: PopoverMenuHandle<ContextMenu>,
    pub(crate) border_color: Hsla,
    pub(crate) callbacks: PermissionControlsCallbacks<T>,
}

pub(crate) fn render_permission_controls<T: 'static>(
    params: PermissionControlsParams<T>,
    cx: &Context<T>,
) -> Div {
    let options = params.options.clone();
    match &options {
        PermissionOptions::Flat(options) => render_permission_buttons_flat(params, options, cx),
        PermissionOptions::Dropdown(choices) => {
            render_permission_buttons_with_dropdown(params, choices, None, cx)
        }
        PermissionOptions::DropdownWithPatterns {
            choices,
            patterns,
            tool_name,
        } => render_permission_buttons_with_dropdown(
            params,
            choices,
            Some((patterns, tool_name)),
            cx,
        ),
    }
}

pub(crate) fn resolve_outcome_from_selection(
    options: &PermissionOptions,
    selection: Option<&PermissionSelection>,
    is_allow: bool,
) -> Option<SelectedPermissionOutcome> {
    let choices = match options {
        PermissionOptions::Dropdown(choices) => choices.as_slice(),
        PermissionOptions::DropdownWithPatterns { choices, .. } => choices.as_slice(),
        PermissionOptions::Flat(_) => {
            let kind = if is_allow {
                acp::PermissionOptionKind::AllowOnce
            } else {
                acp::PermissionOptionKind::RejectOnce
            };
            let option = options.first_option_of_kind(kind)?;
            return Some(SelectedPermissionOutcome::new(
                option.option_id.clone(),
                option.kind,
            ));
        }
    };

    if let Some(PermissionSelection::SelectedPatterns(checked)) = selection {
        if let Some(outcome) = options.build_outcome_for_checked_patterns(checked, is_allow) {
            return Some(outcome);
        }
    }

    let selected_index = selection
        .and_then(|selection| selection.choice_index())
        .unwrap_or_else(|| choices.len().saturating_sub(1));
    choices
        .get(selected_index)
        .or_else(|| choices.last())
        .map(|choice| choice.build_outcome(is_allow))
}

fn render_permission_buttons_with_dropdown<T: 'static>(
    params: PermissionControlsParams<T>,
    choices: &[PermissionOptionChoice],
    patterns: Option<(&[PermissionPattern], &str)>,
    cx: &Context<T>,
) -> Div {
    let selection = params.selection.clone();
    let selected_index = selection
        .as_ref()
        .and_then(|selection| selection.choice_index())
        .unwrap_or_else(|| choices.len().saturating_sub(1));

    let dropdown_label: SharedString =
        if matches!(selection, Some(PermissionSelection::SelectedPatterns(_))) {
            "Always for selected commands".into()
        } else {
            choices
                .get(selected_index)
                .or_else(|| choices.last())
                .map(|choice| choice.label())
                .unwrap_or_else(|| "Only this time".into())
        };

    let dropdown = if let Some((pattern_list, tool_name)) = patterns {
        render_permission_granularity_dropdown_with_patterns(
            &params,
            choices,
            pattern_list,
            tool_name,
            dropdown_label,
            cx,
        )
    } else {
        render_permission_granularity_dropdown(&params, choices, dropdown_label, selected_index, cx)
    };

    h_flex()
        .w_full()
        .p_1()
        .gap_2()
        .justify_between()
        .border_t_1()
        .border_color(params.border_color)
        .child(
            h_flex()
                .gap_0p5()
                .child(
                    Button::new(("allow-btn", params.entry_ix), "Allow")
                        .start_icon(
                            Icon::new(IconName::Check)
                                .size(IconSize::XSmall)
                                .color(Color::Success),
                        )
                        .label_size(LabelSize::Small)
                        .when(params.is_first && params.show_key_bindings, |this| {
                            this.key_binding(
                                KeyBinding::for_action_in(
                                    &AllowOnce as &dyn Action,
                                    &params.focus_handle,
                                    cx,
                                )
                                .map(|key_binding| key_binding.size(rems_from_px(12.))),
                            )
                        })
                        .on_click(cx.listener({
                            let authorize = params.callbacks.authorize.clone();
                            let session_id = params.session_id.clone();
                            let tool_call_id = params.tool_call_id.clone();
                            let options = params.options.clone();
                            let selection = params.selection.clone();
                            move |this, _, window, cx| {
                                let Some(outcome) = resolve_outcome_from_selection(
                                    &options,
                                    selection.as_ref(),
                                    true,
                                ) else {
                                    return;
                                };
                                authorize(
                                    this,
                                    session_id.clone(),
                                    tool_call_id.clone(),
                                    outcome,
                                    window,
                                    cx,
                                );
                            }
                        })),
                )
                .child(
                    Button::new(("deny-btn", params.entry_ix), "Deny")
                        .start_icon(
                            Icon::new(IconName::Close)
                                .size(IconSize::XSmall)
                                .color(Color::Error),
                        )
                        .label_size(LabelSize::Small)
                        .when(params.is_first && params.show_key_bindings, |this| {
                            this.key_binding(
                                KeyBinding::for_action_in(
                                    &RejectOnce as &dyn Action,
                                    &params.focus_handle,
                                    cx,
                                )
                                .map(|key_binding| key_binding.size(rems_from_px(12.))),
                            )
                        })
                        .on_click(cx.listener({
                            let authorize = params.callbacks.authorize.clone();
                            let session_id = params.session_id.clone();
                            let tool_call_id = params.tool_call_id.clone();
                            let options = params.options.clone();
                            let selection = params.selection.clone();
                            move |this, _, window, cx| {
                                let Some(outcome) = resolve_outcome_from_selection(
                                    &options,
                                    selection.as_ref(),
                                    false,
                                ) else {
                                    return;
                                };
                                authorize(
                                    this,
                                    session_id.clone(),
                                    tool_call_id.clone(),
                                    outcome,
                                    window,
                                    cx,
                                );
                            }
                        })),
                ),
        )
        .child(dropdown)
}

fn render_permission_granularity_dropdown<T: 'static>(
    params: &PermissionControlsParams<T>,
    choices: &[PermissionOptionChoice],
    current_label: SharedString,
    selected_index: usize,
    cx: &Context<T>,
) -> AnyElement {
    let menu_options: Vec<(usize, SharedString)> = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| (index, choice.label()))
        .collect();
    let view = cx.entity().downgrade();
    let callbacks = params.callbacks.clone();
    let tool_call_id = params.tool_call_id.clone();

    PopoverMenu::new(("permission-granularity", params.entry_ix))
        .with_handle(params.dropdown_handle.clone())
        .trigger(
            Button::new(("granularity-trigger", params.entry_ix), current_label)
                .end_icon(
                    Icon::new(IconName::ChevronDown)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                )
                .label_size(LabelSize::Small)
                .when(params.is_first && params.show_key_bindings, |this| {
                    this.key_binding(
                        KeyBinding::for_action_in(
                            &OpenPermissionDropdown as &dyn Action,
                            &params.focus_handle,
                            cx,
                        )
                        .map(|key_binding| key_binding.size(rems_from_px(12.))),
                    )
                }),
        )
        .menu(move |window, cx| {
            let tool_call_id = tool_call_id.clone();
            let menu_options = menu_options.clone();
            let callbacks = callbacks.clone();
            let view = view.clone();

            Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                for (index, display_name) in menu_options.iter() {
                    let display_name = display_name.clone();
                    let index = *index;
                    let tool_call_id = tool_call_id.clone();
                    let is_selected = index == selected_index;
                    let view = view.clone();
                    let set_selection = callbacks.set_selection.clone();
                    menu = menu.toggleable_entry(
                        display_name,
                        is_selected,
                        IconPosition::End,
                        None,
                        move |_window, cx| {
                            view.update(cx, |this, cx| {
                                set_selection(
                                    this,
                                    tool_call_id.clone(),
                                    PermissionSelection::Choice(index),
                                    cx,
                                );
                                cx.notify();
                            })
                            .log_err();
                        },
                    );
                }

                menu
            }))
        })
        .into_any_element()
}

fn render_permission_granularity_dropdown_with_patterns<T: 'static>(
    params: &PermissionControlsParams<T>,
    choices: &[PermissionOptionChoice],
    patterns: &[PermissionPattern],
    _tool_name: &str,
    current_label: SharedString,
    cx: &Context<T>,
) -> AnyElement {
    let default_choice_index = choices.len().saturating_sub(1);
    let menu_options: Vec<(usize, SharedString)> = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| (index, choice.label()))
        .collect();
    let pattern_options: Vec<(usize, SharedString)> = patterns
        .iter()
        .enumerate()
        .map(|(index, pattern)| {
            (
                index,
                SharedString::from(format!("Always for `{}` commands", pattern.display_name)),
            )
        })
        .collect();
    let pattern_count = patterns.len();
    let view = cx.entity().downgrade();
    let callbacks = params.callbacks.clone();
    let tool_call_id = params.tool_call_id.clone();
    let dropdown_handle = params.dropdown_handle.clone();

    PopoverMenu::new(("permission-granularity", params.entry_ix))
        .with_handle(dropdown_handle.clone())
        .anchor(gpui::Anchor::TopRight)
        .attach(gpui::Anchor::BottomRight)
        .trigger(
            Button::new(("granularity-trigger", params.entry_ix), current_label)
                .end_icon(
                    Icon::new(IconName::ChevronDown)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                )
                .label_size(LabelSize::Small)
                .when(params.is_first && params.show_key_bindings, |this| {
                    this.key_binding(
                        KeyBinding::for_action_in(
                            &OpenPermissionDropdown as &dyn Action,
                            &params.focus_handle,
                            cx,
                        )
                        .map(|key_binding| key_binding.size(rems_from_px(12.))),
                    )
                }),
        )
        .menu(move |window, cx| {
            let tool_call_id = tool_call_id.clone();
            let menu_options = menu_options.clone();
            let pattern_options = pattern_options.clone();
            let view = view.clone();
            let callbacks = callbacks.clone();
            let dropdown_handle = dropdown_handle.clone();

            Some(ContextMenu::build_persistent(
                window,
                cx,
                move |mut menu, _window, cx| {
                    let selection: Option<PermissionSelection> = view.upgrade().and_then(|view| {
                        let view = view.read(cx);
                        (callbacks.get_selection)(&view, &tool_call_id)
                    });
                    let is_pattern_mode =
                        matches!(selection, Some(PermissionSelection::SelectedPatterns(_)));

                    for (index, display_name) in menu_options.iter() {
                        let display_name = display_name.clone();
                        let index = *index;
                        let tool_call_id = tool_call_id.clone();
                        let is_selected = !is_pattern_mode
                            && selection
                                .as_ref()
                                .and_then(|selection| selection.choice_index())
                                .map_or(index == default_choice_index, |choice_index| {
                                    choice_index == index
                                });
                        let view = view.clone();
                        let set_selection = callbacks.set_selection.clone();
                        menu = menu.toggleable_entry(
                            display_name,
                            is_selected,
                            IconPosition::End,
                            None,
                            move |_window, cx| {
                                view.update(cx, |this, cx| {
                                    set_selection(
                                        this,
                                        tool_call_id.clone(),
                                        PermissionSelection::Choice(index),
                                        cx,
                                    );
                                    cx.notify();
                                })
                                .log_err();
                            },
                        );
                    }

                    menu = menu.separator().header("Select Options…");

                    for (pattern_index, label) in pattern_options.iter() {
                        let label = label.clone();
                        let pattern_index = *pattern_index;
                        let tool_call_id = tool_call_id.clone();
                        let is_checked = selection
                            .as_ref()
                            .is_some_and(|selection| selection.is_pattern_checked(pattern_index));
                        let view = view.clone();
                        let callbacks = callbacks.clone();
                        menu = menu.toggleable_entry(
                            label,
                            is_checked,
                            IconPosition::End,
                            None,
                            move |_window, cx| {
                                let callbacks = callbacks.clone();
                                view.update(cx, |this, cx| {
                                    let mut selection =
                                        (callbacks.get_selection)(this, &tool_call_id)
                                            .unwrap_or_else(|| {
                                                PermissionSelection::SelectedPatterns(
                                                    (0..pattern_count).collect(),
                                                )
                                            });
                                    match &mut selection {
                                        PermissionSelection::SelectedPatterns(_) => {
                                            selection.toggle_pattern(pattern_index);
                                        }
                                        PermissionSelection::Choice(_) => {
                                            selection = PermissionSelection::SelectedPatterns(
                                                (0..pattern_count).collect(),
                                            );
                                        }
                                    }
                                    (callbacks.set_selection)(
                                        this,
                                        tool_call_id.clone(),
                                        selection,
                                        cx,
                                    );
                                    cx.notify();
                                })
                                .log_err();
                            },
                        );
                    }

                    let any_patterns_checked = selection
                        .as_ref()
                        .is_some_and(|selection| selection.has_any_checked_patterns());
                    let dropdown_handle = dropdown_handle.clone();
                    menu.custom_row(move |_window, _cx| {
                        div()
                            .py_1()
                            .w_full()
                            .child(
                                Button::new("apply-patterns", "Apply")
                                    .full_width()
                                    .style(ButtonStyle::Outlined)
                                    .label_size(LabelSize::Small)
                                    .disabled(!any_patterns_checked)
                                    .on_click({
                                        let dropdown_handle = dropdown_handle.clone();
                                        move |_event, _window, cx| {
                                            dropdown_handle.hide(cx);
                                        }
                                    }),
                            )
                            .into_any_element()
                    })
                },
            ))
        })
        .into_any_element()
}

fn render_permission_buttons_flat<T: 'static>(
    params: PermissionControlsParams<T>,
    options: &[acp::PermissionOption],
    cx: &Context<T>,
) -> Div {
    let mut seen_kinds: Vec<acp::PermissionOptionKind> = Vec::new();

    div()
        .p_1()
        .border_t_1()
        .border_color(params.border_color)
        .w_full()
        .v_flex()
        .gap_0p5()
        .children(options.iter().map(move |option| {
            let option_id = SharedString::from(option.option_id.0.clone());
            let option_kind = option.kind;
            let option_id_for_outcome = option.option_id.clone();
            let session_id = params.session_id.clone();
            let tool_call_id = params.tool_call_id.clone();
            let authorize = params.callbacks.authorize.clone();
            Button::new((option_id, params.entry_ix), option.name.clone())
                .map(|this| {
                    let (icon, action) = match option_kind {
                        acp::PermissionOptionKind::AllowOnce => (
                            Icon::new(IconName::Check)
                                .size(IconSize::XSmall)
                                .color(Color::Success),
                            Some(&AllowOnce as &dyn Action),
                        ),
                        acp::PermissionOptionKind::AllowAlways => (
                            Icon::new(IconName::CheckDouble)
                                .size(IconSize::XSmall)
                                .color(Color::Success),
                            Some(&AllowAlways as &dyn Action),
                        ),
                        acp::PermissionOptionKind::RejectOnce => (
                            Icon::new(IconName::Close)
                                .size(IconSize::XSmall)
                                .color(Color::Error),
                            Some(&RejectOnce as &dyn Action),
                        ),
                        acp::PermissionOptionKind::RejectAlways | _ => (
                            Icon::new(IconName::Close)
                                .size(IconSize::XSmall)
                                .color(Color::Error),
                            None,
                        ),
                    };

                    let this = this.start_icon(icon);
                    let Some(action) = action else {
                        return this;
                    };
                    if !params.is_first
                        || !params.show_key_bindings
                        || seen_kinds.contains(&option_kind)
                    {
                        return this;
                    }

                    seen_kinds.push(option_kind);
                    this.key_binding(
                        KeyBinding::for_action_in(action, &params.focus_handle, cx)
                            .map(|key_binding| key_binding.size(rems_from_px(12.))),
                    )
                })
                .label_size(LabelSize::Small)
                .on_click(cx.listener(move |this, _, window, cx| {
                    authorize(
                        this,
                        session_id.clone(),
                        tool_call_id.clone(),
                        SelectedPermissionOutcome::new(option_id_for_outcome.clone(), option_kind),
                        window,
                        cx,
                    );
                }))
        }))
}
