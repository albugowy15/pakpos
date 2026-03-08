use iced::{
    Color, Element, Font, Length, Task, Theme, font, highlighter,
    keyboard::{Key, key::Named},
    widget::{
        button, center_x, column, pick_list, responsive, row, scrollable, space, table, text,
        text_editor, text_input,
    },
};
use uuid::Uuid;

use crate::ui::indent::handle_smart_indent;
use crate::{
    Error,
    models::{EDITOR_TABS, EditorTab, FieldKind, KeyValueField, METHODS, Method},
};
use crate::{models::request::Request, net::RequestTask};

#[derive(Default)]
pub struct App {
    method: Method,
    url: String,
    active_tab: EditorTab,
    requests: Vec<Request>,
    active_request_id: String,
    request_body: text_editor::Content,
    response_body: text_editor::Content,
    query_params: Vec<KeyValueField>,
    headers: Vec<KeyValueField>,
    loading: bool,
}

#[derive(Debug, Clone)]
pub enum AppMessage {
    MethodChanged(Method),
    UrlChanged(String),
    SubmitRequest,
    TabChanged(EditorTab),
    ActiveRequestChanged(String),
    RequestBodyEdited(text_editor::Action),
    RequestFinished(Result<String, Error>),
    ResponseBodyEdited(text_editor::Action),
    FieldKeyUpdated(FieldKind, String, String),
    FieldValueUpdated(FieldKind, String, String),
    AddField(FieldKind),
    RemoveField(FieldKind, String),
    AddRequest,
}

impl App {
    pub fn new() -> Self {
        let requests = vec![
            Request::new("First request".to_string()),
            Request::new("Second Request".to_string()),
            Request::new("Third Request".to_string()),
            Request::new("Fourth Request".to_string()),
            Request::new("Lorem dolor".to_string()),
            Request::new("Sit amet consecturs".to_string()),
        ];

        let active_request_id = requests[0].id.clone();

        Self {
            requests,
            active_request_id,
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
            AppMessage::AddRequest => Task::none(),
            AppMessage::MethodChanged(method) => {
                self.method = method;
                Task::none()
            }
            AppMessage::UrlChanged(url) => {
                self.url = url;
                Task::none()
            }
            AppMessage::SubmitRequest => {
                self.loading = true;
                self.response_body = text_editor::Content::new();

                let query_params: Vec<(String, String)> = self
                    .query_params
                    .iter()
                    .filter_map(|row| match (row.key.as_ref(), row.value.as_ref()) {
                        (Some(k), Some(v)) if !k.is_empty() => Some((k.clone(), v.clone())),
                        _ => None,
                    })
                    .collect();

                let headers: Vec<(String, String)> = self
                    .headers
                    .iter()
                    .filter_map(|row| match (row.key.as_ref(), row.value.as_ref()) {
                        (Some(k), Some(v)) if !k.is_empty() => Some((k.clone(), v.clone())),
                        _ => None,
                    })
                    .collect();

                let task = RequestTask::new(self.method, self.url.clone())
                    .body(self.request_body.text())
                    .query_params(query_params)
                    .headers(headers);

                Task::perform(task.execute(), AppMessage::RequestFinished)
            }
            AppMessage::TabChanged(tab) => {
                self.active_tab = tab;
                Task::none()
            }
            AppMessage::ActiveRequestChanged(id) => {
                self.active_request_id = id;
                Task::none()
            }
            AppMessage::RequestBodyEdited(action) => {
                handle_smart_indent(&mut self.request_body, action);
                Task::none()
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
                Task::none()
            }
            AppMessage::ResponseBodyEdited(action) => {
                handle_smart_indent(&mut self.response_body, action);
                Task::none()
            }
            AppMessage::FieldKeyUpdated(kind, id, val) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(row) = rows.iter_mut().find(|r| r.id == id) {
                    row.key = Some(val);
                }
                Task::none()
            }
            AppMessage::FieldValueUpdated(kind, id, val) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(row) = rows.iter_mut().find(|r| r.id == id) {
                    row.value = Some(val);
                }
                Task::none()
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
                Task::none()
            }
            AppMessage::RemoveField(kind, id) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(pos) = rows.iter().position(|row| row.id == id) {
                    rows.remove(pos);
                }
                Task::none()
            }
        }
    }

    fn render_kv_editor(&self, kind: FieldKind) -> Element<'_, AppMessage> {
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

            scrollable(
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
                .spacing(5),
            )
            .into()
        })
        .into()
    }

    pub fn view(&self) -> Element<'_, AppMessage> {
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

        let active_tab_content: Element<'_, AppMessage> = match self.active_tab {
            EditorTab::Body => responsive(move |size| {
                text_editor(&self.request_body)
                    .placeholder("Request Body")
                    .on_action(AppMessage::RequestBodyEdited)
                    .highlight("json", highlighter::Theme::SolarizedDark)
                    .key_binding(|event| {
                        if event.key == Key::Named(Named::Tab) {
                            Some(text_editor::Binding::Insert('\t'))
                        } else {
                            text_editor::Binding::from_key_press(event)
                        }
                    })
                    .size(14)
                    .padding(10)
                    .height(size.height)
                    .into()
            })
            .into(),
            EditorTab::Params => self.render_kv_editor(FieldKind::QueryParam),
            EditorTab::Headers => self.render_kv_editor(FieldKind::Header),
        };

        row!(
            column!(
                row!(
                    text("Demo request collection"),
                    button("+")
                        .style(button::primary)
                        .on_press(AppMessage::AddRequest)
                ),
                space::vertical().height(20),
                column(self.requests.iter().map(|item| {
                    let id = item.id.clone();
                    button(text(item.title.to_owned()).color(Color::WHITE).size(14))
                        .style(move |theme, status| {
                            if self.active_request_id == id {
                                return button::subtle(theme, status);
                            } else {
                                return button::text(theme, status);
                            }
                        })
                        .padding([8, 16])
                        .width(Length::Fill)
                        .on_press(AppMessage::ActiveRequestChanged(item.id.clone()))
                        .into()
                }),)
                .spacing(8),
            )
            .padding(10)
            .width(500),
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
                        .style(move |theme: &Theme, status| {
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
                    .height(600)
                    .highlight("json", highlighter::Theme::SolarizedDark)
                    .on_action(AppMessage::ResponseBodyEdited)
                    .key_binding(|event| {
                        if event.key == Key::Named(Named::Tab) {
                            Some(text_editor::Binding::Insert('\t'))
                        } else {
                            text_editor::Binding::from_key_press(event)
                        }
                    })
                    .size(14)
                    .padding(10)
            )
            .spacing(10)
            .padding(10)
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::{Action, Edit};

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
        let _ = app.update(AppMessage::FieldKeyUpdated(
            FieldKind::QueryParam,
            id.clone(),
            "name".to_owned(),
        ));
        let _ = app.update(AppMessage::FieldValueUpdated(
            FieldKind::QueryParam,
            id.clone(),
            "pikachu".to_owned(),
        ));
        assert_eq!(app.query_params[0].key.as_deref(), Some("name"));
        assert_eq!(app.query_params[0].value.as_deref(), Some("pikachu"));
    }

    #[test]
    fn test_update_header_key_value() {
        let mut app = App::new();
        let id = app.headers[0].id.clone();
        let _ = app.update(AppMessage::FieldKeyUpdated(
            FieldKind::Header,
            id.clone(),
            "X-Test".to_owned(),
        ));
        let _ = app.update(AppMessage::FieldValueUpdated(
            FieldKind::Header,
            id.clone(),
            "Value".to_owned(),
        ));
        assert_eq!(app.headers[0].key.as_deref(), Some("X-Test"));
        assert_eq!(app.headers[0].value.as_deref(), Some("Value"));
    }

    #[test]
    fn test_full_request_flow_e2e_simulation() {
        let mut app = App::new();

        // 1. Set URL
        let _ = app.update(AppMessage::UrlChanged(
            "https://httpbin.org/post".to_owned(),
        ));
        assert_eq!(app.url, "https://httpbin.org/post");

        // 2. Change Method to POST
        let _ = app.update(AppMessage::MethodChanged(Method::Post));
        assert_eq!(app.method, Method::Post);

        // 3. Add a Query Param
        let _ = app.update(AppMessage::AddField(FieldKind::QueryParam));
        let param_id = app.query_params[0].id.clone();
        let _ = app.update(AppMessage::FieldKeyUpdated(
            FieldKind::QueryParam,
            param_id.clone(),
            "debug".to_owned(),
        ));
        let _ = app.update(AppMessage::FieldValueUpdated(
            FieldKind::QueryParam,
            param_id.clone(),
            "true".to_owned(),
        ));

        // 4. Update default Header
        let header_id = app.headers[0].id.clone();
        let _ = app.update(AppMessage::FieldValueUpdated(
            FieldKind::Header,
            header_id.clone(),
            "application/json".to_owned(),
        ));

        // 5. Edit Body
        let _ = app.update(AppMessage::RequestBodyEdited(Action::Edit(Edit::Insert(
            '{',
        ))));
        let _ = app.update(AppMessage::RequestBodyEdited(Action::Edit(Edit::Insert(
            '}',
        ))));
        // Note: smart indent would have inserted {} and moved cursor left, but let's assume simple edits for simulation

        // 6. Click Send
        let _ = app.update(AppMessage::SubmitRequest);

        // Verify state during loading
        assert!(app.loading);
        assert!(app.response_body.text().is_empty());

        // 7. Simulate Response (Successful)
        let _ = app.update(AppMessage::RequestFinished(Ok(
            "{\"success\": true}".to_owned()
        )));

        assert!(!app.loading);
        assert!(app.response_body.text().contains("success"));
    }

    #[test]
    fn test_unhappy_path_empty_url_send_disabled() {
        let app = App::new();
        // The view logic handles disabling, but we check our message guard
        let submit_msg = if !app.url.is_empty() && !app.loading {
            Some(AppMessage::SubmitRequest)
        } else {
            None
        };
        assert!(submit_msg.is_none());
    }

    #[test]
    fn test_unhappy_path_request_error() {
        let mut app = App::new();
        let _ = app.update(AppMessage::UrlChanged("invalid-url".to_owned()));
        let _ = app.update(AppMessage::SubmitRequest);

        // Simulate an API error
        let _ = app.update(AppMessage::RequestFinished(Err(Error::APIError)));

        assert!(!app.loading);
        // Response should be empty or handle error display (currently we println but state remains clear)
        assert!(app.response_body.text().is_empty());
    }

    #[test]
    fn test_url_changed() {
        let mut app = App::new();
        let _ = app.update(AppMessage::UrlChanged("https://google.com".to_owned()));
        assert_eq!(app.url, "https://google.com");
    }

    #[test]
    fn test_method_changed() {
        let mut app = App::new();
        let _ = app.update(AppMessage::MethodChanged(Method::Post));
        assert_eq!(app.method, Method::Post);
    }

    #[test]
    fn test_tab_changed() {
        let mut app = App::new();
        let _ = app.update(AppMessage::TabChanged(EditorTab::Body));
        assert_eq!(app.active_tab, EditorTab::Body);
    }

    #[test]
    fn test_remove_non_existent_field() {
        let mut app = App::new();
        let initial_count = app.headers.len();
        let _ = app.update(AppMessage::RemoveField(
            FieldKind::Header,
            "non-existent".to_owned(),
        ));
        assert_eq!(app.headers.len(), initial_count);
    }

    #[test]
    fn test_update_non_existent_field() {
        let mut app = App::new();
        let _ = app.update(AppMessage::FieldKeyUpdated(
            FieldKind::Header,
            "non-existent".to_owned(),
            "key".to_owned(),
        ));
        // Should not panic and headers should remain unchanged
        assert_eq!(app.headers[0].key.as_deref(), Some("Content-Type"));
    }
}
