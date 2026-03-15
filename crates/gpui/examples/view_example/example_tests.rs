//! Tests for the `ExampleEditor` entity.
//!
//! These use GPUI's test infrastructure which requires the `test-support` feature:
//!
//! ```sh
//! cargo test --example view_example -p gpui --features test-support
//! ```

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::{Context, Entity, KeyBinding, TestAppContext, Window, prelude::*};

    use crate::example_editor::ExampleEditor;
    use crate::example_input::{ExampleInput, ExampleInputState};
    use crate::example_render_log::RenderLog;
    use crate::example_text_area::ExampleTextArea;
    use crate::{Backspace, Delete, End, Enter, Home, Left, Right};

    struct InputWrapper {
        input_state: Entity<ExampleInputState>,
        render_log: Entity<RenderLog>,
    }

    impl InputWrapper {
        fn editor(&self, cx: &gpui::App) -> Entity<ExampleEditor> {
            self.input_state.read(cx).editor.clone()
        }
    }

    impl Render for InputWrapper {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            ExampleInput::new(self.input_state.clone(), self.render_log.clone())
        }
    }

    struct TextAreaWrapper {
        editor: Entity<ExampleEditor>,
        render_log: Entity<RenderLog>,
    }

    impl Render for TextAreaWrapper {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            ExampleTextArea::new(self.editor.clone(), self.render_log.clone(), 5)
        }
    }

    fn bind_keys(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("backspace", Backspace, None),
                KeyBinding::new("delete", Delete, None),
                KeyBinding::new("left", Left, None),
                KeyBinding::new("right", Right, None),
                KeyBinding::new("home", Home, None),
                KeyBinding::new("end", End, None),
                KeyBinding::new("enter", Enter, None),
            ]);
        });
    }

    fn init_input(
        cx: &mut TestAppContext,
    ) -> (Entity<ExampleEditor>, &mut gpui::VisualTestContext) {
        bind_keys(cx);

        let (wrapper, cx) = cx.add_window_view(|window, cx| {
            let render_log = cx.new(|cx| RenderLog::new(cx));
            let input_state = cx.new(|cx| ExampleInputState::new(render_log.clone(), window, cx));
            InputWrapper {
                input_state,
                render_log,
            }
        });

        let editor = cx.read_entity(&wrapper, |wrapper, cx| wrapper.editor(cx));

        cx.update(|window, cx| {
            let focus_handle = editor.read(cx).focus_handle.clone();
            window.focus(&focus_handle, cx);
        });

        (editor, cx)
    }

    fn init_textarea(
        cx: &mut TestAppContext,
    ) -> (Entity<ExampleEditor>, &mut gpui::VisualTestContext) {
        bind_keys(cx);

        let (wrapper, cx) = cx.add_window_view(|window, cx| {
            let editor = cx.new(|cx| ExampleEditor::new(window, cx));
            let render_log = cx.new(|cx| RenderLog::new(cx));
            TextAreaWrapper { editor, render_log }
        });

        let editor = cx.read_entity(&wrapper, |wrapper, _cx| wrapper.editor.clone());

        cx.update(|window, cx| {
            let focus_handle = editor.read(cx).focus_handle.clone();
            window.focus(&focus_handle, cx);
        });

        (editor, cx)
    }

    #[gpui::test]
    fn test_typing_and_cursor(cx: &mut TestAppContext) {
        let (editor, cx) = init_input(cx);

        cx.simulate_input("hello");

        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "hello");
            assert_eq!(editor.cursor, 5);
        });

        cx.simulate_keystrokes("left left");

        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.cursor, 3);
        });

        cx.simulate_input(" world");

        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "hel worldlo");
            assert_eq!(editor.cursor, 9);
        });
    }

    #[gpui::test]
    fn test_backspace_and_delete(cx: &mut TestAppContext) {
        let (editor, cx) = init_input(cx);

        cx.simulate_input("abcde");

        cx.simulate_keystrokes("backspace");
        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "abcd");
            assert_eq!(editor.cursor, 4);
        });

        cx.simulate_keystrokes("home delete");
        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "bcd");
            assert_eq!(editor.cursor, 0);
        });

        // Boundary no-ops
        cx.simulate_keystrokes("backspace");
        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "bcd");
        });

        cx.simulate_keystrokes("end delete");
        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "bcd");
        });
    }

    #[gpui::test]
    fn test_cursor_blink(cx: &mut TestAppContext) {
        let (editor, cx) = init_input(cx);

        // Typing calls reset_blink(), which makes cursor visible and
        // restarts the blink timer.
        cx.simulate_input("a");

        cx.read_entity(&editor, |editor, _cx| {
            assert!(
                editor.cursor_visible,
                "cursor should be visible after typing"
            );
        });

        // After 500ms the blink task toggles it off.
        cx.background_executor
            .advance_clock(Duration::from_millis(500));
        cx.run_until_parked();

        cx.read_entity(&editor, |editor, _cx| {
            assert!(!editor.cursor_visible, "cursor should have blinked off");
        });

        // Typing again resets the blink.
        cx.simulate_input("b");

        cx.read_entity(&editor, |editor, _cx| {
            assert!(
                editor.cursor_visible,
                "cursor should be visible after typing again"
            );
        });
    }

    #[gpui::test]
    fn test_enter_does_not_insert_in_input(cx: &mut TestAppContext) {
        let (editor, cx) = init_input(cx);

        cx.simulate_input("hello");
        cx.simulate_keystrokes("enter");

        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(
                editor.content, "hello",
                "Enter should not insert text in Input"
            );
            assert_eq!(editor.cursor, 5);
        });
    }

    #[gpui::test]
    fn test_enter_inserts_newline_in_textarea(cx: &mut TestAppContext) {
        let (editor, cx) = init_textarea(cx);

        cx.simulate_input("ab");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("cd");

        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "ab\ncd");
            assert_eq!(editor.cursor, 5);
        });
    }

    #[gpui::test]
    fn test_enter_at_start_of_textarea(cx: &mut TestAppContext) {
        let (editor, cx) = init_textarea(cx);

        cx.simulate_keystrokes("enter");
        cx.simulate_input("hello");

        cx.read_entity(&editor, |editor, _cx| {
            assert_eq!(editor.content, "\nhello");
            assert_eq!(editor.cursor, 6);
        });
    }
}
