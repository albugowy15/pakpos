use super::*;
use editor::{EditorWidgetHandles, add_header_row, add_multipart_row};
use pakpos::{app::CollectionSession, models::Request};
use sidebar::{build_request_context_menu, render_request_buttons};

/// Kept separate from the headless default suite. Run on a GTK display with:
/// cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1
#[test]
#[ignore = "requires a GTK display"]
fn widget_lifetimes() {
    gtk::init().expect("GTK display required");
    let window = ApplicationWindow::builder().build();
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
    });
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&sidebar.root);
    root.append(&editor.headers_box);
    root.append(&body_page);
    window.set_child(Some(&root));

    let first_header = editor.header_rows.borrow()[0].row.downgrade();
    let first_field = editor.body.multipart_rows.borrow()[0].row.downgrade();
    apply_request(
        &Request::default(),
        &editor.method,
        &editor.url,
        &editor.headers_box,
        &editor.header_rows,
        &editor.body,
    );
    assert!(
        first_header.upgrade().is_none(),
        "replaced header row leaked"
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

    // An unchanged capture must consume the autosave flag, including a no-op save.
    state.collection.borrow_mut().take();
    state.autosave_requested.set(true);
    autosave_current_collection(&state, &sidebar, &editor, &sidebar.status, &window);
    assert!(!state.autosave_requested.get());
    window.destroy();
}
