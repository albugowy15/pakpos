use iced::{
    Element, Task,
    keyboard::key::Code,
    padding,
    widget::{button, column, pick_list, row, text, text_editor, text_input},
};

use crate::models::{EDITOR_TABS, EditorTab, HTTP_METHODS, HTTPMethod};

#[derive(Default)]
pub struct AppState {
    pub http_method: HTTPMethod,
    pub url: String,
    pub editor_tab: EditorTab,
    pub raw_body_content: text_editor::Content,
    pub response: text_editor::Content,
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
}

impl AppState {
    pub fn title(&self) -> String {
        String::from("Pakpos")
    }

    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        match message {
            AppMessage::HttpMethodSelected(http_method) => {
                self.http_method = http_method;
                Task::none()
            }
            AppMessage::UrlChanged(url) => {
                self.url = url;
                Task::none()
            }
            AppMessage::SendClicked => {
                let fetcher = Fetcher::new(
                    self.http_method,
                    self.url.clone(),
                    self.raw_body_content.text(),
                );
                Task::perform(fetcher.fetch(), AppMessage::Response)
            }
            AppMessage::EditorTabSelected(tab) => {
                self.editor_tab = tab;
                Task::none()
            }
            AppMessage::RawBodyContentEdit(action) => {
                self.raw_body_content.perform(action);
                Task::none()
            }
            AppMessage::Response(response) => {
                match response {
                    Ok(result) => {
                        self.response = text_editor::Content::with_text(&result);
                    }
                    Err(err) => println!("Failed: {:?}", err),
                }
                Task::none()
            }
            AppMessage::ResponseEdit(action) => {
                self.response.perform(action);
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, AppMessage> {
        let button_message_active = if !self.url.is_empty() {
            Some(AppMessage::SendClicked)
        } else {
            None
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
            text_editor(&self.raw_body_content)
                .placeholder("Request Body")
                .on_action(AppMessage::RawBodyContentEdit)
                .height(400)
                .highlight("json", iced::highlighter::Theme::SolarizedDark)
                .key_binding(|event| {
                    if event.physical_key == Code::Tab {
                        Some(text_editor::Binding::Insert('\t'))
                    } else {
                        text_editor::Binding::from_key_press(event)
                    }
                })
                .size(14)
                .padding(padding::left(10)),
            text_editor(&self.response)
                .height(400)
                .highlight("json", iced::highlighter::Theme::SolarizedDark)
                .on_action(AppMessage::ResponseEdit)
                .key_binding(|event| {
                    if event.physical_key == Code::Tab {
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
enum Error {
    APIError,
    SerdeError,
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::APIError
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::SerdeError
    }
}
