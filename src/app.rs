use iced::{
    Font, Length, font,
    keyboard::{Key, key::Named},
    padding,
    widget::{
        button, center_x, column, pick_list, responsive, row, table, text, text_editor, text_input,
    },
};
use uuid::Uuid;

use crate::models::{EDITOR_TABS, EditorTab, METHODS, Method};

#[derive(Default)]
pub struct App {
    method: Method,
    url: String,
    active_tab: EditorTab,
    request_body: text_editor::Content,
    response_body: text_editor::Content,
    query_params: Vec<KeyValueField>,
    headers: Vec<KeyValueField>,
    loading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    QueryParam,
    Header,
}

#[derive(Debug, Clone)]
pub enum AppMessage {
    MethodChanged(Method),
    UrlChanged(String),
    SubmitRequest,
    TabChanged(EditorTab),
    RequestBodyEdited(text_editor::Action),
    RequestFinished(Result<String, Error>),
    ResponseBodyEdited(text_editor::Action),

    FieldKeyUpdated(FieldKind, String, String),
    FieldValueUpdated(FieldKind, String, String),
    AddField(FieldKind),
    RemoveField(FieldKind, String),
}

#[derive(Debug, Clone)]
struct KeyValueField {
    id: String,
    key: Option<String>,
    value: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            headers: vec![KeyValueField {
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
            AppMessage::MethodChanged(method) => {
                self.method = method;
                iced::Task::none()
            }
            AppMessage::UrlChanged(url) => {
                self.url = url;
                iced::Task::none()
            }
            AppMessage::SubmitRequest => {
                self.loading = true;
                self.response_body = text_editor::Content::new();

                let query_params: Vec<(String, String)> = self
                    .query_params
                    .iter()
                    .filter_map(|row| {
                        match (row.key.as_ref(), row.value.as_ref()) {
                            (Some(k), Some(v)) if !k.is_empty() => Some((k.clone(), v.clone())),
                            _ => None,
                        }
                    })
                    .collect();

                let headers: Vec<(String, String)> = self
                    .headers
                    .iter()
                    .filter_map(|row| {
                        match (row.key.as_ref(), row.value.as_ref()) {
                            (Some(k), Some(v)) if !k.is_empty() => Some((k.clone(), v.clone())),
                            _ => None,
                        }
                    })
                    .collect();

                let task = RequestTask::new(self.method, self.url.clone())
                    .body(self.request_body.text())
                    .query_params(query_params)
                    .headers(headers);

                iced::Task::perform(task.execute(), AppMessage::RequestFinished)
            }
            AppMessage::TabChanged(tab) => {
                self.active_tab = tab;
                iced::Task::none()
            }
            AppMessage::RequestBodyEdited(action) => {
                handle_smart_indent(&mut self.request_body, action);
                iced::Task::none()
            }
            AppMessage::RequestFinished(response) => {
                self.loading = false;
                self.response_body = text_editor::Content::new();
                match response {
                    Ok(result) => {
                        println!("Response: {}", result);
                        self.response_body.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(std::sync::Arc::new(result)),
                        ));
                    }
                    Err(err) => println!("Failed: {:?}", err),
                }
                iced::Task::none()
            }
            AppMessage::ResponseBodyEdited(action) => {
                handle_smart_indent(&mut self.response_body, action);
                iced::Task::none()
            }
            AppMessage::FieldKeyUpdated(kind, id, val) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(row) = rows.iter_mut().find(|r| r.id == id) {
                    row.key = Some(val);
                }
                iced::Task::none()
            }
            AppMessage::FieldValueUpdated(kind, id, val) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(row) = rows.iter_mut().find(|r| r.id == id) {
                    row.value = Some(val);
                }
                iced::Task::none()
            }
            AppMessage::AddField(kind) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                rows.push(KeyValueField {
                    id: Uuid::new_v4().to_string(),
                    key: None,
                    value: None,
                });
                iced::Task::none()
            }
            AppMessage::RemoveField(kind, id) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(pos) = rows.iter().position(|row| row.id == id) {
                    rows.remove(pos);
                }
                iced::Task::none()
            }
        }
    }

    fn render_kv_editor(&self, kind: FieldKind) -> iced::Element<'_, AppMessage> {
        let rows = match kind {
            FieldKind::QueryParam => &self.query_params,
            FieldKind::Header => &self.headers,
        };
        let add_label = match kind {
            FieldKind::QueryParam => "Add Param",
            FieldKind::Header => "Add Header",
        };

        let bold = |header| {
            text(header).font(Font {
                weight: font::Weight::Bold,
                ..Font::DEFAULT
            })
        };

        responsive(move |size| {
            let columns = [
                table::column(bold("Key"), move |row: &KeyValueField| {
                    text_input("Key", row.key.as_deref().unwrap_or_default())
                        .on_input(move |val| AppMessage::FieldKeyUpdated(kind, row.id.clone(), val))
                        .width(Length::Fill)
                })
                .width(Length::Fill),
                table::column(bold("Value"), move |row: &KeyValueField| {
                    text_input("Value", row.value.as_deref().unwrap_or_default())
                        .on_input(move |val| {
                            AppMessage::FieldValueUpdated(kind, row.id.clone(), val)
                        })
                        .width(Length::Fill)
                })
                .width(Length::Fill),
                table::column("Action", move |row: &KeyValueField| {
                    center_x(
                        button("del")
                            .on_press(AppMessage::RemoveField(kind, row.id.clone()))
                            .style(button::danger),
                    )
                }),
            ];

            column!(
                {
                    if rows.is_empty() {
                        None
                    } else {
                        Some(table(columns, rows).width(size.width).padding(5))
                    }
                },
                button(add_label)
                    .on_press(AppMessage::AddField(kind))
                    .style(button::secondary)
            )
            .width(size.width)
            .spacing(5)
            .into()
        })
        .into()
    }

    pub fn view(&self) -> iced::Element<'_, AppMessage> {
        let submit_msg = if !self.url.is_empty() && !self.loading {
            Some(AppMessage::SubmitRequest)
        } else {
            None
        };

        let button_label = if self.loading {
            "Sending... ↻"
        } else {
            "Send"
        };

        let tab_editor_content_height = 400;

        let active_tab_content: iced::Element<'_, AppMessage> = match self.active_tab {
            EditorTab::Body => text_editor(&self.request_body)
                .placeholder("Request Body")
                .on_action(AppMessage::RequestBodyEdited)
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
            EditorTab::Params => self.render_kv_editor(FieldKind::QueryParam),
            EditorTab::Headers => self.render_kv_editor(FieldKind::Header),
        };

        column!(
            row!(
                pick_list(METHODS, Some(self.method), AppMessage::MethodChanged)
                    .placeholder("HTTP Method"),
                text_input("URL...", &self.url).on_input(AppMessage::UrlChanged),
                button(button_label).on_press_maybe(submit_msg),
            )
            .spacing(5),
            row(EDITOR_TABS.map(|tab| {
                button(text!("{tab}"))
                    .style(move |theme: &iced::Theme, status| {
                        if self.active_tab == tab {
                            button::subtle(theme, status)
                        } else {
                            button::text(theme, status)
                        }
                    })
                    .on_press(AppMessage::TabChanged(tab))
                    .into()
            }))
            .spacing(10),
            active_tab_content,
            text_editor(&self.response_body)
                .height(400)
                .highlight("json", iced::highlighter::Theme::SolarizedDark)
                .on_action(AppMessage::ResponseBodyEdited)
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

fn handle_smart_indent(content: &mut text_editor::Content, action: text_editor::Action) {
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

struct RequestTask {
    method: Method,
    url: String,
    body: String,
    query_params: Vec<(String, String)>,
    headers: Vec<(String, String)>,
}

impl RequestTask {
    fn new(method: Method, url: String) -> Self {
        Self {
            method,
            url,
            body: String::new(),
            query_params: Vec::new(),
            headers: Vec::new(),
        }
    }

    fn body(mut self, body: String) -> Self {
        self.body = body;
        self
    }

    fn query_params(mut self, params: Vec<(String, String)>) -> Self {
        self.query_params = params;
        self
    }

    fn headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    async fn execute(self) -> Result<String, Error> {
        let client = reqwest::Client::new();
        let mut req = match self.method {
            Method::Get => client.get(&self.url),
            Method::Post => client.post(&self.url),
            Method::Put => client.put(&self.url),
            Method::Delete => client.delete(&self.url),
            Method::Patch => client.patch(&self.url),
            Method::Head => client.head(&self.url),
        };

        if !self.query_params.is_empty() {
            req = req.query(&self.query_params);
        }

        for (key, val) in self.headers {
            req = req.header(key, val);
        }

        if !self.body.is_empty() {
            req = req.body(self.body.clone());
        }

        let result = req.send().await?;
        let response = result.text().await?;

        let parsed: serde_json::Value = match serde_json::from_str(&response) {
            Ok(val) => val,
            Err(_) => return Ok(response),
        };

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
        handle_smart_indent(&mut content, Action::Edit(Edit::Insert('{')));
        assert_content(&content, "{}", 0, 1);

        let mut content = Content::new();
        handle_smart_indent(&mut content, Action::Edit(Edit::Insert('[')));
        assert_content(&content, "[]", 0, 1);

        let mut content = Content::new();
        handle_smart_indent(&mut content, Action::Edit(Edit::Insert('"')));
        assert_content(&content, "\"\"", 0, 1);
    }

    #[test]
    fn test_insert_normal_char() {
        let mut content = Content::new();
        handle_smart_indent(&mut content, Action::Edit(Edit::Insert('a')));
        assert_content(&content, "a", 0, 1);
    }

    #[test]
    fn test_default_action() {
        let mut content = Content::with_text("abc");
        handle_smart_indent(&mut content, Action::Move(Motion::Right));
        assert_content(&content, "abc", 0, 1);
    }

    #[test]
    fn test_enter_simple_indent() {
        let mut content = Content::with_text("    abc");
        content.perform(Action::Move(Motion::DocumentEnd));
        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "    abc\n    ", 1, 4);
    }

    #[test]
    fn test_enter_after_brace_indents() {
        let mut content = Content::with_text("  {");
        content.perform(Action::Move(Motion::DocumentEnd));
        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "  {\n  \t", 1, 3);
    }

    #[test]
    fn test_enter_after_bracket_indents() {
        let mut content = Content::with_text("[");
        content.perform(Action::Move(Motion::DocumentEnd));
        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "[\n\t", 1, 1);
    }

    #[test]
    fn test_enter_between_braces_splits() {
        let mut content = Content::with_text("  {}");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));
        content.perform(Action::Move(Motion::Right));
        content.perform(Action::Move(Motion::Right));

        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "  {\n  \t\n  }", 1, 3);
    }

    #[test]
    fn test_enter_between_brackets_splits() {
        let mut content = Content::with_text("[]");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));

        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "[\n\t\n]", 1, 1);
    }

    #[test]
    fn test_enter_with_multibyte_chars() {
        let mut content = Content::with_text("🚀 {");
        content.perform(Action::Move(Motion::DocumentEnd));
        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
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

        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert!(content.line_count() > 1);
    }

    #[test]
    fn test_enter_between_mismatched_brackets() {
        let mut content = Content::with_text("{]");
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Right));
        handle_smart_indent(&mut content, Action::Edit(Edit::Enter));
        assert_content(&content, "{\n\t]", 1, 1);
    }

    #[test]
    fn test_add_param_row() {
        let mut app = App::new();
        let initial_count = app.query_params.len();
        let _ = app.update(AppMessage::AddField(FieldKind::QueryParam));
        assert_eq!(app.query_params.len(), initial_count + 1);
    }

    #[test]
    fn test_remove_param_row() {
        let mut app = App::new();
        let _ = app.update(AppMessage::AddField(FieldKind::QueryParam));
        let id = app.query_params[0].id.clone();
        let _ = app.update(AppMessage::RemoveField(FieldKind::QueryParam, id));
        assert_eq!(app.query_params.len(), 0);
    }

    #[test]
    fn test_add_header_row() {
        let mut app = App::new();
        let initial_count = app.headers.len();
        let _ = app.update(AppMessage::AddField(FieldKind::Header));
        assert_eq!(app.headers.len(), initial_count + 1);
    }

    #[test]
    fn test_remove_header_row() {
        let mut app = App::new();
        let _ = app.update(AppMessage::AddField(FieldKind::Header));
        let id = app.headers[0].id.clone();
        let _ = app.update(AppMessage::RemoveField(FieldKind::Header, id));
        assert_eq!(app.headers.len(), 1);
    }

    #[test]
    fn test_update_param_key_value() {
        let mut app = App::new();
        let _ = app.update(AppMessage::AddField(FieldKind::QueryParam));
        let id = app.query_params[0].id.clone();
        let _ = app.update(AppMessage::FieldKeyUpdated(FieldKind::QueryParam, id.clone(), "name".to_owned()));
        let _ = app.update(AppMessage::FieldValueUpdated(FieldKind::QueryParam, id.clone(), "pikachu".to_owned()));
        assert_eq!(app.query_params[0].key.as_deref(), Some("name"));
        assert_eq!(app.query_params[0].value.as_deref(), Some("pikachu"));
    }

    #[test]
    fn test_update_header_key_value() {
        let mut app = App::new();
        let id = app.headers[0].id.clone();
        let _ = app.update(AppMessage::FieldKeyUpdated(FieldKind::Header, id.clone(), "X-Test".to_owned()));
        let _ = app.update(AppMessage::FieldValueUpdated(FieldKind::Header, id.clone(), "Value".to_owned()));
        assert_eq!(app.headers[0].key.as_deref(), Some("X-Test"));
        assert_eq!(app.headers[0].value.as_deref(), Some("Value"));
    }

    #[test]
    fn test_request_task_builder() {
        let task = RequestTask::new(Method::Post, "https://example.com".to_string())
            .body("hello".to_string())
            .query_params(vec![("a".to_owned(), "b".to_owned())])
            .headers(vec![("c".to_owned(), "d".to_owned())]);

        assert_eq!(task.method, Method::Post);
        assert_eq!(task.url, "https://example.com");
        assert_eq!(task.body, "hello");
        assert_eq!(task.query_params, vec![("a".to_owned(), "b".to_owned())]);
        assert_eq!(task.headers, vec![("c".to_owned(), "d".to_owned())]);
    }
}
