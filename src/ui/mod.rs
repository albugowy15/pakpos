use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Deref,
    rc::Rc,
};

use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, DropDown, Entry, HeaderBar,
    Label, ListBox, ListBoxRow, MenuButton, Notebook, Orientation, Paned, ScrolledWindow,
    Separator, TextView, gio, glib, prelude::*,
};
use pakpos::{
    app::{Action, AppEvent, AppState, EffectOutput},
    collections::CollectionSummary,
    models::HttpMethod,
    net::ResponseData,
};

use crate::runtime::EffectRunner;

mod dialogs;
mod editor;
mod flow;
mod sidebar;

#[cfg(test)]
mod tests;

use self::editor::{
    AutosaveTrigger, apply_request, autosave_on_blur, build_body_page, build_headers_page,
    collect_request, readonly_text_view, request_autosave, scrolled,
};
use self::flow::{autosave_current_collection, capture_active_request, collection_is_dirty};
use self::sidebar::{build_sidebar, setup_collection_actions};

type SidebarWidgets = Rc<SidebarWidgetHandles>;

struct SidebarWidgetHandles {
    root: GtkBox,
    collection_picker: DropDown,
    collection_choices: Rc<RefCell<Vec<CollectionSummary>>>,
    new_collection: Button,
    new_request: Button,
    search: Entry,
    applied_search: Rc<RefCell<String>>,
    requests: ListBox,
    request_rows: Rc<RefCell<HashMap<uuid::Uuid, ListBoxRow>>>,
    status: Label,
    autosave: AutosaveTrigger,
}

#[derive(Default)]
struct RequestState {
    app: AppState,
    effects: Rc<EffectRunner>,
    allow_close: Cell<bool>,
    syncing_collection_picker: Cell<bool>,
    syncing_request_list: Cell<bool>,
    applying_editor: Cell<bool>,
}

impl Deref for RequestState {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

pub fn build(application: &Application) {
    let autosave = AutosaveTrigger::default();
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Pakpos")
        .default_width(1000)
        .default_height(700)
        .build();

    let header_bar = HeaderBar::builder().build();
    window.set_titlebar(Some(&header_bar));

    let root = Paned::builder()
        .orientation(Orientation::Horizontal)
        .position(220)
        .shrink_start_child(false)
        .resize_start_child(false)
        .build();
    let sidebar = build_sidebar(autosave.clone());
    header_bar.pack_start(&sidebar.collection_picker);
    header_bar.pack_start(&sidebar.new_collection);
    root.set_start_child(Some(&sidebar.root));

    let main = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    let request_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    let method_labels = HttpMethod::ALL.map(HttpMethod::as_str);
    let method = DropDown::from_strings(&method_labels);
    method.set_tooltip_text(Some("HTTP request method"));
    method.connect_selected_notify({
        let autosave = autosave.clone();
        move |_| request_autosave(&autosave)
    });

    let url = Entry::builder()
        .hexpand(true)
        .placeholder_text("https://api.example.com/resource")
        .build();
    url.add_css_class("monospace");
    url.set_tooltip_text(Some("Absolute HTTP or HTTPS URL"));
    autosave_on_blur(&url, &autosave);
    let send = Button::with_label("Send");
    send.add_css_class("suggested-action");
    let request_actions = gio::Menu::new();
    request_actions.append(Some("Copy as cURL"), Some("win.copy-curl"));
    request_actions.append(Some("Paste cURL"), Some("win.paste-curl"));
    let send_menu = MenuButton::builder()
        .icon_name("pan-down-symbolic")
        .menu_model(&request_actions)
        .tooltip_text("More request actions")
        .build();
    send_menu.add_css_class("suggested-action");
    let send_group = GtkBox::new(Orientation::Horizontal, 0);
    send_group.add_css_class("linked");
    send_group.append(&send);
    send_group.append(&send_menu);
    let cancel = Button::with_label("Cancel");
    cancel.set_visible(false);
    request_row.append(&method);
    request_row.append(&url);
    request_row.append(&send_group);
    request_row.append(&cancel);
    main.append(&request_row);

    let request_notebook = Notebook::new();
    let (headers_page, headers_box, header_rows) = build_headers_page(&autosave);
    request_notebook.append_page(&headers_page, Some(&Label::new(Some("Headers"))));
    let (body_page, body) = build_body_page(&window, &autosave);
    request_notebook.append_page(&body_page, Some(&Label::new(Some("Body"))));
    main.append(&request_notebook);
    main.append(&Separator::new(Orientation::Horizontal));

    let response_summary = Label::builder()
        .label("Send a request to see its response.")
        .halign(Align::Start)
        .selectable(true)
        .wrap(true)
        .build();
    main.append(&response_summary);

    let response_notebook = Notebook::new();
    response_notebook.set_vexpand(true);
    let response_body = readonly_text_view();
    response_notebook.append_page(&scrolled(&response_body), Some(&Label::new(Some("Body"))));
    let response_raw = readonly_text_view();
    let response_raw_page = scrolled(&response_raw);
    response_raw_page.set_visible(false);
    response_notebook.append_page(&response_raw_page, Some(&Label::new(Some("Raw"))));
    let response_headers = readonly_text_view();
    response_notebook.append_page(
        &scrolled(&response_headers),
        Some(&Label::new(Some("Headers"))),
    );
    main.append(&response_notebook);
    root.set_end_child(Some(&main));
    window.set_child(Some(&root));

    let state = Rc::new(RequestState::default());
    let editor_widgets = Rc::new(editor::EditorWidgetHandles {
        method: method.clone(),
        url: url.clone(),
        headers_box: headers_box.clone(),
        header_rows: header_rows.clone(),
        body: body.clone(),
    });
    setup_collection_actions(
        application,
        &window,
        &sidebar,
        &editor_widgets,
        &state,
        &sidebar.status,
    );
    let send_request: Rc<dyn Fn()> = Rc::new({
        let method = method.clone();
        let url = url.clone();
        let body = body.clone();
        let header_rows = header_rows.clone();
        let send_group = send_group.downgrade();
        let cancel = cancel.downgrade();
        let response_summary = response_summary.clone();
        let response_body = response_body.clone();
        let response_raw = response_raw.clone();
        let response_raw_page = response_raw_page.clone();
        let response_headers = response_headers.clone();
        let state = state.clone();

        move || {
            let (Some(send_group), Some(cancel)) = (send_group.upgrade(), cancel.upgrade()) else {
                return;
            };
            if state.active_request_id.get().is_some() {
                return;
            }

            let request = match collect_request(&method, &url, &header_rows, &body) {
                Ok(request) => request,
                Err(error) => {
                    show_error(&response_summary, &error);
                    return;
                }
            };

            let update = state.update(Action::SendRequest(request));
            let Some(effect) = update.effect else {
                return;
            };
            set_request_running(&send_group, &cancel, true);
            response_summary.remove_css_class("error");
            response_summary.set_text("Sending request…");
            response_body.buffer().set_text("");
            response_raw.buffer().set_text("");
            response_raw_page.set_visible(false);
            response_headers.buffer().set_text("");

            let state = state.clone();
            let send_group = send_group.clone();
            let cancel = cancel.clone();
            let response_summary = response_summary.clone();
            let response_body = response_body.clone();
            let response_raw = response_raw.clone();
            let response_raw_page = response_raw_page.clone();
            let response_headers = response_headers.clone();
            let effects = state.effects.clone();
            effects.run(effect, move |output| {
                let EffectOutput::RequestExecuted { id, result } = output else {
                    return;
                };
                if state.update(Action::RequestCompleted { id }).accepted {
                    set_request_running(&send_group, &cancel, false);
                    display_result(
                        result,
                        &response_summary,
                        &response_body,
                        &response_raw,
                        &response_raw_page,
                        &response_headers,
                    );
                }
            });
        }
    });

    send.connect_clicked({
        let send_request = send_request.clone();
        move |_| send_request()
    });
    cancel.connect_clicked({
        let state = state.clone();
        move |_| {
            if let Some(effect) = state.update(Action::CancelRequest).effect {
                state.effects.run(effect, |_| {});
            }
        }
    });

    let copy_curl_action = gio::SimpleAction::new("copy-curl", None);
    copy_curl_action.connect_activate({
        let method = method.clone();
        let url = url.clone();
        let header_rows = header_rows.clone();
        let body = body.clone();
        let state = state.clone();
        let response_summary = response_summary.clone();
        let clipboard = gtk::prelude::WidgetExt::display(&window).clipboard();
        move |_, _| {
            let result = collect_request(&method, &url, &header_rows, &body).and_then(|request| {
                let AppEvent::CurlExported(result) =
                    state.update(Action::ExportCurl(request.into())).event
                else {
                    return Err("Could not export the current request.".to_owned());
                };
                result
            });
            match result {
                Ok(command) => {
                    clipboard.set_text(&command);
                    show_message(&response_summary, "Copied the current request as cURL.");
                }
                Err(error) => show_error(&response_summary, &error),
            }
        }
    });
    window.add_action(&copy_curl_action);

    let paste_curl_action = gio::SimpleAction::new("paste-curl", None);
    paste_curl_action.connect_activate({
        let method = method.clone();
        let url = url.clone();
        let headers_box = headers_box.clone();
        let header_rows = header_rows.clone();
        let body = body.clone();
        let state = state.clone();
        let response_summary = response_summary.clone();
        let clipboard = gtk::prelude::WidgetExt::display(&window).clipboard();
        move |_, _| {
            clipboard.read_text_async(None::<&gio::Cancellable>, {
                let method = method.clone();
                let url = url.clone();
                let headers_box = headers_box.clone();
                let header_rows = header_rows.clone();
                let body = body.clone();
                let state = state.clone();
                let response_summary = response_summary.clone();
                move |result| match result {
                    Ok(Some(text)) => {
                        match state.update(Action::ImportCurl(text.to_string())).event {
                            AppEvent::CurlImported(Ok(import)) => {
                                state.applying_editor.set(true);
                                apply_request(
                                    &import.request,
                                    &method,
                                    &url,
                                    &headers_box,
                                    &header_rows,
                                    &body,
                                );
                                state.applying_editor.set(false);
                                request_autosave(&body.autosave);
                                let message = if import.warnings.is_empty() {
                                    "Pasted the cURL request.".to_owned()
                                } else {
                                    format!(
                                        "Pasted the cURL request. {}",
                                        import.warnings.join(" ")
                                    )
                                };
                                show_message(&response_summary, &message);
                            }
                            AppEvent::CurlImported(Err(error)) => {
                                show_error(&response_summary, &error)
                            }
                            _ => {
                                show_error(&response_summary, "Could not import the cURL request.")
                            }
                        }
                    }
                    Ok(None) => show_error(&response_summary, "The clipboard has no text."),
                    Err(error) => show_error(
                        &response_summary,
                        &format!("Could not read the clipboard: {error}"),
                    ),
                }
            });
        }
    });
    window.add_action(&paste_curl_action);

    let send_action = gio::SimpleAction::new("send", None);
    send_action.connect_activate(move |_, _| send_request());
    window.add_action(&send_action);
    application.set_accels_for_action("win.send", &["<Control>Return"]);

    window.connect_close_request({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor_widgets.clone();
        let collection_status = sidebar.status.clone();
        move |window| {
            if state.allow_close.get() {
                cancel_active_request(&state);
                return glib::Propagation::Proceed;
            }
            capture_active_request(&state, &sidebar, &editor);
            let has_dirty_collection = collection_is_dirty(&state);
            if !has_dirty_collection {
                cancel_active_request(&state);
                return glib::Propagation::Proceed;
            }
            state.update(Action::CloseRequested);
            if !state.collection_busy.get() {
                autosave_current_collection(&state, &sidebar, &editor, &collection_status, window);
            }
            glib::Propagation::Stop
        }
    });

    window.present();
    url.grab_focus();
}

fn set_request_running(send_group: &GtkBox, cancel: &Button, running: bool) {
    send_group.set_visible(!running);
    cancel.set_visible(running);
}

fn cancel_active_request(state: &Rc<RequestState>) {
    if let Some(effect) = state.update(Action::CancelRequest).effect {
        state.effects.run(effect, |_| {});
    }
}

fn show_error(summary: &Label, message: &str) {
    summary.set_text(message);
    summary.add_css_class("error");
}

fn show_message(summary: &Label, message: &str) {
    summary.remove_css_class("error");
    summary.set_text(message);
}

fn display_result(
    result: Result<ResponseData, String>,
    summary: &Label,
    body: &TextView,
    raw: &TextView,
    raw_page: &ScrolledWindow,
    headers: &TextView,
) {
    match result {
        Ok(response) => {
            summary.remove_css_class("error");
            summary.set_text(&response.summary());
            body.buffer().set_text(&response.display_body());
            if let Some(raw_body) = response.display_raw_body() {
                raw.buffer().set_text(&raw_body);
                raw_page.set_visible(true);
            } else {
                raw.buffer().set_text("");
                raw_page.set_visible(false);
            }
            headers.buffer().set_text(&response.display_headers());
        }
        Err(error) => {
            show_error(summary, &error);
            body.buffer().set_text("");
            raw.buffer().set_text("");
            raw_page.set_visible(false);
            headers.buffer().set_text("");
        }
    }
}
