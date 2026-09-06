use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    thread,
};

use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, DropDown, Entry,
    HeaderBar, Label, Notebook, Orientation, Paned, PolicyType, ScrolledWindow, Separator,
    TextView, gio, glib, prelude::*,
};
use pakpos::{
    models::{HeaderRow, HttpMethod, RequestBody, RequestDraft, RequestSnapshot},
    net::{ResponseData, execute},
};
use tokio::sync::oneshot;

#[derive(Clone)]
struct HeaderWidgets {
    row: GtkBox,
    enabled: CheckButton,
    name: Entry,
    value: Entry,
}

#[derive(Default)]
struct RequestState {
    next_id: Cell<u64>,
    active_id: Cell<Option<u64>>,
    cancel: RefCell<Option<oneshot::Sender<()>>>,
}

pub fn build(application: &Application) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Pakpos")
        .default_width(1000)
        .default_height(700)
        .build();

    let title = Label::builder().label("Pakpos").build();
    title.add_css_class("title");
    let header_bar = HeaderBar::builder().title_widget(&title).build();
    window.set_titlebar(Some(&header_bar));

    let root = Paned::builder()
        .orientation(Orientation::Horizontal)
        .position(220)
        .shrink_start_child(false)
        .resize_start_child(false)
        .build();
    root.set_start_child(Some(&build_sidebar()));

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

    let url = Entry::builder()
        .hexpand(true)
        .placeholder_text("https://api.example.com/resource")
        .build();
    url.add_css_class("monospace");
    url.set_tooltip_text(Some("Absolute HTTP or HTTPS URL"));
    let send = Button::with_label("Send");
    send.add_css_class("suggested-action");
    let cancel = Button::with_label("Cancel");
    cancel.set_sensitive(false);
    request_row.append(&method);
    request_row.append(&url);
    request_row.append(&send);
    request_row.append(&cancel);
    main.append(&request_row);

    let request_notebook = Notebook::new();
    let (headers_page, header_rows) = build_headers_page();
    request_notebook.append_page(&headers_page, Some(&Label::new(Some("Headers"))));
    let (body_page, body_mode, json_editor) = build_body_page();
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
    let response_headers = readonly_text_view();
    response_notebook.append_page(
        &scrolled(&response_headers),
        Some(&Label::new(Some("Headers"))),
    );
    main.append(&response_notebook);
    root.set_end_child(Some(&main));
    window.set_child(Some(&root));

    let state = Rc::new(RequestState::default());
    let send_request: Rc<dyn Fn()> = Rc::new({
        let method = method.clone();
        let url = url.clone();
        let body_mode = body_mode.clone();
        let json_editor = json_editor.clone();
        let header_rows = header_rows.clone();
        let send = send.clone();
        let cancel = cancel.clone();
        let response_summary = response_summary.clone();
        let response_body = response_body.clone();
        let response_headers = response_headers.clone();
        let state = state.clone();

        move || {
            if state.active_id.get().is_some() {
                return;
            }

            let Some(method_value) = HttpMethod::ALL.get(method.selected() as usize).copied()
            else {
                show_error(&response_summary, "Select a request method.");
                return;
            };
            let headers = header_rows
                .borrow()
                .iter()
                .map(|widgets| HeaderRow {
                    enabled: widgets.enabled.is_active(),
                    name: widgets.name.text().to_string(),
                    value: widgets.value.text().to_string(),
                })
                .collect();
            let body = if body_mode.selected() == 1 {
                RequestBody::Json(buffer_text(&json_editor))
            } else {
                RequestBody::None
            };
            let draft = RequestDraft {
                method: method_value,
                url: url.text().to_string(),
                headers,
                body,
            };
            let snapshot = match RequestSnapshot::try_from(draft) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    show_error(&response_summary, &error.to_string());
                    return;
                }
            };

            let request_id = state.next_id.get().wrapping_add(1);
            state.next_id.set(request_id);
            state.active_id.set(Some(request_id));
            let (cancel_sender, cancel_receiver) = oneshot::channel();
            state.cancel.replace(Some(cancel_sender));
            set_request_running(&send, &cancel, true);
            response_summary.remove_css_class("error");
            response_summary.set_text("Sending request…");
            response_body.buffer().set_text("");
            response_headers.buffer().set_text("");

            let (result_sender, result_receiver) = mpsc::channel();
            thread::spawn(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("Could not start the request worker: {error}"))
                    .and_then(|runtime| {
                        runtime
                            .block_on(execute(snapshot, cancel_receiver))
                            .map_err(|error| error.to_string())
                    });
                let _ = result_sender.send(result);
            });

            let state = state.clone();
            let send = send.clone();
            let cancel = cancel.clone();
            let response_summary = response_summary.clone();
            let response_body = response_body.clone();
            let response_headers = response_headers.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
                match result_receiver.try_recv() {
                    Ok(result) => {
                        if state.active_id.get() == Some(request_id) {
                            state.active_id.set(None);
                            state.cancel.replace(None);
                            set_request_running(&send, &cancel, false);
                            display_result(
                                result,
                                &response_summary,
                                &response_body,
                                &response_headers,
                            );
                        }
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        state.active_id.set(None);
                        state.cancel.replace(None);
                        set_request_running(&send, &cancel, false);
                        show_error(
                            &response_summary,
                            "The request worker stopped unexpectedly.",
                        );
                        glib::ControlFlow::Break
                    }
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
            if let Some(sender) = state.cancel.borrow_mut().take() {
                let _ = sender.send(());
            }
        }
    });

    let send_action = gio::SimpleAction::new("send", None);
    send_action.connect_activate(move |_, _| send_request());
    window.add_action(&send_action);
    application.set_accels_for_action("win.send", &["<Control>Return"]);

    window.connect_close_request({
        let state = state.clone();
        move |_| {
            if let Some(sender) = state.cancel.borrow_mut().take() {
                let _ = sender.send(());
            }
            glib::Propagation::Proceed
        }
    });

    window.present();
}

fn build_sidebar() -> GtkBox {
    let sidebar = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let heading = Label::builder()
        .label("Collection")
        .halign(Align::Start)
        .build();
    heading.add_css_class("heading");
    let scratch = Button::with_label("Scratch request");
    scratch.set_halign(Align::Fill);
    scratch.set_sensitive(false);
    sidebar.append(&heading);
    sidebar.append(&scratch);
    sidebar
}

fn build_headers_page() -> (GtkBox, Rc<RefCell<Vec<HeaderWidgets>>>) {
    let page = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let rows_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .build();
    let rows = Rc::new(RefCell::new(Vec::new()));
    add_header_row(&rows_box, &rows);
    let add = Button::with_label("Add header");
    add.set_halign(Align::Start);
    add.connect_clicked({
        let rows_box = rows_box.clone();
        let rows = rows.clone();
        move |_| add_header_row(&rows_box, &rows)
    });
    page.append(&rows_box);
    page.append(&add);
    (page, rows)
}

fn add_header_row(container: &GtkBox, rows: &Rc<RefCell<Vec<HeaderWidgets>>>) {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    let enabled = CheckButton::builder()
        .active(true)
        .tooltip_text("Send this header")
        .build();
    let name = Entry::builder()
        .placeholder_text("Header name")
        .hexpand(true)
        .build();
    let value = Entry::builder()
        .placeholder_text("Value")
        .hexpand(true)
        .build();
    name.add_css_class("monospace");
    value.add_css_class("monospace");
    let remove = Button::with_label("Remove");
    row.append(&enabled);
    row.append(&name);
    row.append(&value);
    row.append(&remove);
    container.append(&row);

    rows.borrow_mut().push(HeaderWidgets {
        row: row.clone(),
        enabled,
        name,
        value,
    });
    remove.connect_clicked({
        let rows = rows.clone();
        move |_| {
            if let Some(index) = rows.borrow().iter().position(|item| item.row == row) {
                let removed = rows.borrow_mut().remove(index);
                removed.row.unparent();
            }
        }
    });
}

fn build_body_page() -> (GtkBox, DropDown, TextView) {
    let page = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let mode = DropDown::from_strings(&["None", "JSON"]);
    mode.set_halign(Align::Start);
    let editor = TextView::builder()
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    let editor_scroll = scrolled(&editor);
    editor_scroll.set_min_content_height(150);
    editor_scroll.set_visible(false);
    mode.connect_selected_notify({
        let editor_scroll = editor_scroll.clone();
        move |mode| editor_scroll.set_visible(mode.selected() == 1)
    });
    page.append(&mode);
    page.append(&editor_scroll);
    (page, mode, editor)
}

fn readonly_text_view() -> TextView {
    TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build()
}

fn scrolled<W: IsA<gtk::Widget>>(child: &W) -> ScrolledWindow {
    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(child)
        .build()
}

fn buffer_text(view: &TextView) -> String {
    let buffer = view.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string()
}

fn set_request_running(send: &Button, cancel: &Button, running: bool) {
    send.set_sensitive(!running);
    cancel.set_sensitive(running);
}

fn show_error(summary: &Label, message: &str) {
    summary.set_text(message);
    summary.add_css_class("error");
}

fn display_result(
    result: Result<ResponseData, String>,
    summary: &Label,
    body: &TextView,
    headers: &TextView,
) {
    match result {
        Ok(response) => {
            summary.remove_css_class("error");
            summary.set_text(&response.summary());
            body.buffer().set_text(&response.display_body());
            headers.buffer().set_text(&response.display_headers());
        }
        Err(error) => {
            show_error(summary, &error);
            body.buffer().set_text("");
            headers.buffer().set_text("");
        }
    }
}
