use std::{cell::RefCell, rc::Rc};

use gtk::{
    Align, ApplicationWindow, Box as GtkBox, Button, CheckButton, DropDown, Entry,
    EventControllerFocus, FileDialog, Orientation, PolicyType, ScrolledWindow, TextView, gio, glib,
    prelude::*,
};
use pakpos::models::{HeaderRow, HttpMethod, MultipartField, MultipartValue, Request, RequestBody};
use sourceview5::prelude::{BufferExt, ViewExt};

use super::json_editor::{self, JsonEditorState};

pub(super) type AutosaveTrigger = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

#[derive(Clone)]
pub(super) struct HeaderWidgets {
    pub(super) row: GtkBox,
    pub(super) enabled: CheckButton,
    pub(super) name: Entry,
    pub(super) value: Entry,
}

#[derive(Clone)]
pub(super) struct MultipartWidgets {
    pub(super) row: GtkBox,
    pub(super) enabled: CheckButton,
    pub(super) name: Entry,
    pub(super) kind: DropDown,
    pub(super) value: Entry,
}

#[derive(Clone)]
pub(super) struct BodyWidgets {
    pub(super) mode: DropDown,
    pub(super) json_editor: sourceview5::View,
    pub(super) json_editor_state: Rc<JsonEditorState>,
    pub(super) multipart_box: GtkBox,
    pub(super) multipart_rows: Rc<RefCell<Vec<MultipartWidgets>>>,
    pub(super) autosave: AutosaveTrigger,
}

pub(super) type EditorWidgets = Rc<EditorWidgetHandles>;

pub(super) struct EditorWidgetHandles {
    pub(super) method: DropDown,
    pub(super) url: Entry,
    pub(super) headers_box: GtkBox,
    pub(super) header_rows: Rc<RefCell<Vec<HeaderWidgets>>>,
    pub(super) body: BodyWidgets,
}

pub(super) fn request_autosave(autosave: &AutosaveTrigger) {
    let trigger = autosave.borrow().clone();
    if let Some(trigger) = trigger {
        trigger();
    }
}

pub(super) fn autosave_on_blur<W: IsA<gtk::Widget>>(widget: &W, autosave: &AutosaveTrigger) {
    let focus = EventControllerFocus::new();
    focus.connect_leave({
        let autosave = autosave.clone();
        move |_| {
            let autosave = autosave.clone();
            glib::idle_add_local_once(move || request_autosave(&autosave));
        }
    });
    widget.add_controller(focus);
}

pub(super) fn build_headers_page(
    autosave: &AutosaveTrigger,
) -> (GtkBox, GtkBox, Rc<RefCell<Vec<HeaderWidgets>>>) {
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
    add_header_row(&rows_box, &rows, autosave);
    let add = Button::with_label("Add header");
    add.set_halign(Align::Start);
    add.connect_clicked({
        let rows_box = rows_box.clone();
        let rows = rows.clone();
        let autosave = autosave.clone();
        move |_| {
            add_header_row(&rows_box, &rows, &autosave);
            request_autosave(&autosave);
        }
    });
    page.append(&rows_box);
    page.append(&add);
    (page, rows_box, rows)
}

pub(super) fn add_header_row(
    container: &GtkBox,
    rows: &Rc<RefCell<Vec<HeaderWidgets>>>,
    autosave: &AutosaveTrigger,
) {
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
    autosave_on_blur(&name, autosave);
    autosave_on_blur(&value, autosave);
    enabled.connect_toggled({
        let autosave = autosave.clone();
        move |_| request_autosave(&autosave)
    });
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
        let container = container.downgrade();
        let rows = Rc::downgrade(rows);
        let row = row.downgrade();
        let autosave = autosave.clone();
        move |_| {
            let (Some(container), Some(rows), Some(row)) =
                (container.upgrade(), rows.upgrade(), row.upgrade())
            else {
                return;
            };
            let index = {
                let rows = rows.borrow();
                rows.iter().position(|item| item.row == row)
            };
            if let Some(index) = index {
                let removed = rows.borrow_mut().remove(index);
                container.remove(&removed.row);
                request_autosave(&autosave);
            }
        }
    });
}

pub(super) fn build_body_page(
    window: &ApplicationWindow,
    autosave: &AutosaveTrigger,
) -> (GtkBox, BodyWidgets) {
    let page = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let mode = DropDown::from_strings(&["None", "JSON", "Multipart"]);
    mode.set_halign(Align::Start);
    let language = sourceview5::LanguageManager::default().language("json");
    let editor_buffer = match language.as_ref() {
        Some(language) => sourceview5::Buffer::builder()
            .language(language)
            .highlight_syntax(true)
            .highlight_matching_brackets(true)
            .enable_undo(true)
            .build(),
        None => sourceview5::Buffer::builder()
            .highlight_matching_brackets(true)
            .enable_undo(true)
            .build(),
    };
    editor_buffer.set_max_undo_levels(100);
    configure_json_style_scheme(&editor_buffer);
    let editor = sourceview5::View::builder()
        .buffer(&editor_buffer)
        .auto_indent(true)
        .indent_on_tab(true)
        .indent_width(2)
        .tab_width(2)
        .insert_spaces_instead_of_tabs(true)
        .smart_backspace(true)
        .monospace(true)
        .hexpand(true)
        .vexpand(true)
        .wrap_mode(gtk::WrapMode::None)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    editor.space_drawer().set_enable_matrix(false);
    let json_editor_state = json_editor::configure(&editor);
    let editor_scroll = scrolled(&editor);
    editor_scroll.set_vexpand(true);
    autosave_on_blur(&editor, autosave);
    editor_scroll.set_min_content_height(150);
    editor_scroll.set_visible(false);

    let multipart_panel = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .build();
    let multipart_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .build();
    let multipart_rows = Rc::new(RefCell::new(Vec::new()));
    add_multipart_row(&multipart_box, &multipart_rows, window, autosave);
    let add = Button::with_label("Add field");
    add.set_halign(Align::Start);
    add.connect_clicked({
        let multipart_box = multipart_box.clone();
        let multipart_rows = multipart_rows.clone();
        let window = window.downgrade();
        let autosave = autosave.clone();
        move |_| {
            let Some(window) = window.upgrade() else {
                return;
            };
            add_multipart_row(&multipart_box, &multipart_rows, &window, &autosave);
            request_autosave(&autosave);
        }
    });
    multipart_panel.append(&multipart_box);
    multipart_panel.append(&add);
    let multipart_scroll = scrolled(&multipart_panel);
    multipart_scroll.set_vexpand(true);
    multipart_scroll.set_min_content_height(150);
    multipart_scroll.set_visible(false);

    mode.connect_selected_notify({
        let editor_scroll = editor_scroll.clone();
        let multipart_scroll = multipart_scroll.clone();
        let autosave = autosave.clone();
        move |mode| {
            editor_scroll.set_visible(mode.selected() == 1);
            multipart_scroll.set_visible(mode.selected() == 2);
            request_autosave(&autosave);
        }
    });
    page.append(&mode);
    page.append(&editor_scroll);
    page.append(&multipart_scroll);
    (
        page,
        BodyWidgets {
            mode,
            json_editor: editor,
            json_editor_state,
            multipart_box,
            multipart_rows,
            autosave: autosave.clone(),
        },
    )
}

fn configure_json_style_scheme(buffer: &sourceview5::Buffer) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    apply_json_style_scheme(buffer, &settings);
    settings.connect_gtk_application_prefer_dark_theme_notify({
        let buffer = buffer.downgrade();
        move |settings| {
            if let Some(buffer) = buffer.upgrade() {
                apply_json_style_scheme(&buffer, settings);
            }
        }
    });
    settings.connect_gtk_theme_name_notify({
        let buffer = buffer.downgrade();
        move |settings| {
            if let Some(buffer) = buffer.upgrade() {
                apply_json_style_scheme(&buffer, settings);
            }
        }
    });
}

fn apply_json_style_scheme(buffer: &sourceview5::Buffer, settings: &gtk::Settings) {
    let scheme_id = json_style_scheme_id(
        settings.is_gtk_application_prefer_dark_theme(),
        settings.gtk_theme_name().as_deref(),
    );
    if let Some(scheme) = sourceview5::StyleSchemeManager::default().scheme(scheme_id) {
        buffer.set_style_scheme(Some(&scheme));
    }
}

pub(super) fn json_style_scheme_id(prefer_dark: bool, theme_name: Option<&str>) -> &'static str {
    let theme_name_is_dark =
        theme_name.is_some_and(|name| name.to_ascii_lowercase().contains("dark"));
    if prefer_dark || theme_name_is_dark {
        "Adwaita-dark"
    } else {
        "Adwaita"
    }
}

pub(super) fn add_multipart_row(
    container: &GtkBox,
    rows: &Rc<RefCell<Vec<MultipartWidgets>>>,
    window: &ApplicationWindow,
    autosave: &AutosaveTrigger,
) {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    let enabled = CheckButton::builder()
        .active(true)
        .tooltip_text("Send this multipart field")
        .build();
    let name = Entry::builder()
        .placeholder_text("Field name")
        .hexpand(true)
        .build();
    let kind = DropDown::from_strings(&["Text", "File"]);
    kind.set_tooltip_text(Some("Multipart field type"));
    let value = Entry::builder()
        .placeholder_text("Text value")
        .hexpand(true)
        .build();
    let browse = Button::with_label("Choose…");
    browse.set_visible(false);
    let remove = Button::with_label("Remove");
    name.add_css_class("monospace");
    value.add_css_class("monospace");
    autosave_on_blur(&name, autosave);
    autosave_on_blur(&value, autosave);
    enabled.connect_toggled({
        let autosave = autosave.clone();
        move |_| request_autosave(&autosave)
    });

    kind.connect_selected_notify({
        let value = value.clone();
        let browse = browse.clone();
        let autosave = autosave.clone();
        move |kind| {
            let is_file = kind.selected() == 1;
            value.set_placeholder_text(Some(if is_file { "File path" } else { "Text value" }));
            browse.set_visible(is_file);
            request_autosave(&autosave);
        }
    });
    browse.connect_clicked({
        let value = value.clone();
        let window = window.downgrade();
        let autosave = autosave.clone();
        move |_| {
            let Some(window) = window.upgrade() else {
                return;
            };
            let dialog = FileDialog::builder()
                .title("Choose multipart file")
                .modal(true)
                .build();
            let value = value.clone();
            let autosave = autosave.clone();
            dialog.open(Some(&window), None::<&gio::Cancellable>, move |result| {
                if let Ok(file) = result
                    && let Some(path) = file.path()
                {
                    value.set_text(&path.to_string_lossy());
                    request_autosave(&autosave);
                }
            });
        }
    });

    row.append(&enabled);
    row.append(&name);
    row.append(&kind);
    row.append(&value);
    row.append(&browse);
    row.append(&remove);
    container.append(&row);
    rows.borrow_mut().push(MultipartWidgets {
        row: row.clone(),
        enabled,
        name,
        kind,
        value,
    });
    remove.connect_clicked({
        let container = container.downgrade();
        let rows = Rc::downgrade(rows);
        let row = row.downgrade();
        let autosave = autosave.clone();
        move |_| {
            let (Some(container), Some(rows), Some(row)) =
                (container.upgrade(), rows.upgrade(), row.upgrade())
            else {
                return;
            };
            let index = {
                let rows = rows.borrow();
                rows.iter().position(|item| item.row == row)
            };
            if let Some(index) = index {
                let removed = rows.borrow_mut().remove(index);
                container.remove(&removed.row);
                request_autosave(&autosave);
            }
        }
    });
}

pub(super) fn readonly_text_view() -> TextView {
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

pub(super) fn scrolled<W: IsA<gtk::Widget>>(child: &W) -> ScrolledWindow {
    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(child)
        .build()
}

pub(super) fn buffer_text(view: &impl IsA<TextView>) -> String {
    let buffer = view.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string()
}

pub(super) fn collect_request(
    method: &DropDown,
    url: &Entry,
    header_rows: &Rc<RefCell<Vec<HeaderWidgets>>>,
    body_widgets: &BodyWidgets,
) -> Result<Request, String> {
    let method = HttpMethod::ALL
        .get(method.selected() as usize)
        .copied()
        .ok_or_else(|| "Select a request method.".to_owned())?;
    let headers = header_rows
        .borrow()
        .iter()
        .map(|widgets| HeaderRow {
            enabled: widgets.enabled.is_active(),
            name: widgets.name.text().to_string(),
            value: widgets.value.text().to_string(),
        })
        .collect();
    let body = match body_widgets.mode.selected() {
        0 => RequestBody::None,
        1 => RequestBody::Json(buffer_text(&body_widgets.json_editor)),
        2 => RequestBody::Multipart(
            body_widgets
                .multipart_rows
                .borrow()
                .iter()
                .map(|widgets| MultipartField {
                    enabled: widgets.enabled.is_active(),
                    name: widgets.name.text().to_string(),
                    value: if widgets.kind.selected() == 1 {
                        MultipartValue::File(widgets.value.text().as_str().into())
                    } else {
                        MultipartValue::Text(widgets.value.text().to_string())
                    },
                })
                .collect(),
        ),
        _ => return Err("Select a request body mode.".to_owned()),
    };
    Ok(Request {
        method,
        url: url.text().to_string(),
        headers,
        body,
    })
}

pub(super) fn apply_request(
    request: &Request,
    method: &DropDown,
    url: &Entry,
    headers_box: &GtkBox,
    header_rows: &Rc<RefCell<Vec<HeaderWidgets>>>,
    body_widgets: &BodyWidgets,
) {
    let method_index = HttpMethod::ALL
        .iter()
        .position(|value| *value == request.method)
        .unwrap_or_default();
    method.set_selected(method_index as u32);
    url.set_text(&request.url);

    let old_rows = std::mem::take(&mut *header_rows.borrow_mut());
    for widgets in old_rows {
        headers_box.remove(&widgets.row);
    }
    if request.headers.is_empty() {
        add_header_row(headers_box, header_rows, &body_widgets.autosave);
    } else {
        for header in &request.headers {
            add_header_row(headers_box, header_rows, &body_widgets.autosave);
            if let Some(widgets) = header_rows.borrow().last() {
                widgets.enabled.set_active(header.enabled);
                widgets.name.set_text(&header.name);
                widgets.value.set_text(&header.value);
            }
        }
    }

    match &request.body {
        RequestBody::None => {
            body_widgets.mode.set_selected(0);
            json_editor::set_text(
                &body_widgets.json_editor,
                &body_widgets.json_editor_state,
                "",
            );
            reset_multipart_rows(body_widgets, &[]);
        }
        RequestBody::Json(body) => {
            body_widgets.mode.set_selected(1);
            json_editor::set_text(
                &body_widgets.json_editor,
                &body_widgets.json_editor_state,
                body,
            );
            reset_multipart_rows(body_widgets, &[]);
        }
        RequestBody::Multipart(fields) => {
            body_widgets.mode.set_selected(2);
            json_editor::set_text(
                &body_widgets.json_editor,
                &body_widgets.json_editor_state,
                "",
            );
            reset_multipart_rows(body_widgets, fields);
        }
    }
}

pub(super) fn reset_multipart_rows(body: &BodyWidgets, fields: &[MultipartField]) {
    let old_rows = std::mem::take(&mut *body.multipart_rows.borrow_mut());
    for widgets in old_rows {
        body.multipart_box.remove(&widgets.row);
    }

    // Imported cURL requests are applied to an existing window, so recover the
    // window from the row container instead of retaining a second owner reference.
    let Some(window) = body
        .multipart_box
        .root()
        .and_then(|root| root.downcast::<ApplicationWindow>().ok())
    else {
        return;
    };
    if fields.is_empty() {
        add_multipart_row(
            &body.multipart_box,
            &body.multipart_rows,
            &window,
            &body.autosave,
        );
        return;
    }
    for field in fields {
        add_multipart_row(
            &body.multipart_box,
            &body.multipart_rows,
            &window,
            &body.autosave,
        );
        if let Some(widgets) = body.multipart_rows.borrow().last() {
            widgets.enabled.set_active(field.enabled);
            widgets.name.set_text(&field.name);
            match &field.value {
                MultipartValue::Text(value) => {
                    widgets.kind.set_selected(0);
                    widgets.value.set_text(value);
                }
                MultipartValue::File(path) => {
                    widgets.kind.set_selected(1);
                    widgets.value.set_text(&path.to_string_lossy());
                }
            }
        }
    }
}
