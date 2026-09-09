use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
    thread,
};

use gtk::{
    AlertDialog, Align, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    DropDown, Entry, EventControllerFocus, FileDialog, GestureClick, HeaderBar, Label, MenuButton,
    Notebook, Orientation, Paned, PolicyType, Popover, PropagationPhase, ScrolledWindow, Separator,
    StringList, TextView, Window, gdk, gio, glib, prelude::*,
};
use pakpos::{
    collections::{CollectionNode, CollectionNodeKind, CollectionRequest, CollectionSummary},
    curl::{from_command, to_command},
    models::{HeaderRow, HttpMethod, MultipartField, MultipartValue, Request, RequestBody},
    net::{ResponseData, execute},
    storage::CollectionStore,
};
use tokio::sync::oneshot;
use uuid::Uuid;

type AutosaveTrigger = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

#[derive(Clone)]
struct HeaderWidgets {
    row: GtkBox,
    enabled: CheckButton,
    name: Entry,
    value: Entry,
}

#[derive(Clone)]
struct MultipartWidgets {
    row: GtkBox,
    enabled: CheckButton,
    name: Entry,
    kind: DropDown,
    value: Entry,
}

#[derive(Clone)]
struct BodyWidgets {
    mode: DropDown,
    json_editor: TextView,
    multipart_box: GtkBox,
    multipart_rows: Rc<RefCell<Vec<MultipartWidgets>>>,
    autosave: AutosaveTrigger,
}

#[derive(Clone)]
struct EditorWidgets {
    method: DropDown,
    url: Entry,
    headers_box: GtkBox,
    header_rows: Rc<RefCell<Vec<HeaderWidgets>>>,
    body: BodyWidgets,
}

#[derive(Clone)]
struct SidebarWidgets {
    root: GtkBox,
    collection_picker: DropDown,
    collection_choices: Rc<RefCell<Vec<CollectionSummary>>>,
    new_collection: Button,
    search: Entry,
    applied_search: Rc<RefCell<String>>,
    request_area: ScrolledWindow,
    requests: GtkBox,
    status: Label,
    autosave: AutosaveTrigger,
}

#[derive(Clone)]
struct EditorRequest {
    node: CollectionNode,
    request: Option<Request>,
    saved_node: Option<CollectionNode>,
    saved_request: Option<Request>,
}

impl EditorRequest {
    fn node_is_dirty(&self) -> bool {
        self.saved_node.as_ref() != Some(&self.node)
    }

    fn request_is_dirty(&self) -> bool {
        self.saved_request.as_ref() != self.request.as_ref()
    }

    fn is_dirty(&self) -> bool {
        self.node_is_dirty() || self.request_is_dirty()
    }
}

struct CollectionSession {
    summary: CollectionSummary,
    saved_summary: Option<CollectionSummary>,
    _folders: Vec<CollectionNode>,
    requests: Vec<EditorRequest>,
    deleted_nodes: Vec<Uuid>,
    active_request: Option<Uuid>,
}

#[derive(Clone, PartialEq, Eq)]
struct PendingCollectionSave {
    summary: CollectionSummary,
    changed_nodes: Vec<CollectionNode>,
    changed_requests: Vec<CollectionRequest>,
    deleted_nodes: Vec<Uuid>,
}

impl CollectionSession {
    fn is_dirty(&self) -> bool {
        self.saved_summary.as_ref() != Some(&self.summary)
            || !self.deleted_nodes.is_empty()
            || self.requests.iter().any(EditorRequest::is_dirty)
    }
}

#[derive(Default)]
struct RequestState {
    next_id: Cell<u64>,
    active_id: Cell<Option<u64>>,
    cancel: RefCell<Option<oneshot::Sender<()>>>,
    collection: RefCell<Option<CollectionSession>>,
    collection_busy: Cell<bool>,
    allow_close: Cell<bool>,
    close_after_autosave: Cell<bool>,
    after_autosave: RefCell<Option<Rc<dyn Fn()>>>,
    autosave_requested: Cell<bool>,
    syncing_collection_picker: Cell<bool>,
    applying_editor: Cell<bool>,
}

pub fn build(application: &Application) {
    let autosave = AutosaveTrigger::default();
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
    let sidebar = build_sidebar(autosave.clone());
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
    let editor_widgets = EditorWidgets {
        method: method.clone(),
        url: url.clone(),
        headers_box: headers_box.clone(),
        header_rows: header_rows.clone(),
        body: body.clone(),
    };
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
        let send_group = send_group.clone();
        let cancel = cancel.clone();
        let response_summary = response_summary.clone();
        let response_body = response_body.clone();
        let response_raw = response_raw.clone();
        let response_raw_page = response_raw_page.clone();
        let response_headers = response_headers.clone();
        let state = state.clone();

        move || {
            if state.active_id.get().is_some() {
                return;
            }

            let request = match collect_request(&method, &url, &header_rows, &body) {
                Ok(request) => request,
                Err(error) => {
                    show_error(&response_summary, &error);
                    return;
                }
            };

            let request_id = state.next_id.get().wrapping_add(1);
            state.next_id.set(request_id);
            state.active_id.set(Some(request_id));
            let (cancel_sender, cancel_receiver) = oneshot::channel();
            state.cancel.replace(Some(cancel_sender));
            set_request_running(&send_group, &cancel, true);
            response_summary.remove_css_class("error");
            response_summary.set_text("Sending request…");
            response_body.buffer().set_text("");
            response_raw.buffer().set_text("");
            response_raw_page.set_visible(false);
            response_headers.buffer().set_text("");

            let (result_sender, result_receiver) = mpsc::channel();
            thread::spawn(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("Could not start the request worker: {error}"))
                    .and_then(|runtime| {
                        runtime
                            .block_on(execute(request, cancel_receiver))
                            .map_err(|error| error.to_string())
                    });
                let _ = result_sender.send(result);
            });

            let state = state.clone();
            let send_group = send_group.clone();
            let cancel = cancel.clone();
            let response_summary = response_summary.clone();
            let response_body = response_body.clone();
            let response_raw = response_raw.clone();
            let response_raw_page = response_raw_page.clone();
            let response_headers = response_headers.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
                match result_receiver.try_recv() {
                    Ok(result) => {
                        if state.active_id.get() == Some(request_id) {
                            state.active_id.set(None);
                            state.cancel.replace(None);
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
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        state.active_id.set(None);
                        state.cancel.replace(None);
                        set_request_running(&send_group, &cancel, false);
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

    let copy_curl_action = gio::SimpleAction::new("copy-curl", None);
    copy_curl_action.connect_activate({
        let method = method.clone();
        let url = url.clone();
        let header_rows = header_rows.clone();
        let body = body.clone();
        let response_summary = response_summary.clone();
        let clipboard = gtk::prelude::WidgetExt::display(&window).clipboard();
        move |_, _| {
            let result = collect_request(&method, &url, &header_rows, &body)
                .and_then(|request| to_command(request).map_err(|error| error.to_string()));
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
                    Ok(Some(text)) => match from_command(&text) {
                        Ok(import) => {
                            state.applying_editor.set(true);
                            apply_request(
                                import.request,
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
                                format!("Pasted the cURL request. {}", import.warnings.join(" "))
                            };
                            show_message(&response_summary, &message);
                        }
                        Err(error) => show_error(&response_summary, &error.to_string()),
                    },
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
        let window = window.clone();
        let sidebar = sidebar.clone();
        let editor = editor_widgets.clone();
        let collection_status = sidebar.status.clone();
        move |_| {
            if state.allow_close.get() {
                if let Some(sender) = state.cancel.borrow_mut().take() {
                    let _ = sender.send(());
                }
                return glib::Propagation::Proceed;
            }
            capture_active_request(&state, &sidebar, &editor);
            let has_dirty_collection = collection_is_dirty(&state);
            if !has_dirty_collection {
                if let Some(sender) = state.cancel.borrow_mut().take() {
                    let _ = sender.send(());
                }
                return glib::Propagation::Proceed;
            }
            state.close_after_autosave.set(true);
            if !state.collection_busy.get() {
                autosave_current_collection(&state, &sidebar, &editor, &collection_status, &window);
            }
            glib::Propagation::Stop
        }
    });

    window.present();
}

fn build_sidebar(autosave: AutosaveTrigger) -> SidebarWidgets {
    let sidebar = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let collection_row = GtkBox::new(Orientation::Horizontal, 4);
    let collection_picker = DropDown::from_strings(&[]);
    collection_picker.set_hexpand(true);
    collection_picker.set_tooltip_text(Some("Active collection"));
    let new_collection = Button::builder()
        .label("+")
        .tooltip_text("Create collection")
        .build();
    collection_row.append(&collection_picker);
    collection_row.append(&new_collection);
    let request_heading = Label::builder()
        .label("Requests")
        .halign(Align::Start)
        .build();
    request_heading.add_css_class("heading");
    let requests = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .build();
    let request_scroll = scrolled(&requests);
    request_scroll.set_vexpand(true);
    let search = Entry::builder()
        .placeholder_text("Search requests")
        .tooltip_text("Search request names; press Enter or leave the field to apply")
        .build();
    let status = Label::builder()
        .halign(Align::Start)
        .wrap(true)
        .selectable(true)
        .build();
    status.add_css_class("dim-label");
    sidebar.append(&collection_row);
    sidebar.append(&request_heading);
    sidebar.append(&search);
    sidebar.append(&request_scroll);
    sidebar.append(&status);
    SidebarWidgets {
        root: sidebar,
        collection_picker,
        collection_choices: Rc::new(RefCell::new(Vec::new())),
        new_collection,
        search,
        applied_search: Rc::new(RefCell::new(String::new())),
        request_area: request_scroll,
        requests,
        status,
        autosave,
    }
}

fn setup_collection_actions(
    application: &Application,
    window: &ApplicationWindow,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    state: &Rc<RequestState>,
    response_summary: &Label,
) {
    let new_action = gio::SimpleAction::new("new-collection", None);
    new_action.connect_activate({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move |_, _| {
            let action: Rc<dyn Fn()> = Rc::new({
                let state = state.clone();
                let sidebar = sidebar.clone();
                let editor = editor.clone();
                let response_summary = response_summary.clone();
                let window = window.clone();
                move || {
                    show_create_collection_dialog(
                        &window,
                        &state,
                        &sidebar,
                        &editor,
                        &response_summary,
                    );
                }
            });
            continue_after_autosave(
                &window,
                &state,
                &sidebar,
                &editor,
                &response_summary,
                action,
            );
        }
    });
    window.add_action(&new_action);

    let focus_collections_action = gio::SimpleAction::new("focus-collections", None);
    focus_collections_action.connect_activate({
        let collection_picker = sidebar.collection_picker.clone();
        move |_, _| {
            collection_picker.grab_focus();
        }
    });
    window.add_action(&focus_collections_action);

    application.set_accels_for_action("win.new-collection", &["<Control>n"]);
    application.set_accels_for_action("win.focus-collections", &["<Control>o"]);

    sidebar.new_collection.connect_clicked({
        let window = window.clone();
        move |_| {
            let _ = gtk::prelude::WidgetExt::activate_action(&window, "new-collection", None);
        }
    });

    sidebar.collection_picker.connect_selected_notify({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move |picker| {
            if state.syncing_collection_picker.get() {
                return;
            }
            if state.collection_busy.get() {
                sync_collection_picker(&state, &sidebar);
                return;
            }
            let Some(collection) = sidebar
                .collection_choices
                .borrow()
                .get(picker.selected() as usize)
                .cloned()
            else {
                return;
            };
            if state
                .collection
                .borrow()
                .as_ref()
                .is_some_and(|session| session.summary.id == collection.id)
            {
                return;
            }
            let action: Rc<dyn Fn()> = Rc::new({
                let state = state.clone();
                let sidebar = sidebar.clone();
                let editor = editor.clone();
                let response_summary = response_summary.clone();
                move || {
                    load_collection(
                        collection.clone(),
                        &state,
                        &sidebar,
                        &editor,
                        &response_summary,
                    );
                }
            });
            continue_after_autosave(
                &window,
                &state,
                &sidebar,
                &editor,
                &response_summary,
                action,
            );
        }
    });

    sidebar.search.connect_activate({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |_| apply_request_search(&state, &sidebar, &editor)
    });
    let search_focus = EventControllerFocus::new();
    search_focus.connect_leave({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |_| apply_request_search(&state, &sidebar, &editor)
    });
    sidebar.search.add_controller(search_focus);

    install_sidebar_context_menu(window, state, sidebar, editor, response_summary);
    refresh_collection_choices(state, sidebar, editor, response_summary);

    sidebar.autosave.replace(Some(Rc::new({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move || {
            if !state.applying_editor.get() {
                state.autosave_requested.set(true);
                if !state.collection_busy.get() {
                    autosave_current_collection(
                        &state,
                        &sidebar,
                        &editor,
                        &response_summary,
                        &window,
                    );
                }
            }
        }
    })));
    glib::timeout_add_local(std::time::Duration::from_millis(30), {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move || {
            if state.autosave_requested.get() && !state.collection_busy.get() {
                autosave_current_collection(&state, &sidebar, &editor, &response_summary, &window);
            }
            glib::ControlFlow::Continue
        }
    });
}

fn continue_after_autosave(
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
    action: Rc<dyn Fn()>,
) {
    if state.collection_busy.get() {
        sync_collection_picker(state, sidebar);
        show_message(
            response_summary,
            "Please wait for the current collection operation.",
        );
        return;
    }
    capture_active_request(state, sidebar, editor);
    if !collection_is_dirty(state) {
        action();
        return;
    }
    state.after_autosave.replace(Some(action));
    autosave_current_collection(state, sidebar, editor, response_summary, window);
}

fn refresh_collection_choices(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    _editor: &EditorWidgets,
    response_summary: &Label,
) {
    if state.collection_busy.replace(true) {
        state.autosave_requested.set(true);
        return;
    }
    state.autosave_requested.set(false);
    run_storage_task(
        || {
            let store = CollectionStore::open_default().map_err(|error| error.to_string())?;
            store.list_collections().map_err(|error| error.to_string())
        },
        {
            let state = state.clone();
            let sidebar = sidebar.clone();
            let response_summary = response_summary.clone();
            move |result: Result<Vec<CollectionSummary>, String>| {
                state.collection_busy.set(false);
                match result {
                    Ok(collections) => {
                        let active_id = state
                            .collection
                            .borrow()
                            .as_ref()
                            .map(|session| session.summary.id);
                        let labels = collections
                            .iter()
                            .map(|collection| {
                                format!("{} · {}", collection.name, &collection.id.to_string()[..8])
                            })
                            .collect::<Vec<_>>();
                        let label_refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
                        let model = StringList::new(&label_refs);
                        let selected = active_id
                            .and_then(|id| {
                                collections
                                    .iter()
                                    .position(|collection| collection.id == id)
                            })
                            .map_or(u32::MAX, |index| index as u32);
                        state.syncing_collection_picker.set(true);
                        sidebar.collection_picker.set_model(Some(&model));
                        sidebar.collection_picker.set_selected(selected);
                        sidebar
                            .collection_picker
                            .set_sensitive(!collections.is_empty());
                        sidebar.collection_choices.replace(collections);
                        state.syncing_collection_picker.set(false);
                    }
                    Err(error) => show_error(&response_summary, &error),
                }
            }
        },
    );
}

fn show_create_collection_dialog(
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    let dialog = Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Create collection")
        .default_width(380)
        .resizable(false)
        .build();
    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    let prompt = Label::builder()
        .label("Collection name")
        .halign(Align::Start)
        .build();
    let name = Entry::builder()
        .placeholder_text("Collection name")
        .activates_default(true)
        .build();
    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    let create = Button::with_label("Create Collection");
    create.add_css_class("suggested-action");
    create.set_sensitive(false);
    actions.append(&cancel);
    actions.append(&create);
    content.append(&prompt);
    content.append(&name);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&create));

    name.connect_changed({
        let create = create.clone();
        move |name| create.set_sensitive(!name.text().trim().is_empty())
    });
    cancel.connect_clicked({
        let dialog = dialog.clone();
        move |_| dialog.close()
    });
    create.connect_clicked({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let dialog = dialog.clone();
        let name = name.clone();
        move |_| {
            let collection_name = name.text().trim().to_owned();
            if collection_name.is_empty() || state.collection_busy.replace(true) {
                return;
            }
            dialog.close();
            let collection = CollectionSummary::new(collection_name);
            show_message(&response_summary, "Creating collection…");
            run_storage_task(
                {
                    let collection = collection.clone();
                    move || {
                        let mut store =
                            CollectionStore::open_default().map_err(|error| error.to_string())?;
                        store
                            .save_collection(&collection, &[], &[], &[])
                            .map_err(|error| error.to_string())
                    }
                },
                {
                    let state = state.clone();
                    let sidebar = sidebar.clone();
                    let editor = editor.clone();
                    let response_summary = response_summary.clone();
                    move |result| {
                        state.collection_busy.set(false);
                        match result {
                            Ok(()) => {
                                state.collection.replace(Some(CollectionSession {
                                    summary: collection.clone(),
                                    saved_summary: Some(collection),
                                    _folders: Vec::new(),
                                    requests: Vec::new(),
                                    deleted_nodes: Vec::new(),
                                    active_request: None,
                                }));
                                sidebar.search.set_text("");
                                sidebar.applied_search.replace(String::new());
                                clear_request_editor(&sidebar, &editor);
                                render_request_buttons(&state, &sidebar, &editor);
                                show_message(&response_summary, "Created the collection.");
                                refresh_collection_choices(
                                    &state,
                                    &sidebar,
                                    &editor,
                                    &response_summary,
                                );
                            }
                            Err(error) => show_error(&response_summary, &error),
                        }
                    }
                },
            );
        }
    });
    dialog.present();
    name.grab_focus();
}

fn install_sidebar_context_menu(
    _window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    let popover = Popover::builder().has_arrow(true).build();
    popover.set_parent(&sidebar.request_area);
    let menu = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();
    let new_request = Button::with_label("New HTTP Request");
    new_request.add_css_class("flat");
    menu.append(&new_request);
    popover.set_child(Some(&menu));
    new_request.connect_clicked({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            if add_collection_request(&state, &sidebar, &editor) {
                show_message(&response_summary, "Created HTTP Request.");
            } else {
                show_error(&response_summary, "Create or select a collection first.");
            }
        }
    });
    let gesture = GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(PropagationPhase::Bubble);
    gesture.connect_pressed({
        let popover = popover.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        }
    });
    sidebar.request_area.add_controller(gesture);
}

fn sync_collection_picker(state: &Rc<RequestState>, sidebar: &SidebarWidgets) {
    let active_id = state
        .collection
        .borrow()
        .as_ref()
        .map(|session| session.summary.id);
    let selected = active_id
        .and_then(|id| {
            sidebar
                .collection_choices
                .borrow()
                .iter()
                .position(|collection| collection.id == id)
        })
        .map_or(u32::MAX, |index| index as u32);
    state.syncing_collection_picker.set(true);
    sidebar.collection_picker.set_selected(selected);
    state.syncing_collection_picker.set(false);
}

fn attach_request_context_menu(
    request_button: &Button,
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let popover = Popover::builder().has_arrow(true).build();
    popover.set_parent(request_button);
    let menu = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();
    let duplicate = context_menu_button("Duplicate");
    let delete = context_menu_button("Delete");
    let rename = context_menu_button("Rename");
    let copy_curl = context_menu_button("Copy as Curl");
    menu.append(&duplicate);
    menu.append(&delete);
    menu.append(&rename);
    menu.append(&copy_curl);
    popover.set_child(Some(&menu));

    duplicate.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |_| {
            popover.popdown();
            duplicate_request(request_id, &state, &sidebar, &editor, &sidebar.status);
        }
    });
    delete.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let request_button = request_button.clone();
        move |_| {
            popover.popdown();
            let Some(window) = request_button
                .root()
                .and_then(|root| root.downcast::<ApplicationWindow>().ok())
            else {
                return;
            };
            confirm_delete_request(request_id, &window, &state, &sidebar, &editor);
        }
    });
    rename.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let request_button = request_button.clone();
        move |_| {
            popover.popdown();
            let Some(window) = request_button
                .root()
                .and_then(|root| root.downcast::<ApplicationWindow>().ok())
            else {
                return;
            };
            show_rename_request_dialog(request_id, &window, &state, &sidebar, &editor);
        }
    });
    copy_curl.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let clipboard = gtk::prelude::WidgetExt::display(request_button).clipboard();
        move |_| {
            popover.popdown();
            copy_request_as_curl(
                request_id,
                &state,
                &sidebar,
                &editor,
                &sidebar.status,
                &clipboard,
            );
        }
    });

    let gesture = GestureClick::new();
    gesture.set_button(3);
    gesture.connect_pressed({
        let popover = popover.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        }
    });
    request_button.add_controller(gesture);
}

fn context_menu_button(label: &str) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("flat");
    button.set_halign(Align::Fill);
    button
}

fn show_rename_request_dialog(
    request_id: Uuid,
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let Some(current_name) = state.collection.borrow().as_ref().and_then(|session| {
        session
            .requests
            .iter()
            .find(|request| request.node.id == request_id)
            .map(|request| request.node.name.clone())
    }) else {
        return;
    };
    let dialog = Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename request")
        .default_width(380)
        .resizable(false)
        .build();
    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    let name = Entry::builder()
        .text(current_name)
        .activates_default(true)
        .build();
    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.set_halign(Align::End);
    let done = Button::with_label("Done");
    done.add_css_class("suggested-action");
    actions.append(&done);
    content.append(&Label::new(Some("Request name")));
    content.append(&name);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&done));
    name.connect_changed({
        let done = done.clone();
        move |name| done.set_sensitive(!name.text().trim().is_empty())
    });
    let commit_rename: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let name = name.clone();
        move || {
            let new_name = name.text().trim().to_owned();
            if new_name.is_empty() {
                return;
            }
            let changed = if let Some(session) = state.collection.borrow_mut().as_mut()
                && let Some(request) = session
                    .requests
                    .iter_mut()
                    .find(|request| request.node.id == request_id)
                && request.node.name != new_name
            {
                request.node.name = new_name;
                true
            } else {
                false
            };
            if changed {
                render_request_buttons(&state, &sidebar, &editor);
                request_autosave(&sidebar.autosave);
            }
        }
    });
    let rename_focus = EventControllerFocus::new();
    rename_focus.connect_leave({
        let commit_rename = commit_rename.clone();
        move |_| commit_rename()
    });
    name.add_controller(rename_focus);
    done.connect_clicked({
        let commit_rename = commit_rename.clone();
        let dialog = dialog.clone();
        move |_| {
            commit_rename();
            dialog.close();
        }
    });
    dialog.connect_close_request({
        let commit_rename = commit_rename.clone();
        move |_| {
            commit_rename();
            glib::Propagation::Proceed
        }
    });
    dialog.present();
    name.grab_focus();
}

fn confirm_delete_request(
    request_id: Uuid,
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let Some(request_name) = state.collection.borrow().as_ref().and_then(|session| {
        session
            .requests
            .iter()
            .find(|request| request.node.id == request_id)
            .map(|request| request.node.name.clone())
    }) else {
        return;
    };
    let dialog = AlertDialog::builder()
        .modal(true)
        .message(format!("Delete ‘{request_name}’?"))
        .detail("This request will be permanently deleted.")
        .buttons(["Cancel", "Delete"])
        .cancel_button(0)
        .default_button(0)
        .build();
    dialog.choose(Some(window), None::<&gio::Cancellable>, {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |response| {
            if response == Ok(1) {
                remove_request(request_id, &state, &sidebar, &editor);
                show_message(
                    &sidebar.status,
                    "Deleted the request. Saving automatically…",
                );
            }
        }
    });
}

fn run_storage_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
    complete: impl FnOnce(Result<T, String>) + 'static,
) {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(task());
    });
    let mut complete = Some(complete);
    glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The collection worker stopped unexpectedly.".to_owned())
            }
        };
        if let Some(complete) = complete.take() {
            complete(result);
        }
        glib::ControlFlow::Break
    });
}

fn pending_collection_save(state: &Rc<RequestState>) -> Option<PendingCollectionSave> {
    let session_ref = state.collection.borrow();
    let session = session_ref.as_ref()?;
    if !session.is_dirty() {
        return None;
    }
    let changed_nodes = session
        .requests
        .iter()
        .filter(|request| request.node_is_dirty())
        .map(|request| request.node.clone())
        .collect::<Vec<_>>();
    let changed_requests = session
        .requests
        .iter()
        .filter(|request| request.request_is_dirty())
        .filter_map(|request| {
            Some(CollectionRequest {
                node: request.node.clone(),
                request: request.request.clone()?,
            })
        })
        .collect::<Vec<_>>();
    Some(PendingCollectionSave {
        summary: session.summary.clone(),
        changed_nodes,
        changed_requests,
        deleted_nodes: session.deleted_nodes.clone(),
    })
}

fn autosave_current_collection(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
    window: &ApplicationWindow,
) {
    if state.collection_busy.replace(true) {
        return;
    }
    capture_active_request(state, sidebar, editor);
    let Some(pending) = pending_collection_save(state) else {
        state.collection_busy.set(false);
        return;
    };
    let saved_nodes = pending
        .changed_nodes
        .iter()
        .cloned()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    let saved_requests = pending
        .changed_requests
        .iter()
        .map(|request| (request.node.id, request.request.clone()))
        .collect::<HashMap<_, _>>();
    show_message(response_summary, "Saving changes automatically…");
    run_storage_task(
        {
            let pending = pending.clone();
            move || {
                let mut store =
                    CollectionStore::open_default().map_err(|error| error.to_string())?;
                store
                    .save_collection(
                        &pending.summary,
                        &pending.changed_nodes,
                        &pending.changed_requests,
                        &pending.deleted_nodes,
                    )
                    .map_err(|error| error.to_string())
            }
        },
        {
            let state = state.clone();
            let sidebar = sidebar.clone();
            let editor = editor.clone();
            let response_summary = response_summary.clone();
            let window = window.clone();
            move |result| {
                state.collection_busy.set(false);
                match result {
                    Ok(()) => {
                        if let Some(session) = state.collection.borrow_mut().as_mut()
                            && session.summary.id == pending.summary.id
                        {
                            session.saved_summary = Some(pending.summary.clone());
                            session
                                .deleted_nodes
                                .retain(|id| !pending.deleted_nodes.contains(id));
                            for request in &mut session.requests {
                                if let Some(saved_node) = saved_nodes.get(&request.node.id) {
                                    request.saved_node = Some(saved_node.clone());
                                }
                                if let Some(saved_request) = saved_requests.get(&request.node.id) {
                                    request.saved_request = Some(saved_request.clone());
                                }
                            }
                        }
                        capture_active_request(&state, &sidebar, &editor);
                        show_message(&response_summary, "Changes saved automatically.");

                        if state.close_after_autosave.get() {
                            if collection_is_dirty(&state) {
                                autosave_current_collection(
                                    &state,
                                    &sidebar,
                                    &editor,
                                    &response_summary,
                                    &window,
                                );
                            } else {
                                state.close_after_autosave.set(false);
                                state.allow_close.set(true);
                                window.close();
                            }
                            return;
                        }

                        let action = state.after_autosave.borrow_mut().take();
                        if let Some(action) = action {
                            if collection_is_dirty(&state) {
                                state.after_autosave.replace(Some(action));
                                autosave_current_collection(
                                    &state,
                                    &sidebar,
                                    &editor,
                                    &response_summary,
                                    &window,
                                );
                            } else {
                                action();
                            }
                        } else if collection_is_dirty(&state) {
                            autosave_current_collection(
                                &state,
                                &sidebar,
                                &editor,
                                &response_summary,
                                &window,
                            );
                        }
                    }
                    Err(error) => {
                        state.close_after_autosave.set(false);
                        state.after_autosave.replace(None);
                        sync_collection_picker(&state, &sidebar);
                        show_error(
                            &response_summary,
                            &format!("Could not autosave collection changes: {error}"),
                        );
                    }
                }
            }
        },
    );
}

fn load_collection(
    collection: CollectionSummary,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    if state.collection_busy.replace(true) {
        return;
    }
    show_message(response_summary, "Opening collection…");
    run_storage_task(
        {
            let collection = collection.clone();
            move || {
                let store = CollectionStore::open_default().map_err(|error| error.to_string())?;
                store
                    .load_tree(collection.id)
                    .map_err(|error| error.to_string())
            }
        },
        {
            let state = state.clone();
            let sidebar = sidebar.clone();
            let editor = editor.clone();
            let response_summary = response_summary.clone();
            move |result: Result<Vec<CollectionNode>, String>| {
                state.collection_busy.set(false);
                match result {
                    Ok(nodes) => {
                        let folders = nodes
                            .iter()
                            .filter(|node| node.kind == CollectionNodeKind::Folder)
                            .cloned()
                            .collect();
                        let requests = nodes
                            .into_iter()
                            .filter(|node| node.kind == CollectionNodeKind::Request)
                            .map(|node| EditorRequest {
                                saved_node: Some(node.clone()),
                                node,
                                request: None,
                                saved_request: None,
                            })
                            .collect::<Vec<_>>();
                        let first_request = requests.first().map(|request| request.node.id);
                        state.collection.replace(Some(CollectionSession {
                            summary: collection.clone(),
                            saved_summary: Some(collection.clone()),
                            _folders: folders,
                            requests,
                            deleted_nodes: Vec::new(),
                            active_request: None,
                        }));
                        sidebar.search.set_text("");
                        sidebar.applied_search.replace(String::new());
                        sync_collection_picker(&state, &sidebar);
                        render_request_buttons(&state, &sidebar, &editor);
                        if let Some(request_id) = first_request {
                            select_request(
                                request_id,
                                &state,
                                &sidebar,
                                &editor,
                                &response_summary,
                            );
                        } else {
                            clear_request_editor(&sidebar, &editor);
                            show_message(&response_summary, "Opened an empty collection.");
                        }
                    }
                    Err(error) => {
                        sync_collection_picker(&state, &sidebar);
                        show_error(&response_summary, &error);
                    }
                }
            }
        },
    );
}

fn add_collection_request(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) -> bool {
    capture_active_request(state, sidebar, editor);
    if state.collection.borrow().is_none() {
        return false;
    }
    let request_id = {
        let mut session_ref = state.collection.borrow_mut();
        let Some(session) = session_ref.as_mut() else {
            return false;
        };
        let position = session
            .requests
            .iter()
            .map(|request| request.node.position)
            .max()
            .map_or(0, |position| position.saturating_add(1));
        let request = CollectionRequest::new(
            session.summary.id,
            None,
            "HTTP Request",
            position,
            Request::default(),
        );
        let request_id = request.node.id;
        session.requests.push(EditorRequest {
            node: request.node,
            request: Some(request.request),
            saved_node: None,
            saved_request: None,
        });
        session.active_request = Some(request_id);
        request_id
    };
    apply_loaded_request(request_id, state, sidebar, editor);
    render_request_buttons(state, sidebar, editor);
    request_autosave(&sidebar.autosave);
    true
}

fn duplicate_request(
    source_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    capture_active_request(state, sidebar, editor);
    let source = state.collection.borrow().as_ref().and_then(|session| {
        session
            .requests
            .iter()
            .find(|request| request.node.id == source_id)
            .cloned()
    });
    let Some(source) = source else {
        return;
    };
    if let Some(request) = source.request.clone() {
        append_duplicate_request(source.node, request, state, sidebar, editor);
        show_message(response_summary, "Duplicated the request.");
        return;
    }
    if state.collection_busy.replace(true) {
        return;
    }
    show_message(response_summary, "Loading request…");
    run_storage_task(
        move || {
            let store = CollectionStore::open_default().map_err(|error| error.to_string())?;
            store
                .load_request(source_id)
                .map_err(|error| error.to_string())
        },
        {
            let state = state.clone();
            let sidebar = sidebar.clone();
            let editor = editor.clone();
            let response_summary = response_summary.clone();
            move |result: Result<CollectionRequest, String>| {
                state.collection_busy.set(false);
                match result {
                    Ok(source) => {
                        append_duplicate_request(
                            source.node,
                            source.request,
                            &state,
                            &sidebar,
                            &editor,
                        );
                        show_message(&response_summary, "Duplicated the request.");
                    }
                    Err(error) => show_error(&response_summary, &error),
                }
            }
        },
    );
}

fn append_duplicate_request(
    source_node: CollectionNode,
    request: Request,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    capture_active_request(state, sidebar, editor);
    let request_id = {
        let mut session_ref = state.collection.borrow_mut();
        let Some(session) = session_ref.as_mut() else {
            return;
        };
        let position = session
            .requests
            .iter()
            .map(|request| request.node.position)
            .max()
            .map_or(0, |position| position.saturating_add(1));
        let duplicate = CollectionRequest::new(
            session.summary.id,
            source_node.parent_id,
            format!("{} copy", source_node.name),
            position,
            request,
        );
        let request_id = duplicate.node.id;
        session.requests.push(EditorRequest {
            node: duplicate.node,
            request: Some(duplicate.request),
            saved_node: None,
            saved_request: None,
        });
        session.active_request = Some(request_id);
        request_id
    };
    apply_loaded_request(request_id, state, sidebar, editor);
    render_request_buttons(state, sidebar, editor);
    request_autosave(&sidebar.autosave);
}

fn copy_request_as_curl(
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
    clipboard: &gdk::Clipboard,
) {
    capture_active_request(state, sidebar, editor);
    let request = state.collection.borrow().as_ref().and_then(|session| {
        session
            .requests
            .iter()
            .find(|request| request.node.id == request_id)
            .and_then(|request| request.request.clone())
    });
    if let Some(request) = request {
        copy_request_to_clipboard(request, response_summary, clipboard);
        return;
    }
    if state.collection_busy.replace(true) {
        return;
    }
    show_message(response_summary, "Loading request…");
    run_storage_task(
        move || {
            let store = CollectionStore::open_default().map_err(|error| error.to_string())?;
            store
                .load_request(request_id)
                .map_err(|error| error.to_string())
        },
        {
            let state = state.clone();
            let response_summary = response_summary.clone();
            let clipboard = clipboard.clone();
            move |result: Result<CollectionRequest, String>| {
                state.collection_busy.set(false);
                match result {
                    Ok(request) => {
                        copy_request_to_clipboard(request.request, &response_summary, &clipboard)
                    }
                    Err(error) => show_error(&response_summary, &error),
                }
            }
        },
    );
}

fn copy_request_to_clipboard(request: Request, summary: &Label, clipboard: &gdk::Clipboard) {
    match to_command(request) {
        Ok(command) => {
            clipboard.set_text(&command);
            show_message(summary, "Copied the request as cURL.");
        }
        Err(error) => show_error(summary, &error.to_string()),
    }
}

fn remove_request(
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    capture_active_request(state, sidebar, editor);
    let (removed_active, next_request) = {
        let mut session_ref = state.collection.borrow_mut();
        let Some(session) = session_ref.as_mut() else {
            return;
        };
        let removed_active = session.active_request == Some(request_id);
        if let Some(index) = session
            .requests
            .iter()
            .position(|request| request.node.id == request_id)
        {
            let removed = session.requests.remove(index);
            if removed.saved_node.is_some() {
                session.deleted_nodes.push(removed.node.id);
            }
        }
        let next_request = if removed_active {
            let next = session.requests.first().map(|request| request.node.id);
            session.active_request = None;
            next
        } else {
            session.active_request
        };
        (removed_active, next_request)
    };
    render_request_buttons(state, sidebar, editor);
    if removed_active {
        clear_request_editor(sidebar, editor);
        if let Some(request_id) = next_request {
            select_request(request_id, state, sidebar, editor, &sidebar.status);
        }
    }
    request_autosave(&sidebar.autosave);
}

fn select_request(
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    if state.collection_busy.get() {
        return;
    }
    capture_active_request(state, sidebar, editor);
    let loaded = {
        let mut session_ref = state.collection.borrow_mut();
        let Some(session) = session_ref.as_mut() else {
            return;
        };
        let Some(request) = session
            .requests
            .iter()
            .find(|request| request.node.id == request_id)
        else {
            return;
        };
        session.active_request = Some(request_id);
        request.request.is_some()
    };
    render_request_buttons(state, sidebar, editor);
    if loaded {
        apply_loaded_request(request_id, state, sidebar, editor);
        return;
    }

    state.collection_busy.set(true);
    show_message(response_summary, "Loading request…");
    run_storage_task(
        move || {
            let store = CollectionStore::open_default().map_err(|error| error.to_string())?;
            store
                .load_request(request_id)
                .map_err(|error| error.to_string())
        },
        {
            let state = state.clone();
            let sidebar = sidebar.clone();
            let editor = editor.clone();
            let response_summary = response_summary.clone();
            move |result: Result<CollectionRequest, String>| {
                state.collection_busy.set(false);
                match result {
                    Ok(saved_request) => {
                        if let Some(session) = state.collection.borrow_mut().as_mut()
                            && session.active_request == Some(request_id)
                            && let Some(request) = session
                                .requests
                                .iter_mut()
                                .find(|request| request.node.id == request_id)
                        {
                            request.node = saved_request.node.clone();
                            request.saved_node = Some(saved_request.node);
                            request.request = Some(saved_request.request.clone());
                            request.saved_request = Some(saved_request.request);
                        }
                        apply_loaded_request(request_id, &state, &sidebar, &editor);
                        render_request_buttons(&state, &sidebar, &editor);
                        show_message(&response_summary, "Loaded the saved request.");
                    }
                    Err(error) => {
                        if let Some(session) = state.collection.borrow_mut().as_mut()
                            && session.active_request == Some(request_id)
                        {
                            session.active_request = None;
                        }
                        clear_request_editor(&sidebar, &editor);
                        render_request_buttons(&state, &sidebar, &editor);
                        show_error(&response_summary, &error);
                    }
                }
            }
        },
    );
}

fn capture_active_request(
    state: &Rc<RequestState>,
    _sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let Ok(editor_request) = collect_request(
        &editor.method,
        &editor.url,
        &editor.header_rows,
        &editor.body,
    ) else {
        return;
    };
    let mut session_ref = state.collection.borrow_mut();
    let Some(session) = session_ref.as_mut() else {
        return;
    };
    let Some(active_id) = session.active_request else {
        return;
    };
    if let Some(request) = session
        .requests
        .iter_mut()
        .find(|request| request.node.id == active_id)
    {
        request.request = Some(editor_request);
    }
}

fn apply_loaded_request(
    request_id: Uuid,
    state: &Rc<RequestState>,
    _sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let request = state.collection.borrow().as_ref().and_then(|session| {
        session
            .requests
            .iter()
            .find(|request| request.node.id == request_id)
            .and_then(|request| request.request.clone())
    });
    if let Some(request) = request {
        state.applying_editor.set(true);
        apply_request(
            request,
            &editor.method,
            &editor.url,
            &editor.headers_box,
            &editor.header_rows,
            &editor.body,
        );
        state.applying_editor.set(false);
    }
}

fn render_request_buttons(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    while let Some(child) = sidebar.requests.first_child() {
        sidebar.requests.remove(&child);
    }
    let Some((mut requests, active_request)) = state.collection.borrow().as_ref().map(|session| {
        (
            session
                .requests
                .iter()
                .map(|request| {
                    (
                        request.node.id,
                        request.node.name.clone(),
                        request.node.position,
                    )
                })
                .collect::<Vec<_>>(),
            session.active_request,
        )
    }) else {
        sidebar
            .requests
            .append(&Label::new(Some("Select a collection to view requests.")));
        return;
    };
    let search = sidebar.applied_search.borrow().clone();
    if !search.is_empty() {
        requests.retain(|(_, name, _)| name.to_lowercase().contains(&search));
    }
    requests.sort_by_key(|(_, _, position)| *position);
    if requests.is_empty() {
        let message = if search.is_empty() {
            "Right-click here to create an HTTP request."
        } else {
            "No requests match this search."
        };
        sidebar.requests.append(&Label::new(Some(message)));
        return;
    }
    for (request_id, name, _) in requests {
        let button = Button::with_label(&name);
        button.set_halign(Align::Fill);
        button.add_css_class("flat");
        if active_request == Some(request_id) {
            button.add_css_class("suggested-action");
        }
        button.connect_clicked({
            let state = state.clone();
            let sidebar = sidebar.clone();
            let editor = editor.clone();
            move |_| {
                select_request(request_id, &state, &sidebar, &editor, &sidebar.status);
            }
        });
        attach_request_context_menu(&button, request_id, state, sidebar, editor);
        sidebar.requests.append(&button);
    }
}

fn apply_request_search(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    sidebar
        .applied_search
        .replace(sidebar.search.text().trim().to_lowercase());
    render_request_buttons(state, sidebar, editor);
}

fn clear_request_editor(_sidebar: &SidebarWidgets, editor: &EditorWidgets) {
    apply_request(
        Request::default(),
        &editor.method,
        &editor.url,
        &editor.headers_box,
        &editor.header_rows,
        &editor.body,
    );
}

fn collection_is_dirty(state: &Rc<RequestState>) -> bool {
    state
        .collection
        .borrow()
        .as_ref()
        .is_some_and(CollectionSession::is_dirty)
}

fn request_autosave(autosave: &AutosaveTrigger) {
    let trigger = autosave.borrow().clone();
    if let Some(trigger) = trigger {
        trigger();
    }
}

fn autosave_on_blur<W: IsA<gtk::Widget>>(widget: &W, autosave: &AutosaveTrigger) {
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

fn build_headers_page(
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

fn add_header_row(
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
        let container = container.clone();
        let rows = rows.clone();
        let autosave = autosave.clone();
        move |_| {
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

fn build_body_page(
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
    let editor = TextView::builder()
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    let editor_scroll = scrolled(&editor);
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
        let window = window.clone();
        let autosave = autosave.clone();
        move |_| {
            add_multipart_row(&multipart_box, &multipart_rows, &window, &autosave);
            request_autosave(&autosave);
        }
    });
    multipart_panel.append(&multipart_box);
    multipart_panel.append(&add);
    let multipart_scroll = scrolled(&multipart_panel);
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
            multipart_box,
            multipart_rows,
            autosave: autosave.clone(),
        },
    )
}

fn add_multipart_row(
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
        let window = window.clone();
        let autosave = autosave.clone();
        move |_| {
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
        let container = container.clone();
        let rows = rows.clone();
        let autosave = autosave.clone();
        move |_| {
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

fn collect_request(
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

fn apply_request(
    request: Request,
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
        for header in request.headers {
            add_header_row(headers_box, header_rows, &body_widgets.autosave);
            if let Some(widgets) = header_rows.borrow().last() {
                widgets.enabled.set_active(header.enabled);
                widgets.name.set_text(&header.name);
                widgets.value.set_text(&header.value);
            }
        }
    }

    match request.body {
        RequestBody::None => {
            body_widgets.mode.set_selected(0);
            body_widgets.json_editor.buffer().set_text("");
            reset_multipart_rows(body_widgets, &[]);
        }
        RequestBody::Json(body) => {
            body_widgets.mode.set_selected(1);
            body_widgets.json_editor.buffer().set_text(&body);
            reset_multipart_rows(body_widgets, &[]);
        }
        RequestBody::Multipart(fields) => {
            body_widgets.mode.set_selected(2);
            body_widgets.json_editor.buffer().set_text("");
            reset_multipart_rows(body_widgets, &fields);
        }
    }
}

fn reset_multipart_rows(body: &BodyWidgets, fields: &[MultipartField]) {
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

fn set_request_running(send_group: &GtkBox, cancel: &Button, running: bool) {
    send_group.set_visible(!running);
    cancel.set_visible(running);
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
