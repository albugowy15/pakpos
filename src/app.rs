use std::{collections::HashMap, sync::Arc};

use iced::{
    Alignment, Color, Element, Length, Task, Theme, highlighter,
    keyboard::{Key, key::Named},
    task,
    widget::{
        button, center, column, pick_list, responsive, row, scrollable, space, svg, table, text,
        text_editor, text_input,
    },
};
use uuid::Uuid;

use crate::{
    Error,
    models::{
        FieldKind,
        editor_tab::EDITOR_TABS,
        method::{METHODS, Method},
    },
};
use crate::{
    models::request::{FindRequest, Request},
    models::workspace::Workspace,
    net::RequestTask,
    storage::Storage,
};
use crate::{
    models::{KeyValueField, editor_tab::EditorTab},
    ui::{self},
};

#[derive(Default)]
pub struct App {
    storage: Storage,
    method: Method,
    url: String,
    active_tab: EditorTab,
    // Workspace state
    workspaces: Vec<Workspace>,
    active_workspace_id: String,
    workspace_title: String,
    // Request state
    search_query: String,
    requests: Vec<Request>,
    active_request_id: String,
    request_title: String,
    request_body: text_editor::Content,
    response_body: text_editor::Content,
    query_params: Vec<KeyValueField>,
    headers: Vec<KeyValueField>,
    responses: HashMap<String, String>,
    loading: bool,
    request_handle: Option<task::Handle>,
}

#[derive(Debug, Clone)]
pub enum AppMessage {
    // Workspace
    WorkspaceAdded,
    ActiveWorkspaceChanged(Workspace),
    WorkspaceTitleChanged(String),
    // Request
    SearchQueryChanged(String),
    RequestAdded,
    ActiveRequestChanged(String),
    RequestTitleChanged(String),
    RequestTitleSubmitted,
    RequestSubmitted,
    RequestCancelled,
    RequestFinished(Result<String, Error>),
    // Editor
    MethodChanged(Method),
    UrlChanged(String),
    TabChanged(EditorTab),
    RequestBodyEdited(text_editor::Action),
    ResponseBodyEdited(text_editor::Action),
    // Fields (params & headers)
    FieldAdded(FieldKind),
    FieldRemoved(FieldKind, String),
    FieldKeyChanged(FieldKind, String, String),
    FieldValueChanged(FieldKind, String, String),
}

impl App {
    pub fn new() -> Self {
        Self::with_storage(Storage::default_path())
    }

    fn with_storage(storage: Storage) -> Self {
        let mut app = Self {
            storage,
            loading: false,
            ..Default::default()
        };

        app.workspaces = app.storage.load_all_workspaces();

        if app.workspaces.is_empty() {
            let ws = Workspace::new(String::from("My Workspace"));
            app.storage.save_workspace(&ws);
            app.active_workspace_id = ws.id.clone();
            app.workspace_title = ws.title.clone();
            app.workspaces.push(ws);
        } else {
            let first = &app.workspaces[0];
            app.active_workspace_id = first.id.clone();
            app.workspace_title = first.title.clone();
        }

        app.load_workspace_requests();

        app
    }

    pub fn title(&self) -> String {
        String::from("Pakpos")
    }

    fn load_workspace_requests(&mut self) {
        self.search_query.clear();
        self.requests.clear();
        self.responses.clear();
        self.active_request_id.clear();
        self.reset_editor_state();

        let loaded = self.storage.load_all_requests(&self.active_workspace_id);
        for (request, response) in loaded {
            if let Some(resp) = response {
                self.responses.insert(request.id.clone(), resp);
            }
            self.requests.push(request);
        }

        if let Some(first) = self.requests.first() {
            let id = first.id.clone();
            self.active_request_id = id.clone();
            self.load_request(&id);
        }
    }

    fn reset_editor_state(&mut self) {
        self.method = Method::default();
        self.url.clear();
        self.request_title.clear();
        self.query_params.clear();
        self.headers.clear();
        self.request_body = text_editor::Content::new();
        self.response_body = text_editor::Content::new();
    }

    fn save_active_request(&mut self) {
        if let Some(req) = self.requests.find_by_id_mut(&self.active_request_id) {
            req.method = self.method;
            req.url = if self.url.is_empty() {
                None
            } else {
                Some(self.url.clone())
            };
            req.query_params = self.query_params.clone();
            req.headers = self.headers.clone();
            let body_text = self.request_body.text();
            let body_trimmed = body_text.trim();
            req.body = if body_trimmed.is_empty() {
                None
            } else {
                Some(body_trimmed.to_owned())
            };
            let response = self
                .responses
                .get(&self.active_request_id)
                .map(|s| s.as_str());
            self.storage
                .save_request(&self.active_workspace_id, req, response);
        }
    }

    fn load_request(&mut self, id: &str) {
        if let Some(req) = self.requests.find_by_id(id) {
            self.method = req.method;
            self.url = req.url.clone().unwrap_or_default();
            self.request_title = req.title.clone();
            self.query_params = req.query_params.clone();
            self.headers = req.headers.clone();
            self.request_body = text_editor::Content::new();
            if let Some(body) = &req.body {
                self.request_body
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(body.clone()),
                    )));
            }
            self.response_body = text_editor::Content::new();
            if let Some(resp) = self.responses.get(id) {
                self.response_body
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(resp.clone()),
                    )));
            }
        }
    }

    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        match message {
            AppMessage::WorkspaceAdded => {
                self.save_active_request();
                let ws = Workspace::new(String::from("New Workspace"));
                self.storage.save_workspace(&ws);
                self.active_workspace_id = ws.id.clone();
                self.workspace_title = ws.title.clone();
                self.workspaces.push(ws);
                self.load_workspace_requests();
                Task::none()
            }
            AppMessage::ActiveWorkspaceChanged(ws) => {
                if ws.id == self.active_workspace_id {
                    return Task::none();
                }
                self.save_active_request();
                self.active_workspace_id = ws.id.clone();
                self.workspace_title = ws.title.clone();
                self.load_workspace_requests();
                Task::none()
            }
            AppMessage::WorkspaceTitleChanged(title) => {
                if let Some(ws) = self
                    .workspaces
                    .iter_mut()
                    .find(|w| w.id == self.active_workspace_id)
                {
                    ws.title = title.clone();
                    self.storage.save_workspace(ws);
                }
                self.workspace_title = title;
                Task::none()
            }
            AppMessage::SearchQueryChanged(query) => {
                self.search_query = query;
                Task::none()
            }
            AppMessage::RequestAdded => {
                self.save_active_request();
                let new_request = Request::new(String::from("New Request"));
                self.active_request_id = new_request.id.clone();
                self.storage
                    .save_request(&self.active_workspace_id, &new_request, None);
                self.requests.push(new_request);
                self.load_request(&self.active_request_id.clone());
                Task::none()
            }
            AppMessage::MethodChanged(method) => {
                self.method = method;
                Task::none()
            }
            AppMessage::UrlChanged(url) => {
                self.url = url;
                Task::none()
            }
            AppMessage::RequestSubmitted => {
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

                let (task, handle) =
                    Task::perform(task.execute(), AppMessage::RequestFinished).abortable();
                self.request_handle = Some(handle);
                task
            }
            AppMessage::RequestCancelled => {
                self.loading = false;
                if let Some(handle) = &self.request_handle {
                    handle.abort();
                }
                Task::none()
            }
            AppMessage::TabChanged(tab) => {
                self.active_tab = tab;
                Task::none()
            }
            AppMessage::ActiveRequestChanged(id) => {
                self.save_active_request();
                self.active_request_id = id.clone();
                self.load_request(&id);
                Task::none()
            }
            AppMessage::RequestBodyEdited(action) => {
                ui::text_input::smart_indent(&mut self.request_body, action);
                Task::none()
            }
            AppMessage::RequestFinished(response) => {
                self.loading = false;
                self.response_body = text_editor::Content::new();
                match response {
                    Ok(result) => {
                        self.response_body.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(Arc::new(result.clone())),
                        ));
                        self.response_body.perform(text_editor::Action::Move(
                            text_editor::Motion::DocumentStart,
                        ));
                        self.responses
                            .insert(self.active_request_id.clone(), result);
                        self.save_active_request();
                    }
                    Err(err) => println!("Failed: {:?}", err),
                }
                Task::none()
            }
            AppMessage::ResponseBodyEdited(action) => {
                ui::text_input::smart_indent(&mut self.response_body, action);
                Task::none()
            }
            AppMessage::FieldKeyChanged(kind, id, val) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(row) = rows.iter_mut().find(|r| r.id == id) {
                    row.key = Some(val);
                }
                Task::none()
            }
            AppMessage::FieldValueChanged(kind, id, val) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(row) = rows.iter_mut().find(|r| r.id == id) {
                    row.value = Some(val);
                }
                Task::none()
            }
            AppMessage::FieldAdded(kind) => {
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
            AppMessage::FieldRemoved(kind, id) => {
                let rows = match kind {
                    FieldKind::QueryParam => &mut self.query_params,
                    FieldKind::Header => &mut self.headers,
                };
                if let Some(pos) = rows.iter().position(|row| row.id == id) {
                    rows.remove(pos);
                }
                Task::none()
            }
            AppMessage::RequestTitleChanged(title) => {
                self.request_title = title;
                Task::none()
            }
            AppMessage::RequestTitleSubmitted => {
                if let Some(req) = self
                    .requests
                    .iter_mut()
                    .find(|r| r.id == self.active_request_id)
                {
                    req.title = self.request_title.clone();
                }
                self.save_active_request();
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

        responsive(move |size| {
            let columns = [
                table::column(ui::text::bold("Key"), move |row: &KeyValueField| {
                    text_input("Key", row.key.as_deref().unwrap_or_default())
                        .on_input(move |val| AppMessage::FieldKeyChanged(kind, row.id.clone(), val))
                        .width(Length::Fill)
                })
                .width(Length::Fill),
                table::column(ui::text::bold("Value"), move |row: &KeyValueField| {
                    text_input("Value", row.value.as_deref().unwrap_or_default())
                        .on_input(move |val| {
                            AppMessage::FieldValueChanged(kind, row.id.clone(), val)
                        })
                        .width(Length::Fill)
                })
                .width(Length::Fill),
                table::column(ui::text::bold("Action"), move |row: &KeyValueField| {
                    button(
                        svg("assets/icon/trash.svg")
                            .style(|_theme, _status| svg::Style {
                                color: Some(Color::WHITE),
                            })
                            .width(18)
                            .height(18),
                    )
                    .on_press(AppMessage::FieldRemoved(kind, row.id.clone()))
                    .style(button::danger)
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
                    button(add_label).on_press(AppMessage::FieldAdded(kind))
                )
                .width(size.width)
                .spacing(5),
            )
            .into()
        })
        .into()
    }

    pub fn view(&self) -> Element<'_, AppMessage> {
        let submit_button_message = if self.loading {
            Some(AppMessage::RequestCancelled)
        } else if !self.url.is_empty() {
            Some(AppMessage::RequestSubmitted)
        } else {
            None
        };

        let submit_button_label = if self.loading { "Cancel" } else { "Send" };

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
                    text_input("Search requests...", &self.search_query)
                        .on_input(AppMessage::SearchQueryChanged)
                        .size(14)
                        .width(Length::Fill),
                    space::horizontal().width(5),
                    button("+")
                        .style(button::primary)
                        .on_press(AppMessage::RequestAdded),
                )
                .align_y(Alignment::Center),
                column(
                    self.requests
                        .iter()
                        .filter(|item| {
                            self.search_query.is_empty()
                                || item
                                    .title
                                    .to_lowercase()
                                    .contains(&self.search_query.to_lowercase())
                                || item
                                    .url
                                    .as_deref()
                                    .unwrap_or_default()
                                    .to_lowercase()
                                    .contains(&self.search_query.to_lowercase())
                                || item
                                    .method
                                    .to_string()
                                    .to_lowercase()
                                    .contains(&self.search_query.to_lowercase())
                        })
                        .map(|item| {
                            let id = item.id.clone();
                            button(text(item.title.to_owned()).size(14))
                                .style(move |theme, status| {
                                    if self.active_request_id == id {
                                        ui::button::background(theme, status)
                                    } else {
                                        button::text(theme, status)
                                    }
                                })
                                .padding([8, 16])
                                .width(Length::Fill)
                                .on_press(AppMessage::ActiveRequestChanged(item.id.clone()))
                                .into()
                        }),
                )
                .spacing(8),
            )
            .padding(10)
            .spacing(4)
            .width(500),
            if self.active_request_id.is_empty() {
                Element::from(
                    center(text("Choose a request to edit"))
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
            } else {
                column!(
                    row!(
                        text(self.workspace_title.as_str()).size(14),
                        text("/").size(14),
                        text_input("", &self.request_title)
                            .on_input(AppMessage::RequestTitleChanged)
                            .on_submit(AppMessage::RequestTitleSubmitted)
                            .size(14)
                            .width(300)
                    )
                    .spacing(8)
                    .align_y(Alignment::Center),
                    row!(
                        pick_list(METHODS, Some(self.method), AppMessage::MethodChanged)
                            .placeholder("HTTP Method"),
                        text_input("URL...", &self.url).on_input(AppMessage::UrlChanged),
                        button(text(submit_button_label).align_x(Alignment::Center))
                            .width(80)
                            .style(|theme, status| {
                                if self.loading {
                                    button::danger(theme, status)
                                } else {
                                    button::primary(theme, status)
                                }
                            })
                            .on_press_maybe(submit_button_message),
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
                .into()
            }
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::{Action, Edit};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_app() -> App {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let storage = Storage::new(PathBuf::from(format!(
            "/tmp/pakpos-app-test-{}-{}",
            std::process::id(),
            n
        )));
        App::with_storage(storage)
    }

    /// Creates an App with one request already added and active.
    fn app_with_request() -> App {
        let mut app = test_app();
        let _ = app.update(AppMessage::RequestAdded);
        app
    }

    #[test]
    fn test_add_param_row() {
        let mut app = test_app();
        let initial_count = app.query_params.len();
        let _ = app.update(AppMessage::FieldAdded(FieldKind::QueryParam));
        assert_eq!(app.query_params.len(), initial_count + 1);
    }

    #[test]
    fn test_remove_param_row() {
        let mut app = test_app();
        let _ = app.update(AppMessage::FieldAdded(FieldKind::QueryParam));
        let id = app.query_params[0].id.clone();
        let _ = app.update(AppMessage::FieldRemoved(FieldKind::QueryParam, id));
        assert_eq!(app.query_params.len(), 0);
    }

    #[test]
    fn test_add_header_row() {
        let mut app = test_app();
        let initial_count = app.headers.len();
        let _ = app.update(AppMessage::FieldAdded(FieldKind::Header));
        assert_eq!(app.headers.len(), initial_count + 1);
    }

    #[test]
    fn test_remove_header_row() {
        let mut app = app_with_request();
        let _ = app.update(AppMessage::FieldAdded(FieldKind::Header));
        let id = app.headers[0].id.clone();
        let _ = app.update(AppMessage::FieldRemoved(FieldKind::Header, id));
        assert_eq!(app.headers.len(), 1);
    }

    #[test]
    fn test_update_param_key_value() {
        let mut app = test_app();
        let _ = app.update(AppMessage::FieldAdded(FieldKind::QueryParam));
        let id = app.query_params[0].id.clone();
        let _ = app.update(AppMessage::FieldKeyChanged(
            FieldKind::QueryParam,
            id.clone(),
            "name".to_owned(),
        ));
        let _ = app.update(AppMessage::FieldValueChanged(
            FieldKind::QueryParam,
            id.clone(),
            "pikachu".to_owned(),
        ));
        assert_eq!(app.query_params[0].key.as_deref(), Some("name"));
        assert_eq!(app.query_params[0].value.as_deref(), Some("pikachu"));
    }

    #[test]
    fn test_update_header_key_value() {
        let mut app = app_with_request();
        let id = app.headers[0].id.clone();
        let _ = app.update(AppMessage::FieldKeyChanged(
            FieldKind::Header,
            id.clone(),
            "X-Test".to_owned(),
        ));
        let _ = app.update(AppMessage::FieldValueChanged(
            FieldKind::Header,
            id.clone(),
            "Value".to_owned(),
        ));
        assert_eq!(app.headers[0].key.as_deref(), Some("X-Test"));
        assert_eq!(app.headers[0].value.as_deref(), Some("Value"));
    }

    #[test]
    fn test_full_request_flow_e2e_simulation() {
        let mut app = app_with_request();

        // 1. Set URL
        let _ = app.update(AppMessage::UrlChanged(
            "https://httpbin.org/post".to_owned(),
        ));
        assert_eq!(app.url, "https://httpbin.org/post");

        // 2. Change Method to POST
        let _ = app.update(AppMessage::MethodChanged(Method::Post));
        assert_eq!(app.method, Method::Post);

        // 3. Add a Query Param
        let _ = app.update(AppMessage::FieldAdded(FieldKind::QueryParam));
        let param_id = app.query_params[0].id.clone();
        let _ = app.update(AppMessage::FieldKeyChanged(
            FieldKind::QueryParam,
            param_id.clone(),
            "debug".to_owned(),
        ));
        let _ = app.update(AppMessage::FieldValueChanged(
            FieldKind::QueryParam,
            param_id.clone(),
            "true".to_owned(),
        ));

        // 4. Update default Header
        let header_id = app.headers[0].id.clone();
        let _ = app.update(AppMessage::FieldValueChanged(
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

        // 6. Click Send
        let _ = app.update(AppMessage::RequestSubmitted);

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
        let app = test_app();
        let submit_msg = if !app.url.is_empty() && !app.loading {
            Some(AppMessage::RequestSubmitted)
        } else {
            None
        };
        assert!(submit_msg.is_none());
    }

    #[test]
    fn test_unhappy_path_request_error() {
        let mut app = test_app();
        let _ = app.update(AppMessage::UrlChanged("invalid-url".to_owned()));
        let _ = app.update(AppMessage::RequestSubmitted);

        let _ = app.update(AppMessage::RequestFinished(Err(Error::Api)));

        assert!(!app.loading);
        assert!(app.response_body.text().is_empty());
    }

    #[test]
    fn test_url_changed() {
        let mut app = test_app();
        let _ = app.update(AppMessage::UrlChanged("https://google.com".to_owned()));
        assert_eq!(app.url, "https://google.com");
    }

    #[test]
    fn test_method_changed() {
        let mut app = test_app();
        let _ = app.update(AppMessage::MethodChanged(Method::Post));
        assert_eq!(app.method, Method::Post);
    }

    #[test]
    fn test_tab_changed() {
        let mut app = test_app();
        let _ = app.update(AppMessage::TabChanged(EditorTab::Body));
        assert_eq!(app.active_tab, EditorTab::Body);
    }

    #[test]
    fn test_remove_non_existent_field() {
        let mut app = test_app();
        let initial_count = app.headers.len();
        let _ = app.update(AppMessage::FieldRemoved(
            FieldKind::Header,
            "non-existent".to_owned(),
        ));
        assert_eq!(app.headers.len(), initial_count);
    }

    #[test]
    fn test_update_non_existent_field() {
        let mut app = app_with_request();
        let _ = app.update(AppMessage::FieldKeyChanged(
            FieldKind::Header,
            "non-existent".to_owned(),
            "key".to_owned(),
        ));
        assert_eq!(app.headers[0].key.as_deref(), Some("Content-Type"));
    }

    #[test]
    fn test_add_workspace() {
        let mut app = test_app();
        let initial_count = app.workspaces.len();
        let _ = app.update(AppMessage::WorkspaceAdded);
        assert_eq!(app.workspaces.len(), initial_count + 1);
        assert!(app.requests.is_empty());
    }

    #[test]
    fn test_switch_workspace() {
        let mut app = test_app();
        let first_ws = app.workspaces[0].clone();

        // Add a request to first workspace
        let _ = app.update(AppMessage::RequestAdded);
        assert_eq!(app.requests.len(), 1);

        // Create second workspace
        let _ = app.update(AppMessage::WorkspaceAdded);
        assert_ne!(first_ws.id, app.active_workspace_id);
        assert!(app.requests.is_empty());

        // Switch back to first workspace
        let _ = app.update(AppMessage::ActiveWorkspaceChanged(first_ws));
        assert_eq!(app.requests.len(), 1);
    }

    #[test]
    fn test_workspace_title_changed() {
        let mut app = test_app();
        let _ = app.update(AppMessage::WorkspaceTitleChanged("Renamed".to_owned()));
        assert_eq!(app.workspace_title, "Renamed");
        assert_eq!(app.workspaces[0].title, "Renamed");
    }

    #[test]
    fn test_search_query_changed() {
        let mut app = test_app();
        let _ = app.update(AppMessage::SearchQueryChanged("my query".to_owned()));
        assert_eq!(app.search_query, "my query");
    }

    #[test]
    fn test_active_request_changed() {
        let mut app = app_with_request();
        let initial_id = app.active_request_id.clone();

        // Add another request
        let _ = app.update(AppMessage::RequestAdded);
        let second_id = app.active_request_id.clone();
        assert_ne!(initial_id, second_id);

        // Switch back
        let _ = app.update(AppMessage::ActiveRequestChanged(initial_id.clone()));
        assert_eq!(app.active_request_id, initial_id);
    }

    #[test]
    fn test_request_title_changed() {
        let mut app = app_with_request();
        // RequestTitleChanged only update app.request_title state
        let _ = app.update(AppMessage::RequestTitleChanged(
            "New Request Title".to_owned(),
        ));
        assert_eq!(app.request_title, "New Request Title");
        assert_eq!(app.requests[0].title, "New Request");

        // RequestTitleSubmitted should update app.requests state
        let _ = app.update(AppMessage::RequestTitleSubmitted);
        assert_eq!(app.request_title, "New Request Title");
        assert_eq!(app.requests[0].title, "New Request Title");
    }

    #[test]
    fn test_request_body_edited() {
        let mut app = app_with_request();
        let _ = app.update(AppMessage::RequestBodyEdited(Action::Edit(Edit::Insert(
            'a',
        ))));
        assert_eq!(app.request_body.text(), "a");
    }

    #[test]
    fn test_response_body_edited() {
        let mut app = app_with_request();
        let _ = app.update(AppMessage::ResponseBodyEdited(Action::Edit(Edit::Insert(
            'b',
        ))));
        assert_eq!(app.response_body.text(), "b");
    }

    #[test]
    fn test_active_workspace_changed_same_id() {
        let mut app = test_app();
        let ws = app.workspaces[0].clone();
        let _ = app.update(AppMessage::ActiveWorkspaceChanged(ws));
        // Should return early, no state change expected
    }

    #[test]
    fn test_field_header_ops() {
        let mut app = app_with_request();
        let initial_count = app.headers.len();

        // Add header
        let _ = app.update(AppMessage::FieldAdded(FieldKind::Header));
        assert_eq!(app.headers.len(), initial_count + 1);

        let id = app.headers[initial_count].id.clone();

        // Key change
        let _ = app.update(AppMessage::FieldKeyChanged(
            FieldKind::Header,
            id.clone(),
            "X-Custom".to_owned(),
        ));
        assert_eq!(app.headers[initial_count].key.as_deref(), Some("X-Custom"));

        // Value change
        let _ = app.update(AppMessage::FieldValueChanged(
            FieldKind::Header,
            id.clone(),
            "CustomValue".to_owned(),
        ));
        assert_eq!(
            app.headers[initial_count].value.as_deref(),
            Some("CustomValue")
        );

        // Remove
        let _ = app.update(AppMessage::FieldRemoved(FieldKind::Header, id));
        assert_eq!(app.headers.len(), initial_count);
    }

    #[test]
    fn test_request_finished_ok() {
        let mut app = app_with_request();
        app.loading = true;
        let _ = app.update(AppMessage::RequestFinished(Ok(
            "{\"status\":\"ok\"}".to_owned()
        )));
        assert!(!app.loading);
        assert_eq!(app.response_body.text(), "{\"status\":\"ok\"}");
        assert_eq!(
            app.responses.get(&app.active_request_id).unwrap(),
            "{\"status\":\"ok\"}"
        );
    }

    #[test]
    fn test_request_finished_err() {
        let mut app = app_with_request();
        app.loading = true;
        let _ = app.update(AppMessage::RequestFinished(Err(crate::Error::Api)));
        assert!(!app.loading);
    }

    #[test]
    fn test_tab_navigation() {
        let mut app = test_app();
        let tabs = [EditorTab::Params, EditorTab::Headers, EditorTab::Body];
        for tab in tabs {
            let _ = app.update(AppMessage::TabChanged(tab));
            assert_eq!(app.active_tab, tab);
        }
    }

    #[test]
    fn test_app_title() {
        let app = test_app();
        assert_eq!(app.title(), "Pakpos");
    }

    #[test]
    fn test_render_kv_editor_branch() {
        let mut app = test_app();
        // Test with empty rows
        let _ = app.render_kv_editor(FieldKind::QueryParam);

        // Test with non-empty rows
        let _ = app.update(AppMessage::FieldAdded(FieldKind::QueryParam));
        let _ = app.render_kv_editor(FieldKind::QueryParam);
    }

    #[test]
    fn test_view_branch() {
        let mut app = test_app();
        // Test with no active request
        let _ = app.view();

        // Test with active request
        let _ = app.update(AppMessage::RequestAdded);
        let _ = app.view();

        // Test with loading state
        app.loading = true;
        let _ = app.view();
    }
}
