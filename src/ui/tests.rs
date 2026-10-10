//! GTK integration and widget-lifetime tests.
//!
//! Pure UI planning is tested in its owning modules. Tests here cover behavior
//! that requires real GTK objects, including theme selection and release of
//! transient widget graphs. Display-dependent coverage stays ignored in the
//! default headless suite and documents the command needed to run it manually.

use super::*;
use editor::{
    EditorWidgetHandles, ResponseWidgets, add_form_urlencoded_row, add_header_row,
    add_multipart_row, apply_request, build_source_viewer, json_style_scheme_id,
    set_source_content_type,
};
use pakpos::{app::CollectionSession, models::Request};
use sidebar::{build_request_context_menu, render_request_buttons};
use sourceview5::prelude::*;
use std::sync::Arc;

fn assert_has_accessible_label(widget: &impl IsA<gtk::Widget>) {
    assert!(
        gtk::test_accessible_has_property(widget.as_ref(), gtk::AccessibleProperty::Label,),
        "{} has no explicit accessible label",
        widget.as_ref().type_().name(),
    );
}

#[test]
fn json_style_scheme_follows_the_gtk_theme() {
    assert_eq!(json_style_scheme_id(false, Some("Adwaita")), "Adwaita");
    assert_eq!(
        json_style_scheme_id(false, Some("Adwaita-dark")),
        "Adwaita-dark"
    );
    assert_eq!(
        json_style_scheme_id(false, Some("Breeze_Darker")),
        "Adwaita-dark"
    );
    assert_eq!(json_style_scheme_id(true, None), "Adwaita-dark");
}

/// Exercises the production ownership graph without loading or writing storage.
/// Run separately from other GTK tests because GTK belongs to one test thread.
#[test]
#[ignore = "requires a GTK display"]
fn gobject_ownership() {
    gtk::init().expect("GTK display required");
    sourceview5::init();
    toast::assert_lifetimes();
    let application = Application::builder()
        .application_id("io.github.pakpos.OwnershipTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.register(None::<&gio::Cancellable>).unwrap();
    let context = glib::MainContext::default();
    for _ in 0..10 {
        let (window, resources) = build_window(&application);
        window.present();
        while context.pending() {
            context.iteration(false);
        }
        let weak_resources = Rc::downgrade(&resources);
        let weak_state = Rc::downgrade(&resources.state);
        let weak_sidebar = Rc::downgrade(&resources.sidebar);
        let weak_editor = Rc::downgrade(&resources.editor);
        let weak_autosave = Rc::downgrade(&resources.sidebar.autosave);
        let weak_styles = resources._styles.provider.downgrade();
        let weak_request_view = resources.editor.body.json_editor.downgrade();
        let weak_request_buffer = resources.editor.body.json_editor.buffer().downgrade();
        let weak_response_view = resources.editor.response.body.view.downgrade();
        let weak_response_buffer = resources.editor.response.body.view.buffer().downgrade();
        let weak_search = resources.editor.response.body.search_context.downgrade();
        let weak_toast = glib::clone::Downgrade::downgrade(&resources._toast);
        let poll = context
            .find_source_by_id(resources.autosave_poll.borrow().as_ref().unwrap())
            .unwrap();
        let copy_action = window.lookup_action("copy-curl").unwrap();
        let paste_action = window.lookup_action("paste-curl").unwrap();
        let send_action = window.lookup_action("send").unwrap();
        let format_button = resources.editor.body.json_format.clone();

        // Weak callbacks must still work while the window's owner is alive.
        resources.editor.url.set_text("https://example.com");
        resources.editor.body.mode.set_selected(1);
        resources
            .editor
            .body
            .json_editor
            .buffer()
            .set_text("{\"a\":1}");
        // Prevent edits from starting storage effects in this lifetime test.
        resources.state.collection_busy.set(true);
        format_button.emit_clicked();
        assert!(editor::buffer_text(&resources.editor.body.json_editor).contains('\n'));
        resources
            .editor
            .response
            .body
            .view
            .buffer()
            .set_text("alpha beta alpha");
        // Exercise the search context without selecting text: GTK 4.24.1's
        // selection provider independently retains its buffer (see the audit).
        let search = &resources.editor.response.body.search_context;
        search.settings().set_search_text(Some("alpha"));
        assert!(
            search
                .forward(&resources.editor.response.body.view.buffer().start_iter())
                .is_some()
        );
        copy_action.activate(None);

        // Closing an unaccepted dialog must release it and its children. Keep
        // the entry to detect callbacks that retain the sibling default button.
        dialogs::show_create_collection_dialog(
            &window,
            &resources.state,
            &resources.sidebar,
            &resources.editor,
            &resources.sidebar.status,
        );
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
            .find(|candidate| candidate.title().as_deref() == Some("Create collection"))
            .unwrap();
        let content = dialog.child().unwrap();
        let entry = content
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<Entry>()
            .unwrap();
        let default = dialog.default_widget().unwrap().downgrade();
        let weak_dialog = dialog.downgrade();
        entry.set_text("New collection");
        assert!(dialog.default_widget().unwrap().is_sensitive());
        dialog.close();
        drop(content);
        drop(dialog);
        while context.pending() {
            context.iteration(false);
        }
        assert!(weak_dialog.upgrade().is_none());
        assert!(default.upgrade().is_none());
        entry.set_text("Detached entry");
        drop(entry);

        let collection = CollectionSummary::new("Rename lifetime test");
        let node = pakpos::collections::CollectionNode::request(collection.id, "Original", 0);
        let request_id = node.id;
        resources
            .state
            .update(Action::SetCollection(CollectionSession::from_requests(
                collection,
                vec![node],
            )));
        dialogs::show_rename_request_dialog(
            request_id,
            &window,
            &resources.state,
            &resources.sidebar,
            &resources.editor,
        );
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
            .find(|candidate| candidate.title().as_deref() == Some("Rename request"))
            .unwrap();
        let entry = dialog
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<Entry>()
            .unwrap();
        let done = dialog
            .default_widget()
            .unwrap()
            .downcast::<Button>()
            .unwrap();
        let weak_dialog = dialog.downgrade();
        let weak_done = done.downgrade();
        entry.set_text("Renamed");
        done.emit_clicked();
        assert_eq!(
            resources
                .state
                .collection
                .borrow()
                .as_ref()
                .unwrap()
                .request_name(request_id),
            Some("Renamed")
        );
        drop(done);
        drop(dialog);
        while context.pending() {
            context.iteration(false);
        }
        assert!(weak_dialog.upgrade().is_none());
        assert!(weak_done.upgrade().is_none());
        entry.set_text("Detached rename entry");
        drop(entry);

        // Leave a modal dialog open: destroying its parent must destroy it too.
        dialogs::show_create_collection_dialog(
            &window,
            &resources.state,
            &resources.sidebar,
            &resources.editor,
            &resources.sidebar.status,
        );
        let open_dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
            .find(|candidate| candidate.title().as_deref() == Some("Create collection"))
            .unwrap()
            .downgrade();

        drop(resources);
        assert!(
            weak_resources.upgrade().is_some(),
            "window must own its live state"
        );
        window.destroy();
        drop(window);
        while context.pending() {
            context.iteration(false);
        }
        // GtkSourceView finishes asynchronous search/highlight work on idle.
        let main_loop = glib::MainLoop::new(None, false);
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), {
            let main_loop = main_loop.clone();
            move || main_loop.quit()
        });
        main_loop.run();
        assert!(weak_resources.upgrade().is_none());
        assert!(weak_state.upgrade().is_none());
        assert!(weak_sidebar.upgrade().is_none());
        assert!(weak_editor.upgrade().is_none());
        assert!(weak_autosave.upgrade().is_none());
        assert!(glib::clone::Upgrade::upgrade(&weak_toast).is_none());
        assert!(
            poll.is_destroyed(),
            "window disposal must remove the autosave source"
        );
        assert!(
            weak_styles.upgrade().is_none(),
            "display must release the CSS provider"
        );
        assert!(open_dialog.upgrade().is_none());
        assert!(weak_search.upgrade().is_none());
        assert!(weak_request_view.upgrade().is_none());
        assert!(weak_request_buffer.upgrade().is_none());
        assert!(weak_response_view.upgrade().is_none());
        assert!(weak_response_buffer.upgrade().is_none());
        // External handles to old actions/buttons cannot keep the UI alive;
        // invoking them after teardown safely does nothing.
        copy_action.activate(None);
        paste_action.activate(None);
        send_action.activate(None);
        format_button.emit_clicked();
    }
}

/// Run separately from other GTK tests because GTK belongs to one test thread.
#[test]
#[ignore = "requires a GTK display"]
fn collection_switch_lifetimes() {
    gtk::init().expect("GTK display required");
    sourceview5::init();
    let window = ApplicationWindow::builder()
        .title("Pakpos collection memory verification")
        .default_width(900)
        .default_height(600)
        .build();
    let autosave = AutosaveTrigger::default();
    let sidebar = build_sidebar(autosave.clone());
    let (_, headers_box, header_rows) = build_headers_page(&autosave);
    let (body_page, body) = build_body_page(&window, &autosave);
    let editor = Rc::new(EditorWidgetHandles {
        method: DropDown::from_strings(&["GET", "POST"]),
        url: Entry::new(),
        headers_box,
        header_rows,
        body,
        response: ResponseWidgets {
            body: build_source_viewer(),
            metadata: Label::new(None),
            headers: gtk::TextView::new(),
        },
    });
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&sidebar.root);
    root.append(&editor.headers_box);
    root.append(&body_page);
    root.append(&editor.response.body.root);
    window.set_child(Some(&root));
    window.present();
    let state = Rc::new(RequestState::default());
    let context = glib::MainContext::default();
    for iteration in 0..100 {
        let previous_rows: Vec<_> = sidebar
            .request_rows
            .borrow()
            .values()
            .map(|row| row.downgrade())
            .collect();
        let previous_header = editor.header_rows.borrow()[0].row.downgrade();
        let previous_form = editor.body.form_rows.borrow()[0].row.downgrade();
        let previous_multipart = editor.body.multipart_rows.borrow()[0].row.downgrade();
        let summary = CollectionSummary::new(format!("Collection {iteration}"));
        let nodes: Vec<_> = (0..1000)
            .map(|position| {
                pakpos::collections::CollectionNode::request(
                    summary.id,
                    format!("Request {position}"),
                    position,
                )
            })
            .collect();
        let active_node = nodes[0].clone();
        let id = active_node.id;
        let mut session = CollectionSession::from_requests(summary, nodes);
        let request = Request {
            url: format!("https://example.com/{iteration}"),
            body: pakpos::models::RequestBody::Json(format!(
                "{{\"data\":\"{}\"}}",
                "x".repeat(64 * 1024)
            )),
            ..Request::default()
        };
        session.apply_loaded_request(pakpos::collections::CollectionRequest {
            node: active_node,
            request: Arc::new(request),
        });
        let payload = session.shared_request(id).unwrap();
        let previous_payload = Arc::downgrade(&payload);
        state.update(Action::SetCollection(session));
        render_request_buttons(&state, &sidebar, &editor);
        apply_loaded_request(id, &state, &sidebar, &editor);
        editor
            .response
            .body
            .view
            .buffer()
            .set_text(&"response\n".repeat(8192));
        editor.response.clear();
        assert!(
            !editor.body.json_editor.buffer().can_undo(),
            "navigation retained request undo history"
        );
        assert!(
            !editor.response.body.view.buffer().can_undo(),
            "navigation retained response undo history"
        );
        drop(payload);
        assert!(
            previous_rows.iter().all(|row| row.upgrade().is_none()),
            "old collection rows leaked"
        );
        assert!(previous_header.upgrade().is_none(), "old header leaked");
        assert!(previous_form.upgrade().is_none(), "old form row leaked");
        assert!(
            previous_multipart.upgrade().is_none(),
            "old multipart row leaked"
        );
        state.update(Action::SetCollection(CollectionSession::empty(
            CollectionSummary::new("Empty"),
        )));
        assert!(
            previous_payload.upgrade().is_none(),
            "old request payload leaked"
        );
        // Let GTK allocate and render the new rows before measuring retention.
        let main_loop = glib::MainLoop::new(None, false);
        glib::timeout_add_local_once(std::time::Duration::from_millis(30), {
            let main_loop = main_loop.clone();
            move || main_loop.quit()
        });
        main_loop.run();
        if iteration % 25 == 0 || iteration == 99 {
            eprintln!(
                "collection switch {iteration}: {}",
                std::fs::read_to_string("/proc/self/status")
                    .unwrap()
                    .lines()
                    .find(|line| line.starts_with("VmRSS:"))
                    .unwrap()
            );
        }
        while context.pending() {
            context.iteration(false);
        }
    }
    let response_view = editor.response.body.view.downgrade();
    let response_search = editor.response.body.search_entry.downgrade();
    let response_buffer = editor.response.body.view.buffer().downgrade();
    let request_views: Vec<_> = [
        &editor.body.json_editor,
        &editor.body.xml_editor,
        &editor.body.text_editor,
    ]
    .into_iter()
    .map(|view| (view.downgrade(), view.buffer().downgrade()))
    .collect();
    window.set_child(None::<&gtk::Widget>);
    window.destroy();
    drop(window);
    drop(root);
    drop(body_page);
    drop(editor);
    while context.pending() {
        context.iteration(false);
    }
    assert!(
        response_search.upgrade().is_none(),
        "response search leaked"
    );
    assert!(response_view.upgrade().is_none(), "response view leaked");
    assert!(
        response_buffer.upgrade().is_none(),
        "response buffer leaked"
    );
    assert!(
        request_views
            .iter()
            .all(|(view, buffer)| view.upgrade().is_none() && buffer.upgrade().is_none()),
        "request editor or buffer leaked"
    );
}

fn assert_source_folding(view: &sourceview5::View, text: &str) {
    let buffer = view.buffer();
    buffer.set_text(text);
    buffer.set_modified(false);
    let header = buffer.iter_at_line(0).unwrap();
    buffer.place_cursor(&header);
    view.activate_action("fold.collapse", None).unwrap();
    let tag = buffer.tag_table().lookup("pakpos-folded-lines").unwrap();
    assert!(buffer.iter_at_line(1).unwrap().has_tag(&tag));
    assert!(!buffer.start_iter().has_tag(&tag));
    assert_eq!(editor::buffer_text(view), text);
    assert!(!buffer.is_modified(), "folding modified the request");
    view.activate_action("fold.expand", None).unwrap();
    assert!(!buffer.iter_at_line(1).unwrap().has_tag(&tag));
    view.activate_action("fold.collapse-all", None).unwrap();
    assert!(buffer.iter_at_line(1).unwrap().has_tag(&tag));
    view.activate_action("fold.expand-all", None).unwrap();
    assert!(!buffer.iter_at_line(1).unwrap().has_tag(&tag));

    // Moving into folded content reveals it for keyboard navigation/search.
    view.activate_action("fold.collapse-all", None).unwrap();
    buffer.place_cursor(&buffer.iter_at_line(1).unwrap());
    assert!(!buffer.iter_at_line(1).unwrap().has_tag(&tag));

    // Content replacement clears folds, including when switching requests.
    buffer.place_cursor(&header);
    view.activate_action("fold.collapse-all", None).unwrap();
    buffer.set_text("replacement\nsource");
    assert!(!buffer.iter_at_line(1).unwrap().has_tag(&tag));
}

fn assert_nested_folding_and_edits(view: &sourceview5::View) {
    let buffer = view.buffer();
    let text = "{\n  \"data\": [\n    {\n      \"value\": 1\n    }\n  ],\n  \"other\": true\n}";
    buffer.set_text(text);
    let tag = buffer.tag_table().lookup("pakpos-folded-lines").unwrap();
    buffer.place_cursor(&buffer.iter_at_line(1).unwrap());
    view.activate_action("fold.collapse", None).unwrap();
    assert!(buffer.iter_at_line(2).unwrap().has_tag(&tag));
    assert!(!buffer.iter_at_line(6).unwrap().has_tag(&tag));
    buffer.place_cursor(&buffer.start_iter());
    view.activate_action("fold.collapse", None).unwrap();
    view.activate_action("fold.expand", None).unwrap();
    assert!(
        buffer.iter_at_line(2).unwrap().has_tag(&tag),
        "nested fold was lost"
    );
    assert!(!buffer.iter_at_line(6).unwrap().has_tag(&tag));

    buffer.set_modified(false);
    buffer.insert(&mut buffer.start_iter(), " ");
    assert!(!buffer.iter_at_line(2).unwrap().has_tag(&tag));
    assert_eq!(editor::buffer_text(view), format!(" {text}"));
    assert!(buffer.is_modified());
    buffer.undo();
    assert_eq!(
        editor::buffer_text(view),
        text,
        "folding changed undo history"
    );
    buffer.place_cursor(&buffer.start_iter());
    view.activate_action("fold.collapse-all", None).unwrap();
    buffer.delete(&mut buffer.start_iter(), &mut buffer.iter_at_offset(1));
    assert!(!buffer.iter_at_line(1).unwrap().has_tag(&tag));
    assert_eq!(editor::buffer_text(view), &text[1..]);

    // Backspace after a collapsed section removes only its final newline.
    buffer.set_text(text);
    buffer.place_cursor(&buffer.iter_at_line(1).unwrap());
    view.activate_action("fold.collapse", None).unwrap();
    buffer.place_cursor(&buffer.iter_at_line(6).unwrap());
    view.emit_by_name::<()>("backspace", &[]);
    assert_eq!(editor::buffer_text(view), text.replace("],\n", "],"));

    // Delete at a folded header removes only the newline after that header.
    buffer.set_text(text);
    let mut header_end = buffer.iter_at_line(1).unwrap();
    header_end.forward_to_line_end();
    buffer.place_cursor(&header_end);
    view.activate_action("fold.collapse", None).unwrap();
    view.emit_by_name::<()>("delete-from-cursor", &[&gtk::DeleteType::Chars, &1i32]);
    assert_eq!(editor::buffer_text(view), text.replace("[\n", "["));
}

fn assert_request_body_formatting(body: &editor::BodyWidgets) {
    let (notifications, toast) = Toast::overlay(&GtkBox::new(Orientation::Vertical, 0));
    body.connect_format_buttons(&toast);
    let notification = notifications
        .last_child()
        .unwrap()
        .downcast::<gtk::Revealer>()
        .unwrap();
    assert_has_accessible_label(&body.json_format);
    assert_eq!(body.json_format.halign(), gtk::Align::End);
    assert_eq!(body.json_format.valign(), gtk::Align::End);
    let panel = body.json_format.parent().unwrap();
    body.mode.set_selected(1);
    assert!(panel.is_visible());
    for mode in [0, 2, 3, 4, 5] {
        body.mode.set_selected(mode);
        assert!(!panel.is_visible());
    }
    body.mode.set_selected(1);

    let saves = Rc::new(std::cell::Cell::new(0));
    *body.autosave.borrow_mut() = Some(Rc::new({
        let saves = saves.clone();
        move || saves.set(saves.get() + 1)
    }));
    let buffer = body.json_editor.buffer();
    buffer.set_text("");
    assert!(!body.json_format.is_sensitive());
    let pasted = r#"{"z":[1,2],"a":true}"#;
    buffer.insert(&mut buffer.start_iter(), pasted);
    assert!(body.json_format.is_sensitive());
    let sensitivity_changes = Rc::new(std::cell::Cell::new(0));
    let sensitivity_handler = body.json_format.connect_sensitive_notify({
        let sensitivity_changes = sensitivity_changes.clone();
        move |_| sensitivity_changes.set(sensitivity_changes.get() + 1)
    });
    body.json_format.emit_clicked();
    body.json_format.disconnect(sensitivity_handler);
    assert_eq!(
        sensitivity_changes.get(),
        0,
        "formatting briefly disabled its button"
    );
    let expected = "{\n  \"z\": [\n    1,\n    2\n  ],\n  \"a\": true\n}";
    assert_eq!(editor::buffer_text(&body.json_editor), expected);
    assert_eq!(saves.get(), 1);
    assert!(buffer.is_modified());
    body.json_format.emit_clicked();
    assert_eq!(saves.get(), 1, "unchanged formatting requested a save");
    buffer.undo();
    assert_eq!(editor::buffer_text(&body.json_editor), pasted);
    buffer.undo();
    assert_eq!(editor::buffer_text(&body.json_editor), "");
    buffer.redo();
    buffer.redo();
    assert_eq!(editor::buffer_text(&body.json_editor), expected);

    // Formatting reads the entire document, including collapsed sections.
    let folded = "{\n \"hidden\" : [ 1 , 2 ]\n}";
    buffer.set_text(folded);
    buffer.place_cursor(&buffer.start_iter());
    body.json_editor
        .activate_action("fold.collapse-all", None)
        .unwrap();
    body.json_format.emit_clicked();
    assert_eq!(
        editor::buffer_text(&body.json_editor),
        "{\n  \"hidden\": [\n    1,\n    2\n  ]\n}"
    );
    let tag = buffer.tag_table().lookup("pakpos-folded-lines").unwrap();
    assert!(!buffer.iter_at_line(1).unwrap().has_tag(&tag));

    let invalid = "{\n  nope\n}";
    buffer.set_text(invalid);
    buffer.set_modified(false);
    let saved_before_error = saves.get();
    body.json_format.emit_clicked();
    assert_eq!(editor::buffer_text(&body.json_editor), invalid);
    assert!(!buffer.is_modified());
    assert_eq!(saves.get(), saved_before_error);
    assert!(notification.reveals_child(), "missing formatting error");

    for (mode, view, button, input, expected) in [
        (
            2,
            &body.xml_editor,
            &body.xml_format,
            "<root><child>value</child><empty/></root>",
            "<root>\n  <child>value</child>\n  <empty/>\n</root>",
        ),
        (
            5,
            &body.text_editor,
            &body.text_format,
            "{\"a\":1}",
            "{\n  \"a\": 1\n}",
        ),
        (
            5,
            &body.text_editor,
            &body.text_format,
            "<root><child/></root>",
            "<root>\n  <child/>\n</root>",
        ),
    ] {
        body.mode.set_selected(mode);
        assert_has_accessible_label(button);
        assert!(button.parent().unwrap().is_visible());
        let buffer = view.buffer();
        buffer.set_text("");
        assert!(!button.is_sensitive());
        buffer.insert(&mut buffer.start_iter(), input);
        assert!(button.is_sensitive());
        let saved_before_format = saves.get();
        button.emit_clicked();
        assert_eq!(editor::buffer_text(view), expected);
        assert_eq!(saves.get(), saved_before_format + 1);
        buffer.undo();
        assert_eq!(editor::buffer_text(view), input);
        buffer.redo();
        assert_eq!(editor::buffer_text(view), expected);
    }
    for (mode, view, button, input) in [
        (
            2,
            &body.xml_editor,
            &body.xml_format,
            "<root><child></root>",
        ),
        (
            5,
            &body.text_editor,
            &body.text_format,
            "plain words\n  preserve these spaces",
        ),
    ] {
        body.mode.set_selected(mode);
        let buffer = view.buffer();
        buffer.set_text(input);
        buffer.set_modified(false);
        let saved_before_error = saves.get();
        button.emit_clicked();
        assert_eq!(editor::buffer_text(view), input);
        assert!(!buffer.is_modified());
        assert_eq!(saves.get(), saved_before_error);
    }
    *body.autosave.borrow_mut() = None;
}

fn assert_rendered_folding(
    body_page: &GtkBox,
    body: &editor::BodyWidgets,
    response: &editor::SourceViewerWidgets,
) {
    let text = "{\n  \"user\": {\n    \"name\": \"Pakpos\",\n    \"roles\": [\n      \"reader\",\n      \"editor\"\n    ]\n  },\n  \"active\": true\n}";
    body.mode.set_selected(1);
    body.json_editor.buffer().set_text(text);
    response.view.buffer().set_text(text);
    response
        .view
        .buffer()
        .place_cursor(&response.view.buffer().iter_at_line(1).unwrap());
    response
        .view
        .activate_action("fold.collapse", None)
        .unwrap();
    let request_panel = GtkBox::new(Orientation::Vertical, 8);
    request_panel.append(&Label::new(Some("Request editor — expanded")));
    request_panel.append(body_page);
    let response_panel = GtkBox::new(Orientation::Vertical, 8);
    response_panel.append(&Label::new(Some("Response viewer — collapsed")));
    response_panel.append(&response.root);
    let panels = GtkBox::new(Orientation::Horizontal, 16);
    panels.set_homogeneous(true);
    panels.set_margin_top(16);
    panels.set_margin_bottom(16);
    panels.set_margin_start(16);
    panels.set_margin_end(16);
    panels.append(&request_panel);
    panels.append(&response_panel);
    let window = gtk::Window::builder()
        .title("Pakpos source folding verification")
        .default_width(900)
        .default_height(360)
        .child(&panels)
        .build();
    let paintable = gtk::WidgetPaintable::new(Some(&panels));
    window.present();
    let main_loop = glib::MainLoop::new(None, false);
    glib::timeout_add_local_once(std::time::Duration::from_millis(250), {
        let main_loop = main_loop.clone();
        move || main_loop.quit()
    });
    main_loop.run();

    // Capture only the verification widgets, without desktop or request data.
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, panels.width() as f64, panels.height() as f64);
    let node = snapshot.to_node().expect("folding window should render");
    window
        .renderer()
        .unwrap()
        .render_texture(&node, None)
        .save_to_png(std::env::temp_dir().join("pakpos-source-folding.png"))
        .unwrap();

    let gutter = sourceview5::prelude::ViewExt::gutter(&response.view, gtk::TextWindowType::Left);
    let mut child = gutter.first_child();
    let renderer = loop {
        let widget = child.expect("folding gutter should be installed");
        if widget.widget_name() == "pakpos-folding-gutter" {
            break widget
                .downcast::<sourceview5::GutterRendererText>()
                .unwrap();
        }
        child = widget.next_sibling();
    };
    let buffer = response.view.buffer();
    let header = buffer.iter_at_line(1).unwrap();
    let area = gtk::gdk::Rectangle::new(0, 0, 20, 20);
    assert!(renderer.query_activatable(&header, &area));
    assert!(!renderer.query_activatable(&buffer.iter_at_line(8).unwrap(), &area));
    let tag = buffer.tag_table().lookup("pakpos-folded-lines").unwrap();
    renderer.emit_by_name::<()>(
        "activate",
        &[
            &header,
            &area,
            &1u32,
            &gtk::gdk::ModifierType::empty(),
            &1i32,
        ],
    );
    assert!(!buffer.iter_at_line(2).unwrap().has_tag(&tag));
    renderer.emit_by_name::<()>(
        "activate",
        &[
            &header,
            &area,
            &1u32,
            &gtk::gdk::ModifierType::empty(),
            &1i32,
        ],
    );
    assert!(buffer.iter_at_line(2).unwrap().has_tag(&tag));

    // Folding large content changes scrollbar ranges while the view is mapped.
    // Exercise those transitions across frames, with both scrollbars needed.
    let item = format!("    {{\"value\": \"{}\"}}", "x".repeat(300));
    let large = format!(
        "{{\n  \"items\": [\n{}\n  ]\n}}",
        vec![item; 300].join(",\n")
    );
    body.json_editor.buffer().set_text(&large);
    buffer.set_text(&large);
    for view in [&body.json_editor, &response.view] {
        view.buffer().place_cursor(&view.buffer().start_iter());
    }
    let mut gutter_widths = None;
    for action in [
        "fold.expand-all",
        "fold.collapse-all",
        "fold.expand-all",
        "fold.collapse-all",
        "fold.expand-all",
    ] {
        for view in [&body.json_editor, &response.view] {
            if action == "fold.collapse-all" {
                let mut end = view.buffer().end_iter();
                view.scroll_to_iter(&mut end, 0.0, true, 1.0, 1.0);
                view.buffer().place_cursor(&end);
            }
            view.activate_action(action, None).unwrap();
        }
        let main_loop = glib::MainLoop::new(None, false);
        glib::timeout_add_local_once(std::time::Duration::from_millis(200), {
            let main_loop = main_loop.clone();
            move || main_loop.quit()
        });
        main_loop.run();
        let widths: Vec<_> = [&body.json_editor, &response.view]
            .into_iter()
            .map(|view| {
                sourceview5::prelude::ViewExt::gutter(view, gtk::TextWindowType::Left).width()
            })
            .collect();
        if let Some(previous) = &gutter_widths {
            assert_eq!(&widths, previous, "folding changed the gutter allocation");
        } else {
            gutter_widths = Some(widths);
        }
        for view in [&body.json_editor, &response.view] {
            let scroll = view
                .ancestor(gtk::ScrolledWindow::static_type())
                .unwrap()
                .downcast::<gtk::ScrolledWindow>()
                .unwrap();
            let horizontal = scroll.hadjustment();
            let vertical = scroll.vadjustment();
            horizontal.set_value(horizontal.upper() - horizontal.page_size());
            vertical.set_value(vertical.upper() - vertical.page_size());
        }
    }

    // Formatting changes both scroll ranges, gutter width, and focus while the
    // button and editor are mapped. Exercise real frame allocation around it.
    let compact_json = format!("{{\"items\":[{}]}}", vec!["{\"value\":1}"; 1000].join(","));
    let compact_xml = format!("<items>{}</items>", "<item>value</item>".repeat(1000));
    for (mode, view, button, compact) in [
        (1, &body.json_editor, &body.json_format, &compact_json),
        (2, &body.xml_editor, &body.xml_format, &compact_xml),
        (5, &body.text_editor, &body.text_format, &compact_xml),
    ] {
        body.mode.set_selected(mode);
        let request_buffer = view.buffer();
        for _ in 0..2 {
            request_buffer.set_text(compact);
            let main_loop = glib::MainLoop::new(None, false);
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), {
                let main_loop = main_loop.clone();
                move || main_loop.quit()
            });
            main_loop.run();
            let scroll = view
                .ancestor(gtk::ScrolledWindow::static_type())
                .unwrap()
                .downcast::<gtk::ScrolledWindow>()
                .unwrap();
            let horizontal = scroll.hadjustment();
            horizontal.set_value(horizontal.upper() - horizontal.page_size());
            button.grab_focus();
            button.emit_clicked();
            let main_loop = glib::MainLoop::new(None, false);
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), {
                let main_loop = main_loop.clone();
                move || main_loop.quit()
            });
            main_loop.run();
            assert!(request_buffer.line_count() > 1000);
            request_buffer.undo();
            let main_loop = glib::MainLoop::new(None, false);
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), {
                let main_loop = main_loop.clone();
                move || main_loop.quit()
            });
            main_loop.run();
            assert_eq!(&editor::buffer_text(view), compact);
        }
    }

    let html_source = "<!doctype html><html><head><meta charset=utf-8><title>Pakpos preview</title></head><body><main><h1>Hello <strong>HTML</strong></h1><p>Readable source<br>without rendering</p></main></body></html>";
    let html_response = pakpos::response::ResponseData {
        status: 200,
        reason: "OK".to_owned(),
        elapsed: std::time::Duration::from_millis(12),
        body_size: html_source.len() as u64,
        headers: vec![pakpos::response::ResponseHeader {
            name: "Content-Type".to_owned(),
            value: "text/html; charset=utf-8".to_owned(),
        }],
        body: pakpos::response::ResponseBody::Text {
            text: html_source.to_owned(),
            kind: pakpos::response::ResponseTextKind::Html,
            truncated: false,
            saved_path: None,
            notices: Vec::new(),
        },
    };
    ResponseWidgets {
        body: response.clone(),
        metadata: Label::new(None),
        headers: gtk::TextView::new(),
    }
    .display(&html_response);
    let formatted_html = html_response.display_body();
    assert_eq!(editor::buffer_text(&response.view), formatted_html);
    assert!(formatted_html.contains("\n      <h1>Hello <strong>HTML</strong></h1>"));
    assert_eq!(
        response
            .view
            .buffer()
            .downcast::<sourceview5::Buffer>()
            .unwrap()
            .language()
            .unwrap()
            .id(),
        "html"
    );
    buffer.place_cursor(&buffer.iter_at_line(1).unwrap());
    response
        .view
        .activate_action("fold.collapse", None)
        .unwrap();
    assert!(buffer.iter_at_line(2).unwrap().has_tag(&tag));
    response
        .view
        .activate_action("fold.expand-all", None)
        .unwrap();
    assert_eq!(editor::buffer_text(&response.view), formatted_html);
    body.mode.set_selected(1);
    body.json_editor.buffer().set_text(text);
    response_panel
        .first_child()
        .unwrap()
        .downcast::<Label>()
        .unwrap()
        .set_text("Response viewer — formatted HTML");
    let main_loop = glib::MainLoop::new(None, false);
    glib::timeout_add_local_once(std::time::Duration::from_millis(250), {
        let main_loop = main_loop.clone();
        move || main_loop.quit()
    });
    main_loop.run();
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, panels.width() as f64, panels.height() as f64);
    window
        .renderer()
        .unwrap()
        .render_texture(snapshot.to_node().unwrap(), None)
        .save_to_png(std::env::temp_dir().join("pakpos-html-response.png"))
        .unwrap();

    request_panel.remove(body_page);
    response_panel.remove(&response.root);
    window.destroy();
}

/// Kept separate from the headless default suite. Run on a GTK display with:
/// cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1
#[test]
#[ignore = "requires a GTK display"]
fn widget_lifetimes() {
    gtk::init().expect("GTK display required");
    sourceview5::init();
    let window = ApplicationWindow::builder().build();
    let _styles = install_styles(&gtk::prelude::WidgetExt::display(&window));
    let autosave = AutosaveTrigger::default();
    let sidebar = build_sidebar(autosave.clone());
    let (_, headers_box, header_rows) = build_headers_page(&autosave);
    let (body_page, body) = build_body_page(&window, &autosave);
    let response_body = build_source_viewer();
    let editor = Rc::new(EditorWidgetHandles {
        method: DropDown::from_strings(&["GET", "POST"]),
        url: Entry::new(),
        headers_box,
        header_rows,
        body,
        response: ResponseWidgets {
            body: response_body.clone(),
            metadata: Label::new(None),
            headers: gtk::TextView::new(),
        },
    });
    assert_has_accessible_label(&sidebar.collection_picker);
    assert_has_accessible_label(&sidebar.search);
    assert_has_accessible_label(&sidebar.new_request);
    assert!(gtk::test_accessible_has_role(
        &sidebar.status,
        gtk::AccessibleRole::Status,
    ));
    let header = editor.header_rows.borrow()[0].clone();
    assert_has_accessible_label(&header.enabled);
    assert_has_accessible_label(&header.name);
    assert_has_accessible_label(&header.value);
    assert_has_accessible_label(&editor.body.mode);
    assert_has_accessible_label(&editor.body.json_editor);
    assert_has_accessible_label(&editor.body.xml_editor);
    assert_has_accessible_label(&editor.body.text_editor);
    assert_request_body_formatting(&editor.body);
    assert!(response_body.view.shows_line_numbers());
    assert!(!response_body.view.is_editable());
    assert_eq!(response_body.view.wrap_mode(), gtk::WrapMode::None);
    assert!(!response_body.search_revealer.reveals_child());
    assert!(response_body.search_context.settings().wraps_around());
    let response_buffer = response_body
        .view
        .buffer()
        .downcast::<sourceview5::Buffer>()
        .expect("response viewer should use a GtkSourceView buffer");
    assert!(!response_buffer.is_highlight_syntax());
    set_source_content_type(&response_body.view, Some("application/json; charset=utf-8"));
    if sourceview5::LanguageManager::default()
        .language("json")
        .is_some()
    {
        assert_eq!(
            response_buffer.language().map(|language| language.id()),
            Some("json".into())
        );
        assert!(response_buffer.is_highlight_syntax());
    }
    set_source_content_type(
        &response_body.view,
        Some("application/x-pakpos-unsupported"),
    );
    assert!(response_buffer.language().is_none());
    assert!(!response_buffer.is_highlight_syntax());
    response_buffer.set_text("alpha beta alpha");
    response_body.search_revealer.set_reveal_child(true);
    response_body.search_entry.set_text("alpha");
    response_body
        .search_entry
        .emit_by_name::<()>("search-changed", &[]);
    assert_eq!(
        response_body
            .search_context
            .settings()
            .search_text()
            .as_deref(),
        Some("alpha")
    );
    let (start, end) = response_buffer
        .selection_bounds()
        .expect("the first search match should be selected");
    assert_eq!((start.offset(), end.offset()), (0, 5));
    response_body.search_entry.emit_activate();
    let (start, end) = response_buffer
        .selection_bounds()
        .expect("the next search match should be selected");
    assert_eq!((start.offset(), end.offset()), (11, 16));
    response_body.search_entry.emit_previous_match();
    let (start, end) = response_buffer
        .selection_bounds()
        .expect("the previous search match should be selected");
    assert_eq!((start.offset(), end.offset()), (0, 5));
    response_body.search_entry.emit_stop_search();
    assert!(!response_body.search_revealer.reveals_child());
    assert!(
        response_body
            .search_context
            .settings()
            .search_text()
            .is_none()
    );
    let form = editor.body.form_rows.borrow()[0].clone();
    assert_has_accessible_label(&form.enabled);
    assert_has_accessible_label(&form.name);
    assert_has_accessible_label(&form.value);
    let multipart = editor.body.multipart_rows.borrow()[0].clone();
    assert_has_accessible_label(&multipart.enabled);
    assert_has_accessible_label(&multipart.name);
    assert_has_accessible_label(&multipart.kind);
    assert_has_accessible_label(&multipart.value);
    drop(header);
    drop(form);
    drop(multipart);
    assert!(editor.body.json_editor.is_auto_indent());
    assert!(editor.body.json_editor.is_indent_on_tab());
    assert!(editor.body.json_editor.is_insert_spaces_instead_of_tabs());
    assert!(editor.body.json_editor.shows_line_numbers());
    assert!(!editor.body.json_editor.space_drawer().enables_matrix());
    assert_eq!(editor.body.json_editor.indent_width(), 2);
    assert_eq!(editor.body.json_editor.tab_width(), 2);
    assert!(editor.body.xml_editor.is_auto_indent());
    assert!(editor.body.xml_editor.is_indent_on_tab());
    assert!(editor.body.xml_editor.is_insert_spaces_instead_of_tabs());
    assert!(editor.body.xml_editor.shows_line_numbers());
    assert!(!editor.body.xml_editor.space_drawer().enables_matrix());
    assert_eq!(editor.body.xml_editor.indent_width(), 2);
    assert_eq!(editor.body.xml_editor.tab_width(), 2);
    assert!(editor.body.text_editor.is_auto_indent());
    assert!(editor.body.text_editor.is_indent_on_tab());
    assert!(editor.body.text_editor.is_insert_spaces_instead_of_tabs());
    assert!(editor.body.text_editor.shows_line_numbers());
    assert!(!editor.body.text_editor.space_drawer().enables_matrix());
    assert_eq!(editor.body.text_editor.indent_width(), 2);
    assert_eq!(editor.body.text_editor.tab_width(), 2);
    let source_buffer = editor
        .body
        .json_editor
        .buffer()
        .downcast::<sourceview5::Buffer>()
        .expect("JSON editor should use a GtkSourceView buffer");
    assert_eq!(
        source_buffer
            .language()
            .as_ref()
            .map(|language| language.id()),
        Some("json".into())
    );
    let xml_buffer = editor
        .body
        .xml_editor
        .buffer()
        .downcast::<sourceview5::Buffer>()
        .expect("XML editor should use a GtkSourceView buffer");
    assert_eq!(
        xml_buffer.language().as_ref().map(|language| language.id()),
        Some("xml".into())
    );
    let settings = gtk::Settings::default().expect("GTK settings should be available");
    let expected_scheme = json_style_scheme_id(
        settings.is_gtk_application_prefer_dark_theme(),
        settings.gtk_theme_name().as_deref(),
    );
    assert_eq!(
        source_buffer
            .style_scheme()
            .as_ref()
            .map(|scheme| scheme.id()),
        Some(expected_scheme.into())
    );
    let text_buffer = editor
        .body
        .text_editor
        .buffer()
        .downcast::<sourceview5::Buffer>()
        .expect("Plain-text editor should use a GtkSourceView buffer");
    assert!(text_buffer.language().is_none());
    assert_eq!(
        text_buffer
            .style_scheme()
            .as_ref()
            .map(|scheme| scheme.id()),
        Some(expected_scheme.into())
    );
    assert_eq!(
        xml_buffer.style_scheme().as_ref().map(|scheme| scheme.id()),
        Some(expected_scheme.into())
    );
    assert_source_folding(&editor.body.json_editor, "{\n  \"name\": \"Pakpos\"\n}");
    assert_source_folding(
        &editor.body.xml_editor,
        "<root>\n  <name>Pakpos</name>\n</root>",
    );
    assert_source_folding(&editor.body.text_editor, "root\n  child\n  value");
    assert_nested_folding_and_edits(&editor.body.json_editor);
    set_source_content_type(&response_body.view, Some("application/json"));
    assert_source_folding(&response_body.view, "{\n  \"name\": \"Pakpos\"\n}");
    response_buffer.set_text("{\n  \"name\": \"hidden match\"\n}");
    response_buffer.place_cursor(&response_buffer.start_iter());
    response_body
        .view
        .activate_action("fold.collapse-all", None)
        .unwrap();
    response_body.search_entry.set_text("hidden match");
    response_body
        .search_entry
        .emit_by_name::<()>("search-changed", &[]);
    let tag = response_buffer
        .tag_table()
        .lookup("pakpos-folded-lines")
        .unwrap();
    assert!(!response_buffer.iter_at_line(1).unwrap().has_tag(&tag));
    assert!(response_buffer.selection_bounds().is_some());
    response_body.reset_search();
    assert_rendered_folding(&body_page, &editor.body, &response_body);
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&sidebar.root);
    root.append(&editor.headers_box);
    root.append(&body_page);
    window.set_child(Some(&root));

    let first_header = editor.header_rows.borrow()[0].row.downgrade();
    let first_form = editor.body.form_rows.borrow()[0].row.downgrade();
    let first_field = editor.body.multipart_rows.borrow()[0].row.downgrade();
    let xml_request = Request {
        body: pakpos::models::RequestBody::Xml("<message>hello</message>".to_owned()),
        ..Request::default()
    };
    apply_request(
        &xml_request,
        &editor.method,
        &editor.url,
        &editor.headers_box,
        &editor.header_rows,
        &editor.body,
    );
    assert_eq!(
        collect_request(
            &editor.method,
            &editor.url,
            &editor.header_rows,
            &editor.body,
        )
        .unwrap()
        .body,
        xml_request.body
    );
    assert!(
        first_header.upgrade().is_none(),
        "replaced header row leaked"
    );
    assert!(
        first_form.upgrade().is_none(),
        "replaced URL-encoded form row leaked"
    );
    assert!(
        first_field.upgrade().is_none(),
        "replaced multipart row leaked"
    );

    add_header_row(&editor.headers_box, &editor.header_rows, &autosave);
    let row = editor.header_rows.borrow().last().unwrap().row.clone();
    let weak = row.downgrade();
    row.last_child()
        .unwrap()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    drop(row);
    assert!(weak.upgrade().is_none(), "removed header row leaked");

    add_form_urlencoded_row(&editor.body.form_box, &editor.body.form_rows, &autosave);
    let row = editor.body.form_rows.borrow().last().unwrap().row.clone();
    let weak = row.downgrade();
    row.last_child()
        .unwrap()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    drop(row);
    assert!(
        weak.upgrade().is_none(),
        "removed URL-encoded form row leaked"
    );

    add_multipart_row(
        &editor.body.multipart_box,
        &editor.body.multipart_rows,
        &window,
        &autosave,
    );
    let row = editor
        .body
        .multipart_rows
        .borrow()
        .last()
        .unwrap()
        .row
        .clone();
    let weak = row.downgrade();
    row.last_child()
        .unwrap()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    drop(row);
    assert!(weak.upgrade().is_none(), "removed multipart row leaked");

    let state = Rc::new(RequestState::default());
    let mut session = CollectionSession::empty(CollectionSummary::new("Test"));
    let id = session.add_request();
    state.update(Action::SetCollection(session));
    render_request_buttons(&state, &sidebar, &editor);
    let row = sidebar.request_rows.borrow()[&id]
        .clone()
        .upcast::<gtk::Widget>();
    let weak_row = row.downgrade();
    let menu = build_request_context_menu(&row, id, &state, &sidebar, &editor);
    let weak_menu = menu.downgrade();
    menu.emit_by_name::<()>("closed", &[]);
    drop(menu);
    let context = glib::MainContext::default();
    while context.pending() {
        context.iteration(false);
    }
    assert!(weak_menu.upgrade().is_none(), "closed menu leaked");
    drop(row);
    render_request_buttons(&state, &sidebar, &editor);
    assert!(weak_row.upgrade().is_none(), "replaced sidebar row leaked");

    let row = sidebar.request_rows.borrow()[&id]
        .clone()
        .upcast::<gtk::Widget>();
    let weak_row = row.downgrade();
    let menu = build_request_context_menu(&row, id, &state, &sidebar, &editor);
    assert!(menu.parent().is_some(), "context menu was not parented");
    drop(row);
    render_request_buttons(&state, &sidebar, &editor);
    assert!(
        menu.parent().is_none(),
        "context menu remained parented during request-list rebuild"
    );
    assert!(
        weak_row.upgrade().is_none(),
        "row with an open context menu leaked during replacement"
    );
    drop(menu);

    // An unchanged capture must consume the autosave flag, including a no-op save.
    state.collection.borrow_mut().take();
    state.autosave_requested.set(true);
    autosave_current_collection(&state, &sidebar, &editor, &sidebar.status, &window);
    assert!(!state.autosave_requested.get());

    let row = sidebar.requests.first_child().unwrap();
    let menu = build_request_context_menu(&row, id, &state, &sidebar, &editor);
    drop(row);
    drop(sidebar);
    assert!(
        menu.parent().is_none(),
        "context menu remained parented during sidebar teardown"
    );
    window.destroy();
}
