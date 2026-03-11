use iced::widget::text_editor;
use std::sync::Arc;

pub fn smart_indent(content: &mut text_editor::Content, action: text_editor::Action) {
    match action {
        text_editor::Action::Edit(text_editor::Edit::Insert(c @ ('{' | '[' | '"'))) => {
            let matching = match c {
                '{' => '}',
                '[' => ']',
                '"' => '"',
                _ => unreachable!(),
            };
            content.perform(text_editor::Action::Edit(text_editor::Edit::Insert(c)));
            content.perform(text_editor::Action::Edit(text_editor::Edit::Insert(
                matching,
            )));
            content.perform(text_editor::Action::Move(text_editor::Motion::Left));
        }
        text_editor::Action::Edit(text_editor::Edit::Enter) => {
            let cursor = content.cursor();
            let line_index = cursor.position.line;
            if let Some(line) = content.line(line_index) {
                let text = &line.text;
                let column = cursor.position.column;

                let byte_offset = text
                    .char_indices()
                    .nth(column)
                    .map(|(i, _)| i)
                    .unwrap_or(text.len());

                let text_before_cursor = &text[..byte_offset];

                let ws_end = text_before_cursor
                    .find(|c: char| !c.is_whitespace())
                    .unwrap_or(text_before_cursor.len());
                let leading_whitespace = &text_before_cursor[..ws_end];

                let trimmed_before = text_before_cursor.trim_end();
                let ends_with_open = trimmed_before.ends_with('{') || trimmed_before.ends_with('[');

                let char_after_cursor = text.chars().nth(column);
                let is_between_brackets = ends_with_open
                    && match (trimmed_before.chars().last(), char_after_cursor) {
                        (Some('{'), Some('}')) => true,
                        (Some('['), Some(']')) => true,
                        _ => false,
                    };

                if is_between_brackets {
                    let mut to_insert = String::with_capacity(leading_whitespace.len() * 2 + 4);
                    to_insert.push('\n');
                    to_insert.push_str(leading_whitespace);
                    to_insert.push('\t');
                    to_insert.push('\n');
                    to_insert.push_str(leading_whitespace);

                    content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(to_insert),
                    )));
                    content.perform(text_editor::Action::Move(text_editor::Motion::Up));
                    content.perform(text_editor::Action::Move(text_editor::Motion::End));
                } else {
                    let mut to_insert = String::with_capacity(leading_whitespace.len() + 2);
                    to_insert.push('\n');
                    to_insert.push_str(leading_whitespace);
                    if ends_with_open {
                        to_insert.push('\t');
                    }
                    content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(to_insert),
                    )));
                }
            } else {
                content.perform(action);
            }
        }
        _ => content.perform(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::{Action, Content, Cursor, Edit, Motion, Position};

    fn assert_content(
        content: &Content,
        expected_text: &str,
        expected_line: usize,
        expected_column: usize,
    ) {
        let text = content.text();
        assert_eq!(text, expected_text, "Content text mismatch");
        let cursor = content.cursor();
        assert_eq!(cursor.position.line, expected_line, "Line mismatch");
        assert_eq!(cursor.position.column, expected_column, "Column mismatch");
    }

    #[test]
    fn test_insert_brackets_and_quotes() {
        let mut content = Content::new();
        smart_indent(&mut content, Action::Edit(Edit::Insert('{')));
        assert_content(&content, "{}", 0, 1);

        let mut content = Content::new();
        smart_indent(&mut content, Action::Edit(Edit::Insert('[')));
        assert_content(&content, "[]", 0, 1);

        let mut content = Content::new();
        smart_indent(&mut content, Action::Edit(Edit::Insert('"')));
        assert_content(&content, "\"\"", 0, 1);
    }

    #[test]
    fn test_insert_normal_char() {
        let mut content = Content::new();
        smart_indent(&mut content, Action::Edit(Edit::Insert('a')));
        assert_content(&content, "a", 0, 1);
    }

    #[test]
    fn test_default_action() {
        let mut content = Content::with_text("abc");
        smart_indent(&mut content, Action::Move(Motion::Right));
        assert_content(&content, "abc", 0, 1);
    }

    #[test]
    fn test_enter_simple_indent() {
        let mut content = Content::with_text("    abc");
        content.perform(Action::Move(Motion::DocumentEnd));
        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "    abc\n    ", 1, 4);
    }

    #[test]
    fn test_enter_after_brace_indents() {
        let mut content = Content::with_text("  {");
        content.perform(Action::Move(Motion::DocumentEnd));
        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "  {\n  \t", 1, 3);
    }

    #[test]
    fn test_enter_after_bracket_indents() {
        let mut content = Content::with_text("[");
        content.perform(Action::Move(Motion::DocumentEnd));
        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "[\n\t", 1, 1);
    }

    #[test]
    fn test_enter_between_braces_splits() {
        let mut content = Content::with_text("  {}");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));
        content.perform(Action::Move(Motion::Right));
        content.perform(Action::Move(Motion::Right));

        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "  {\n  \t\n  }", 1, 3);
    }

    #[test]
    fn test_enter_between_brackets_splits() {
        let mut content = Content::with_text("[]");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));

        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "[\n\t\n]", 1, 1);
    }

    #[test]
    fn test_enter_with_multibyte_chars() {
        let mut content = Content::with_text("🚀 {");
        content.perform(Action::Move(Motion::DocumentEnd));
        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "🚀 {\n\t", 1, 1);
    }

    #[test]
    fn test_enter_invalid_line_index() {
        let mut content = Content::new();
        content.move_to(Cursor {
            position: Position {
                line: 100,
                column: 0,
            },
            selection: None,
        });

        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert!(content.line_count() > 1);
    }

    #[test]
    fn test_enter_between_mismatched_brackets() {
        let mut content = Content::with_text("{]");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));
        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "{\n\t]", 1, 1);
    }

    #[test]
    fn test_enter_empty_content() {
        let mut content = Content::new();
        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "\n", 1, 0);
    }

    #[test]
    fn test_enter_start_of_line_no_indent() {
        let mut content = Content::with_text("abc");
        content.perform(Action::Move(Motion::DocumentStart));
        smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "\nabc", 1, 0);
    }

    #[test]
    fn test_insert_bracket_at_end_of_large_content() {
        let mut content = Content::with_text("{\n\t\"a\": \"b\"\n}");
        content.perform(Action::Move(Motion::DocumentEnd));
        smart_indent(&mut content, Action::Edit(Edit::Insert('{')));
        assert_content(&content, "{\n\t\"a\": \"b\"\n}{}", 2, 2);
    }
}
