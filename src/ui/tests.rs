//! GTK integration and widget-lifetime tests.
//!
//! Pure UI planning is tested in its owning modules. Tests here cover behavior
//! that requires real GTK objects, including theme selection and release of
//! transient widget graphs. Display-dependent coverage stays ignored in the
//! default headless suite and documents the command needed to run it manually.

use super::*;
use editor::{
    EditorWidgetHandles, add_form_urlencoded_row, add_header_row, add_multipart_row,
    json_style_scheme_id,
};
use pakpos::{app::CollectionSession, models::Request};
use sidebar::{build_request_context_menu, render_request_buttons};
use sourceview5::prelude::*;

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

/// Kept separate from the headless default suite. Run on a GTK display with:
/// cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1
#[test]
#[ignore = "requires a GTK display"]
fn widget_lifetimes() {
    gtk::init().expect("GTK display required");
    sourceview5::init();
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
    assert_has_accessible_label(&editor.body.text_editor);
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
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&sidebar.root);
    root.append(&editor.headers_box);
    root.append(&body_page);
    window.set_child(Some(&root));

    let first_header = editor.header_rows.borrow()[0].row.downgrade();
    let first_form = editor.body.form_rows.borrow()[0].row.downgrade();
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

    // An unchanged capture must consume the autosave flag, including a no-op save.
    state.collection.borrow_mut().take();
    state.autosave_requested.set(true);
    autosave_current_collection(&state, &sidebar, &editor, &sidebar.status, &window);
    assert!(!state.autosave_requested.get());
    window.destroy();
}
