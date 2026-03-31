use std::sync::LazyLock;

use gpui::{
    AnyWindowHandle, AppContext, Context, FocusHandle, Focusable, StatefulInteractiveElement, Task,
};
use parking_lot::Mutex;

use crate::workspace_settings;

#[derive(Default)]
struct FfmState {
    // The window and element to be focused
    handles: Option<(AnyWindowHandle, FocusHandle)>,
    // The debounced task which will do the focusing
    debounce_task: Option<Task<()>>,
}

// Global focus-follows-mouse state.
static FFM_STATE: LazyLock<Mutex<FfmState>> = LazyLock::new(Default::default);

pub trait FocusFollowsMouse<E: Focusable>: StatefulInteractiveElement {
    fn focus_follows_mouse(
        self,
        settings: workspace_settings::FocusFollowsMouse,
        cx: &Context<E>,
    ) -> Self {
        if settings.enabled {
            self.on_hover(cx.listener(move |this, enter, window, cx| {
                if *enter {
                    let window_handle = window.window_handle();
                    let focus_handle = this.focus_handle(cx);

                    let mut state = FFM_STATE.lock();

                    // Set the window/element to be focused to the most recent hovered element.
                    state.handles.replace((window_handle, focus_handle));

                    // Start a task to focus the most recent target after the debounce period
                    state
                        .debounce_task
                        .replace(cx.spawn(async move |_this, cx| {
                            cx.background_executor().timer(settings.debounce).await;

                            let mut state = FFM_STATE.lock();
                            let Some((window, focus)) = state.handles.take() else {
                                return;
                            };

                            let _ = cx.update_window(window, move |_view, window, cx| {
                                window.focus(&focus, cx);
                            });
                        }));
                }
            }))
        } else {
            self
        }
    }
}

impl<E: Focusable, T: StatefulInteractiveElement> FocusFollowsMouse<E> for T {}
