use iced::{
    Font, Length, font,
    keyboard::{Key, key::Named},
    padding,
    widget::{
        button, center_x, column, pick_list, responsive, row, table, text, text_editor, text_input,
    },
};
use uuid::Uuid;

use crate::models::{EDITOR_TABS, EditorTab, HTTP_METHODS, HTTPMethod};

#[derive(Default)]
pub struct AppState {
    http_method: HTTPMethod,
    url: String,
    editor_tab: EditorTab,
    raw_body_content: text_editor::Content,
    response: text_editor::Content,
    param_rows: Vec<ParamRow>,
}

#[derive(Debug, Clone)]
pub enum AppMessage {
    HttpMethodSelected(HTTPMethod),
    UrlChanged(String),
    SendClicked,
    EditorTabSelected(EditorTab),
    RawBodyContentEdit(text_editor::Action),
    Response(Result<String, Error>),
    ResponseEdit(text_editor::Action),
    ParamKeyChanged(String, String),
    ParamValueChanged(String, String),
    AddParamRow,
    RemoveParamRow(String),
}

#[derive(Debug, Clone)]
struct ParamRow {
    id: String,
    key: Option<String>,
    value: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            param_rows: vec![ParamRow {
                id: Uuid::new_v4().to_string(),
                key: Some("Content-Type".to_owned()),
                value: Some("application/json".to_owned()),
            }],
            ..Default::default()
        }
    }
    pub fn title(&self) -> String {
        String::from("Pakpos")
    }

    pub fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        match message {
            AppMessage::HttpMethodSelected(http_method) => {
                self.http_method = http_method;
                iced::Task::none()
            }
            AppMessage::UrlChanged(url) => {
                self.url = url;
                iced::Task::none()
            }
            AppMessage::SendClicked => {
                let fetcher = Fetcher::new(
                    self.http_method,
                    self.url.clone(),
                    self.raw_body_content.text(),
                );
                iced::Task::perform(fetcher.fetch(), AppMessage::Response)
            }
            AppMessage::EditorTabSelected(tab) => {
                self.editor_tab = tab;
                iced::Task::none()
            }
            AppMessage::RawBodyContentEdit(action) => {
                perform_smart_edit(&mut self.raw_body_content, action);
                iced::Task::none()
            }
            AppMessage::Response(response) => {
                match response {
                    Ok(result) => {
                        self.response = text_editor::Content::with_text(&result);
                    }
                    Err(err) => println!("Failed: {:?}", err),
                }
                iced::Task::none()
            }
            AppMessage::ResponseEdit(action) => {
                perform_smart_edit(&mut self.response, action);
                iced::Task::none()
            }
            AppMessage::ParamKeyChanged(id, val) => {
                if let Some(row) = self.param_rows.iter_mut().find(|r| r.id == id) {
                    row.key = Some(val);
                }
                iced::Task::none()
            }
            AppMessage::ParamValueChanged(id, val) => {
                if let Some(row) = self.param_rows.iter_mut().find(|r| r.id == id) {
                    row.value = Some(val);
                }
                iced::Task::none()
            }
            AppMessage::AddParamRow => {
                self.param_rows.push(ParamRow {
                    id: Uuid::new_v4().to_string(),
                    key: None,
                    value: None,
                });
                iced::Task::none()
            }
            AppMessage::RemoveParamRow(id) => {
                if let Some(pos) = self.param_rows.iter().position(|row| row.id == id) {
                    self.param_rows.remove(pos);
                }
                iced::Task::none()
            }
        }
    }

    pub fn view(&self) -> iced::Element<'_, AppMessage> {
        let button_message_active = if !self.url.is_empty() {
            Some(AppMessage::SendClicked)
        } else {
            None
        };

        let tab_editor_content_height = 400;

        let tab_editor_active_content: iced::Element<'_, AppMessage> = match self.editor_tab {
            EditorTab::Body => text_editor(&self.raw_body_content)
                .placeholder("Request Body")
                .on_action(AppMessage::RawBodyContentEdit)
                .highlight("json", iced::highlighter::Theme::SolarizedDark)
                .key_binding(|event| {
                    if event.key == Key::Named(Named::Tab) {
                        Some(text_editor::Binding::Insert('\t'))
                    } else {
                        text_editor::Binding::from_key_press(event)
                    }
                })
                .size(14)
                .padding(10)
                .height(tab_editor_content_height)
                .into(),
            EditorTab::Params => {
                let bold = |header| {
                    text(header).font(Font {
                        weight: font::Weight::Bold,
                        ..Font::DEFAULT
                    })
                };

                responsive(move |size| {
                    let columns = [
                        table::column(bold("Key"), |param: &ParamRow| {
                            let id = param.id.clone();
                            text_input("Key", param.key.as_deref().unwrap_or_default())
                                .on_input(move |val| AppMessage::ParamKeyChanged(id.clone(), val))
                                .width(Length::Fill)
                        })
                        .width(Length::Fill),
                        table::column(bold("Value"), |param: &ParamRow| {
                            let id = param.id.clone();
                            text_input("Value", param.value.as_deref().unwrap_or_default())
                                .on_input(move |val| AppMessage::ParamValueChanged(id.clone(), val))
                                .width(Length::Fill)
                        })
                        .width(Length::Fill),
                        table::column("Action", |param: &ParamRow| {
                            center_x(
                                button("del")
                                    .on_press(AppMessage::RemoveParamRow(param.id.clone()))
                                    .style(button::danger),
                            )
                        }),
                    ];

                    column!(
                        {
                            let param_table = if self.param_rows.is_empty() {
                                None
                            } else {
                                Some(
                                    table(columns, &self.param_rows)
                                        .width(size.width)
                                        .padding(5),
                                )
                            };
                            param_table
                        },
                        button("Add Param")
                            .on_press(AppMessage::AddParamRow)
                            .style(button::secondary)
                    )
                    .width(size.width)
                    .spacing(5)
                    .into()
                })
                .into()
            }
            _ => text(self.editor_tab.to_string()).into(),
        };

        column!(
            row!(
                pick_list(
                    HTTP_METHODS,
                    Some(self.http_method),
                    AppMessage::HttpMethodSelected
                )
                .placeholder("HTTP Method"),
                text_input("URL...", &self.url).on_input(AppMessage::UrlChanged),
                button("Send").on_press_maybe(button_message_active),
            )
            .spacing(5),
            row(EDITOR_TABS.map(|tab| {
                button(text!("{tab}"))
                    .style(move |theme: &iced::Theme, status| {
                        if self.editor_tab == tab {
                            button::subtle(theme, status)
                        } else {
                            button::text(theme, status)
                        }
                    })
                    .on_press(AppMessage::EditorTabSelected(tab))
                    .into()
            }))
            .spacing(10),
            tab_editor_active_content,
            text_editor(&self.response)
                .height(400)
                .highlight("json", iced::highlighter::Theme::SolarizedDark)
                .on_action(AppMessage::ResponseEdit)
                .key_binding(|event| {
                    if event.key == Key::Named(Named::Tab) {
                        Some(text_editor::Binding::Insert('\t'))
                    } else {
                        text_editor::Binding::from_key_press(event)
                    }
                })
                .size(14)
                .padding(padding::left(10))
        )
        .spacing(10)
        .padding(10)
        .into()
    }
}

fn perform_smart_edit(content: &mut text_editor::Content, action: text_editor::Action) {
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
                        std::sync::Arc::new(to_insert),
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
                        std::sync::Arc::new(to_insert),
                    )));
                }
            } else {
                content.perform(action);
            }
        }
        _ => content.perform(action),
    }
}

struct Fetcher {
    http_method: HTTPMethod,
    url: String,
    body: String,
}

impl Fetcher {
    fn new(http_method: HTTPMethod, url: String, body: String) -> Self {
        Self {
            http_method,
            url,
            body,
        }
    }
    async fn fetch(self) -> Result<String, Error> {
        let client = reqwest::Client::new();
        let mut req = match self.http_method {
            HTTPMethod::Get => client.get(&self.url),
            HTTPMethod::Post => client.post(&self.url),
            HTTPMethod::Put => client.put(&self.url),
            HTTPMethod::Delete => client.delete(&self.url),
            HTTPMethod::Patch => client.patch(&self.url),
            HTTPMethod::Head => client.head(&self.url),
        };
        if !self.body.is_empty() {
            req = req.body(self.body.clone());
        }

        let result = req.send().await?;
        let response = result.text().await?;
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        serde::Serialize::serialize(&parsed, &mut ser)?;
        Ok(String::from_utf8(buf).map_err(|_| Error::SerdeError)?)
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    APIError,
    SerdeError,
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        dbg!(value);
        Self::APIError
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        dbg!(value);
        Self::SerdeError
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
        perform_smart_edit(&mut content, Action::Edit(Edit::Insert('{')));
        assert_content(&content, "{}", 0, 1);

        let mut content = Content::new();
        perform_smart_edit(&mut content, Action::Edit(Edit::Insert('[')));
        assert_content(&content, "[]", 0, 1);

        let mut content = Content::new();
        perform_smart_edit(&mut content, Action::Edit(Edit::Insert('"')));
        assert_content(&content, "\"\"", 0, 1);
    }

    #[test]
    fn test_insert_normal_char() {
        let mut content = Content::new();
        perform_smart_edit(&mut content, Action::Edit(Edit::Insert('a')));
        assert_content(&content, "a", 0, 1);
    }

    #[test]
    fn test_default_action() {
        let mut content = Content::with_text("abc");
        perform_smart_edit(&mut content, Action::Move(Motion::Right));
        assert_content(&content, "abc", 0, 1);
    }

    #[test]
    fn test_enter_simple_indent() {
        let mut content = Content::with_text("    abc");
        content.perform(Action::Move(Motion::DocumentEnd));
        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "    abc\n    ", 1, 4);
    }

    #[test]
    fn test_enter_after_brace_indents() {
        let mut content = Content::with_text("  {");
        content.perform(Action::Move(Motion::DocumentEnd));
        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "  {\n  \t", 1, 3);
    }

    #[test]
    fn test_enter_after_bracket_indents() {
        let mut content = Content::with_text("[");
        content.perform(Action::Move(Motion::DocumentEnd));
        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "[\n\t", 1, 1);
    }

    #[test]
    fn test_enter_between_braces_splits() {
        let mut content = Content::with_text("  {}");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));
        content.perform(Action::Move(Motion::Right));
        content.perform(Action::Move(Motion::Right));

        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "  {\n  \t\n  }", 1, 3);
    }

    #[test]
    fn test_enter_between_brackets_splits() {
        let mut content = Content::with_text("[]");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));

        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "[\n\t\n]", 1, 1);
    }

    #[test]
    fn test_enter_with_multibyte_chars() {
        let mut content = Content::with_text("🚀 {");
        content.perform(Action::Move(Motion::DocumentEnd));
        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
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

        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
        assert!(content.line_count() > 1);
    }

    #[test]
    fn test_enter_between_mismatched_brackets() {
        let mut content = Content::with_text("{]");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));
        perform_smart_edit(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "{\n\t]", 1, 1);
    }
}
