//! JSON-specific editing assistance layered on GtkSourceView.
//!
//! Key events are converted into pure edit plans for bracket completion,
//! generated-closer skipping, paired deletion, indentation, and closer alignment.
//! Structural scanning understands JSON strings and escapes so punctuation inside
//! a string is never treated as syntax. The planner is testable without GTK.
//!
//! Generated closing characters are tracked with moving [`TextMark`] values
//! rather than byte offsets because GTK updates marks as surrounding text changes.
//! Programmatic loads clear those marks and reset undo history so one request
//! cannot undo into the previously selected request.

use std::{cell::RefCell, rc::Rc};

use gtk::{EventControllerKey, TextBuffer, TextMark, gdk, glib, prelude::*};
use sourceview5::View;

const INDENT: &str = "  ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyAction {
    Open(char),
    Close(char),
    Enter,
    Backspace,
}

#[derive(Debug, PartialEq, Eq)]
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
    cursor: usize,
    generated_closer: Option<(usize, char)>,
    consume_generated_closer: bool,
}

impl Edit {
    fn replace(start: usize, end: usize, replacement: String, cursor: usize) -> Self {
        Self {
            start,
            end,
            replacement,
            cursor,
            generated_closer: None,
            consume_generated_closer: false,
        }
    }
}

struct GeneratedCloser {
    mark: TextMark,
    value: char,
}

#[derive(Default)]
pub(super) struct JsonEditorState {
    generated_closers: RefCell<Vec<GeneratedCloser>>,
}

pub(super) fn configure(view: &View) -> Rc<JsonEditorState> {
    let state = Rc::new(JsonEditorState::default());
    view.buffer().connect_changed({
        let view = view.downgrade();
        move |_| {
            if let Some(view) = view.upgrade() {
                // GtkTextView normally invalidates changed glyph runs itself. A
                // full widget invalidation also clears stale cached glyphs after
                // the surrounding GtkPaned changes the editor allocation.
                view.queue_draw();
            }
        }
    });
    let controller = EventControllerKey::new();
    controller.connect_key_pressed({
        let view = view.downgrade();
        let state = state.clone();
        move |_, key, _, modifiers| {
            let Some(view) = view.upgrade() else {
                return glib::Propagation::Proceed;
            };
            handle_key(&view, &state, key, modifiers)
        }
    });
    view.add_controller(controller);
    state
}

pub(super) fn set_text(view: &View, state: &JsonEditorState, text: &str) {
    let buffer = view.buffer();
    clear_generated_closers(&buffer, state);
    // Loading and cURL import establish a new document baseline. They must not
    // become an undo step that restores the previously selected request.
    buffer.set_enable_undo(false);
    buffer.set_text(text);
    buffer.set_enable_undo(true);
    buffer.set_max_undo_levels(100);
}

fn handle_key(
    view: &View,
    state: &JsonEditorState,
    key: gdk::Key,
    modifiers: gdk::ModifierType,
) -> glib::Propagation {
    if modifiers.intersects(
        gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK,
    ) {
        return glib::Propagation::Proceed;
    }
    let action = if key == gdk::Key::braceleft {
        Some(KeyAction::Open('{'))
    } else if key == gdk::Key::bracketleft {
        Some(KeyAction::Open('['))
    } else if key == gdk::Key::braceright {
        Some(KeyAction::Close('}'))
    } else if key == gdk::Key::bracketright {
        Some(KeyAction::Close(']'))
    } else if key == gdk::Key::Return || key == gdk::Key::KP_Enter {
        Some(KeyAction::Enter)
    } else if key == gdk::Key::BackSpace {
        Some(KeyAction::Backspace)
    } else {
        None
    };
    let Some(action) = action else {
        return glib::Propagation::Proceed;
    };

    let buffer = view.buffer();
    prune_generated_closers(&buffer, state);
    let selection = buffer.selection_bounds().map(|(start, end)| {
        let start = start.offset().max(0) as usize;
        let end = end.offset().max(0) as usize;
        (start.min(end), start.max(end))
    });
    if selection.is_some() {
        return glib::Propagation::Proceed;
    }
    let cursor = buffer.cursor_position().max(0) as usize;
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    let generated = generated_closer_for_action(&buffer, state, &text, cursor, action);
    let generated_index = generated.map(|(index, _, _)| index);
    let generated = generated.map(|(_, offset, value)| (offset, value));
    let planned = plan_edit(&text, cursor, action, generated);
    let Some(edit) = planned else {
        return glib::Propagation::Proceed;
    };

    if edit.consume_generated_closer
        && let Some(index) = generated_index
    {
        let closer = state.generated_closers.borrow_mut().remove(index);
        buffer.delete_mark(&closer.mark);
    }

    if edit.start == edit.end && edit.replacement.is_empty() {
        buffer.place_cursor(&buffer.iter_at_offset(edit.cursor as i32));
        return glib::Propagation::Stop;
    }

    buffer.begin_user_action();
    let mut start = buffer.iter_at_offset(edit.start as i32);
    let mut end = buffer.iter_at_offset(edit.end as i32);
    if edit.start != edit.end {
        buffer.delete(&mut start, &mut end);
    }
    buffer.insert(&mut start, &edit.replacement);
    buffer.place_cursor(&buffer.iter_at_offset(edit.cursor as i32));
    if let Some((offset, value)) = edit.generated_closer {
        let mark = buffer.create_mark(None, &buffer.iter_at_offset(offset as i32), false);
        state
            .generated_closers
            .borrow_mut()
            .push(GeneratedCloser { mark, value });
    }
    buffer.end_user_action();
    glib::Propagation::Stop
}

fn generated_closer_for_action(
    buffer: &TextBuffer,
    state: &JsonEditorState,
    text: &str,
    cursor: usize,
    action: KeyAction,
) -> Option<(usize, usize, char)> {
    let expected = match action {
        KeyAction::Close(value) => Some(value),
        KeyAction::Backspace => None,
        _ => return None,
    };
    state
        .generated_closers
        .borrow()
        .iter()
        .enumerate()
        .filter_map(|(index, closer)| {
            let offset = buffer.iter_at_mark(&closer.mark).offset().max(0) as usize;
            let matches_position = match action {
                KeyAction::Close(_) => {
                    offset >= cursor && range_is_whitespace(text, cursor, offset)
                }
                KeyAction::Backspace => offset == cursor,
                _ => false,
            };
            (matches_position && expected.is_none_or(|value| value == closer.value)).then_some((
                index,
                offset,
                closer.value,
            ))
        })
        .min_by_key(|(_, offset, _)| *offset)
}

fn prune_generated_closers(buffer: &TextBuffer, state: &JsonEditorState) {
    let stale = {
        let closers = state.generated_closers.borrow();
        closers
            .iter()
            .enumerate()
            .filter_map(|(index, closer)| {
                let iter = buffer.iter_at_mark(&closer.mark);
                (iter.is_end() || iter.char() != closer.value).then_some(index)
            })
            .collect::<Vec<_>>()
    };
    for index in stale.into_iter().rev() {
        let closer = state.generated_closers.borrow_mut().remove(index);
        buffer.delete_mark(&closer.mark);
    }
}

fn clear_generated_closers(buffer: &TextBuffer, state: &JsonEditorState) {
    for closer in state.generated_closers.borrow_mut().drain(..) {
        buffer.delete_mark(&closer.mark);
    }
}

fn plan_edit(
    text: &str,
    cursor: usize,
    action: KeyAction,
    generated_closer: Option<(usize, char)>,
) -> Option<Edit> {
    let context = scan_context(text, cursor);
    match action {
        KeyAction::Open(open) if !context.in_string => {
            let close = matching_close(open)?;
            let mut edit = Edit::replace(cursor, cursor, format!("{open}{close}"), cursor + 1);
            edit.generated_closer = Some((cursor + 1, close));
            Some(edit)
        }
        KeyAction::Open(_) => None,
        KeyAction::Close(close)
            if !context.in_string && generated_closer == Some((cursor, close)) =>
        {
            let mut edit = Edit::replace(cursor, cursor, String::new(), cursor + 1);
            edit.consume_generated_closer = true;
            Some(edit)
        }
        KeyAction::Close(close)
            if !context.in_string
                && generated_closer
                    .is_some_and(|(offset, value)| offset > cursor && value == close) =>
        {
            align_existing_generated_closer(text, cursor, close, generated_closer?.0, &context)
        }
        KeyAction::Close(close) if !context.in_string => {
            align_closer(text, cursor, close, &context)
        }
        KeyAction::Close(_) => None,
        KeyAction::Enter => newline_edit(text, cursor, &context),
        KeyAction::Backspace
            if !context.in_string
                && generated_closer.is_some()
                && cursor > 0
                && char_at(text, cursor - 1)
                    .and_then(matching_close)
                    .is_some_and(|close| generated_closer == Some((cursor, close))) =>
        {
            let mut edit = Edit::replace(cursor - 1, cursor + 1, String::new(), cursor - 1);
            edit.consume_generated_closer = true;
            Some(edit)
        }
        KeyAction::Backspace => None,
    }
}

#[derive(Default)]
struct JsonContext {
    in_string: bool,
    stack: Vec<(char, usize)>,
}

fn scan_context(text: &str, cursor: usize) -> JsonContext {
    let mut context = JsonContext::default();
    let mut escaped = false;
    for (offset, value) in text.chars().take(cursor).enumerate() {
        if context.in_string {
            if escaped {
                escaped = false;
            } else if value == '\\' {
                escaped = true;
            } else if value == '"' {
                context.in_string = false;
            }
            continue;
        }
        match value {
            '"' => context.in_string = true,
            '{' | '[' => context.stack.push((value, offset)),
            '}' | ']'
                if context
                    .stack
                    .last()
                    .is_some_and(|(open, _)| matching_close(*open) == Some(value)) =>
            {
                context.stack.pop();
            }
            _ => {}
        }
    }
    context
}

fn newline_edit(text: &str, cursor: usize, context: &JsonContext) -> Option<Edit> {
    let line_start = line_start(text, cursor);
    let base_indent = leading_spaces(text, line_start);
    let previous = cursor
        .checked_sub(1)
        .and_then(|offset| char_at(text, offset));
    let next = char_at(text, cursor);
    let empty_pair = !context.in_string
        && previous
            .and_then(matching_close)
            .is_some_and(|close| Some(close) == next);

    if empty_pair {
        let replacement = format!(
            "\n{}{INDENT}\n{}",
            " ".repeat(base_indent),
            " ".repeat(base_indent)
        );
        let new_cursor = cursor + 1 + base_indent + INDENT.len();
        return Some(Edit::replace(cursor, cursor, replacement, new_cursor));
    }

    let add_level = !context.in_string
        && previous_non_whitespace(text, line_start, cursor)
            .is_some_and(|value| matches!(value, '{' | '['));
    let indent = base_indent + usize::from(add_level) * INDENT.len();
    let replacement = format!("\n{}", " ".repeat(indent));
    Some(Edit::replace(
        cursor,
        cursor,
        replacement,
        cursor + 1 + indent,
    ))
}

fn align_closer(text: &str, cursor: usize, close: char, context: &JsonContext) -> Option<Edit> {
    let current_line_start = line_start(text, cursor);
    if !text
        .chars()
        .skip(current_line_start)
        .take(cursor.saturating_sub(current_line_start))
        .all(|value| value == ' ')
    {
        return None;
    }
    let (open, open_offset) = context.stack.last().copied()?;
    if matching_close(open) != Some(close) {
        return None;
    }
    let indent = leading_spaces(text, line_start(text, open_offset));
    let replacement = format!("{}{close}", " ".repeat(indent));
    Some(Edit::replace(
        current_line_start,
        cursor,
        replacement,
        current_line_start + indent + 1,
    ))
}

fn align_existing_generated_closer(
    text: &str,
    cursor: usize,
    close: char,
    closer_offset: usize,
    context: &JsonContext,
) -> Option<Edit> {
    let current_line_start = line_start(text, cursor);
    if !range_is_spaces(text, current_line_start, cursor)
        || !range_is_whitespace(text, cursor, closer_offset)
    {
        return None;
    }
    let (open, open_offset) = context.stack.last().copied()?;
    if matching_close(open) != Some(close) {
        return None;
    }
    let indent = leading_spaces(text, line_start(text, open_offset));
    let mut edit = Edit::replace(
        current_line_start,
        closer_offset,
        " ".repeat(indent),
        current_line_start + indent + 1,
    );
    edit.consume_generated_closer = true;
    Some(edit)
}

fn line_start(text: &str, cursor: usize) -> usize {
    text.chars()
        .take(cursor)
        .enumerate()
        .filter_map(|(offset, value)| (value == '\n').then_some(offset + 1))
        .last()
        .unwrap_or(0)
}

fn leading_spaces(text: &str, start: usize) -> usize {
    text.chars()
        .skip(start)
        .take_while(|value| *value == ' ')
        .count()
}

fn previous_non_whitespace(text: &str, start: usize, cursor: usize) -> Option<char> {
    text.chars()
        .skip(start)
        .take(cursor.saturating_sub(start))
        .filter(|value| !value.is_whitespace())
        .last()
}

fn range_is_spaces(text: &str, start: usize, end: usize) -> bool {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .all(|value| value == ' ')
}

fn range_is_whitespace(text: &str, start: usize, end: usize) -> bool {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .all(char::is_whitespace)
}

fn char_at(text: &str, offset: usize) -> Option<char> {
    text.chars().nth(offset)
}

fn matching_close(open: char) -> Option<char> {
    match open {
        '{' => Some('}'),
        '[' => Some(']'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(text: &str, cursor: usize, action: KeyAction, generated: Option<char>) -> Option<Edit> {
        plan_edit(text, cursor, action, generated.map(|value| (cursor, value)))
    }

    #[test]
    fn completes_nested_pairs_and_skips_generated_closers() {
        let object = edit("", 0, KeyAction::Open('{'), None).unwrap();
        assert_eq!(object.replacement, "{}");
        assert_eq!(object.cursor, 1);
        assert_eq!(object.generated_closer, Some((1, '}')));

        let array = edit("{}", 1, KeyAction::Open('['), None).unwrap();
        assert_eq!(array.replacement, "[]");
        assert_eq!(array.cursor, 2);
        let close = edit("{[]}", 2, KeyAction::Close(']'), Some(']')).unwrap();
        assert_eq!(close.cursor, 3);
        assert!(close.consume_generated_closer);
    }

    #[test]
    fn treats_brackets_inside_strings_as_literal_text() {
        assert!(edit(r#"{"text":""}"#, 9, KeyAction::Open('{'), None).is_none());
        let odd_escape = r#""a\""#;
        assert!(
            edit(
                odd_escape,
                odd_escape.chars().count(),
                KeyAction::Open('['),
                None
            )
            .is_none()
        );
        let even_escape = r#""a\\""#;
        assert!(
            edit(
                even_escape,
                even_escape.chars().count(),
                KeyAction::Open('['),
                None
            )
            .is_some()
        );
    }

    #[test]
    fn enter_indents_and_expands_empty_pairs() {
        let pair_edit = edit("{}", 1, KeyAction::Enter, None).unwrap();
        assert_eq!(pair_edit.replacement, "\n  \n");
        assert_eq!(pair_edit.cursor, 4);

        let nested_edit = edit("{\n  \"value\": [1,", 16, KeyAction::Enter, None).unwrap();
        assert_eq!(nested_edit.replacement, "\n  ");
    }

    #[test]
    fn aligns_closer_with_its_matching_opening_line() {
        let text = "  {\n      ";
        let edit = edit(text, text.chars().count(), KeyAction::Close('}'), None).unwrap();
        assert_eq!(edit.start, 4);
        assert_eq!(edit.replacement, "  }");
        assert_eq!(edit.cursor, 7);
    }

    #[test]
    fn accepts_an_expanded_generated_closer_without_duplicating_it() {
        let text = "{\n  \n}";
        let edit = plan_edit(text, 4, KeyAction::Close('}'), Some((5, '}'))).unwrap();
        assert_eq!((edit.start, edit.end), (2, 5));
        assert_eq!(edit.replacement, "");
        assert_eq!(edit.cursor, 3);
        assert!(edit.consume_generated_closer);
    }

    #[test]
    fn backspace_removes_only_an_untouched_generated_pair() {
        let remove = edit("{}", 1, KeyAction::Backspace, Some('}')).unwrap();
        assert_eq!((remove.start, remove.end, remove.cursor), (0, 2, 0));
        assert!(remove.consume_generated_closer);
        assert!(edit("{x}", 2, KeyAction::Backspace, Some('}')).is_none());
        assert!(edit("{}", 1, KeyAction::Backspace, None).is_none());
    }
}
