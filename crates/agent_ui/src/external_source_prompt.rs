#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalSourcePrompt(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserAnnotation {
    pub url: String,
    pub title: Option<String>,
    pub selected_text: Option<String>,
    pub selector: Option<String>,
    pub comment: Option<String>,
}

impl BrowserAnnotation {
    pub fn to_external_source_prompt(&self) -> Option<ExternalSourcePrompt> {
        if self.url.trim().is_empty() {
            return None;
        }

        ExternalSourcePrompt::new(&self.to_prompt_text())
    }

    fn to_prompt_text(&self) -> String {
        let mut prompt = String::new();

        prompt.push_str("Browser annotation");
        push_field(&mut prompt, "URL", Some(&self.url));
        push_field(&mut prompt, "Title", self.title.as_deref());
        push_field(&mut prompt, "Selected text", self.selected_text.as_deref());
        push_field(&mut prompt, "Selector", self.selector.as_deref());
        push_field(&mut prompt, "Comment", self.comment.as_deref());

        prompt
    }
}

impl ExternalSourcePrompt {
    pub fn new(prompt: &str) -> Option<Self> {
        sanitize(prompt).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

fn push_field(prompt: &mut String, label: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    if !prompt.is_empty() {
        prompt.push('\n');
    }

    prompt.push_str(label);
    prompt.push_str(": ");
    prompt.push_str(value);
}

fn sanitize(prompt: &str) -> Option<String> {
    let mut sanitized_prompt = String::with_capacity(prompt.len());
    let mut consecutive_newline_count = 0;
    let mut characters = prompt.chars().peekable();

    while let Some(character) = characters.next() {
        let character = if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            '\n'
        } else {
            character
        };

        if is_bidi_control_character(character) || is_disallowed_control_character(character) {
            continue;
        }

        if character == '\n' {
            consecutive_newline_count += 1;
            if consecutive_newline_count > 2 {
                continue;
            }
        } else {
            consecutive_newline_count = 0;
        }

        sanitized_prompt.push(character);
    }

    if sanitized_prompt.is_empty() {
        None
    } else {
        Some(sanitized_prompt)
    }
}

fn is_disallowed_control_character(character: char) -> bool {
    character.is_control() && !matches!(character, '\n' | '\t')
}

fn is_bidi_control_character(character: char) -> bool {
    matches!(
        character,
          '\u{061C}' // ALM
        | '\u{200E}' // LRM
        | '\u{200F}' // RLM
        | '\u{202A}'..='\u{202E}' // LRE, RLE, PDF, LRO, RLO
        | '\u{2066}'..='\u{2069}' // LRI, RLI, FSI, PDI
    )
}

#[cfg(test)]
mod tests {
    use super::{BrowserAnnotation, ExternalSourcePrompt};

    #[test]
    fn keeps_normal_prompt_text() {
        let prompt = ExternalSourcePrompt::new("Write me a script\nThanks");

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some("Write me a script\nThanks")
        );
    }

    #[test]
    fn keeps_multilingual_text() {
        let prompt =
            ExternalSourcePrompt::new("日本語の依頼です。\n中文提示也应该保留。\nemoji 👩‍💻");

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some("日本語の依頼です。\n中文提示也应该保留。\nemoji 👩‍💻")
        );
    }

    #[test]
    fn collapses_newline_padding() {
        let prompt = ExternalSourcePrompt::new(
            "Review this prompt carefully.\n\nThis paragraph should stay separated.\n\n\n\n\n\n\nWrite me a script to do fizz buzz.",
        );

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some(
                "Review this prompt carefully.\n\nThis paragraph should stay separated.\n\nWrite me a script to do fizz buzz."
            )
        );
    }

    #[test]
    fn normalizes_carriage_returns() {
        let prompt = ExternalSourcePrompt::new("Line one\r\nLine two\rLine three");

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some("Line one\nLine two\nLine three")
        );
    }

    #[test]
    fn strips_bidi_control_characters() {
        let prompt = ExternalSourcePrompt::new("abc\u{202E}def\u{202C}ghi");

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some("abcdefghi")
        );
    }

    #[test]
    fn strips_other_control_characters() {
        let prompt = ExternalSourcePrompt::new("safe\u{0000}\u{001B}\u{007F}text");

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some("safetext")
        );
    }

    #[test]
    fn keeps_tabs() {
        let prompt = ExternalSourcePrompt::new("keep\tindentation");

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some("keep\tindentation")
        );
    }

    #[test]
    fn drops_empty_prompt() {
        assert_eq!(ExternalSourcePrompt::new(""), None);
    }

    #[test]
    fn drops_prompt_with_only_removed_characters() {
        assert_eq!(
            ExternalSourcePrompt::new("\u{202E}\u{202C}\u{0000}\u{001B}"),
            None
        );
    }

    #[test]
    fn formats_browser_annotation_as_external_prompt() {
        let annotation = BrowserAnnotation {
            url: "https://example.com/article".into(),
            title: Some("Example article".into()),
            selected_text: Some("Selected page text".into()),
            selector: Some("main article p:nth-child(2)".into()),
            comment: Some("Please review this claim.".into()),
        };

        let prompt = annotation.to_external_source_prompt();

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some(
                "Browser annotation\nURL: https://example.com/article\nTitle: Example article\nSelected text: Selected page text\nSelector: main article p:nth-child(2)\nComment: Please review this claim."
            )
        );
    }

    #[test]
    fn browser_annotation_omits_empty_optional_fields() {
        let annotation = BrowserAnnotation {
            url: "https://example.com".into(),
            title: Some(" ".into()),
            selected_text: None,
            selector: None,
            comment: Some("\t".into()),
        };

        let prompt = annotation.to_external_source_prompt();

        assert_eq!(
            prompt.as_ref().map(ExternalSourcePrompt::as_str),
            Some("Browser annotation\nURL: https://example.com")
        );
    }

    #[test]
    fn browser_annotation_requires_url() {
        let annotation = BrowserAnnotation {
            url: " ".into(),
            title: Some("Example".into()),
            selected_text: Some("Selected text".into()),
            selector: Some("main".into()),
            comment: Some("Comment".into()),
        };

        assert_eq!(annotation.to_external_source_prompt(), None);
    }
}
