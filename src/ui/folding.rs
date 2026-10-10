//! Line folding shared by request editors and the response source viewer.
//!
//! Folding only applies an invisible text tag: the buffer, undo history, and
//! request source remain intact. Region discovery is debounced while typing.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
    time::Duration,
};

use gtk::{gio, glib, prelude::*};
use sourceview5::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    start: i32,
    end: i32,
}

fn regions(text: &str, language: Option<&str>) -> Vec<Region> {
    let mut regions = match language {
        Some("json") => bracket_regions(text),
        Some("xml" | "html") => xml_regions(text, language == Some("html")),
        _ => indentation_regions(text),
    };
    // A gutter cell can represent only one region. Prefer the outermost region
    // when multiple opening delimiters occur on the same line.
    regions.sort_unstable_by_key(|region| (region.start, -region.end));
    regions.dedup_by_key(|region| region.start);
    regions
}

fn bracket_regions(text: &str) -> Vec<Region> {
    let mut regions = Vec::new();
    let mut stack = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut line = 0;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
        }
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '{' | '[' => stack.push((ch, line)),
            '}' | ']' => {
                if let Some((open, start)) = stack.pop() {
                    if matches!((open, ch), ('{', '}') | ('[', ']')) {
                        if line > start {
                            regions.push(Region { start, end: line });
                        }
                    } else {
                        stack.clear();
                    }
                }
            }
            _ => {}
        }
    }
    regions
}

fn xml_regions(text: &str, html: bool) -> Vec<Region> {
    let mut regions = Vec::new();
    let mut stack = Vec::new();
    let mut rest = text;
    let mut line = 0;
    while let Some(open) = rest.find('<') {
        line += rest[..open].bytes().filter(|&ch| ch == b'\n').count() as i32;
        rest = &rest[open..];
        let start = line;
        let special = if rest.starts_with("<!--") {
            Some("-->")
        } else if rest.starts_with("<![CDATA[") {
            Some("]]>")
        } else if rest.starts_with("<?") {
            Some("?>")
        } else {
            None
        };
        let end = if let Some(terminator) = special {
            rest.find(terminator).map(|at| at + terminator.len())
        } else {
            let mut quote = None;
            let mut subset = 0;
            rest.char_indices().find_map(|(at, ch)| {
                if let Some(active) = quote {
                    if ch == active {
                        quote = None;
                    }
                } else {
                    match ch {
                        '\'' | '"' => quote = Some(ch),
                        '[' => subset += 1,
                        ']' => subset -= 1,
                        '>' if subset == 0 => return Some(at + 1),
                        _ => {}
                    }
                }
                None
            })
        };
        let Some(end) = end else { break };
        let tag = &rest[..end];
        line += tag.bytes().filter(|&ch| ch == b'\n').count() as i32;
        rest = &rest[end..];
        if special.is_some() || tag.starts_with("<!") {
            continue;
        }
        let closing = tag.starts_with("</");
        let name = tag[if closing { 2 } else { 1 }..]
            .split(|ch: char| ch.is_whitespace() || ch == '/' || ch == '>')
            .next()
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        if closing {
            if let Some((open_name, start)) = stack.pop() {
                if open_name == name {
                    if line > start {
                        regions.push(Region { start, end: line });
                    }
                } else {
                    stack.clear();
                }
            }
        } else if !tag.trim_end_matches('>').trim_end().ends_with('/')
            && !(html
                && matches!(
                    name,
                    "area"
                        | "base"
                        | "br"
                        | "col"
                        | "embed"
                        | "hr"
                        | "img"
                        | "input"
                        | "link"
                        | "meta"
                        | "param"
                        | "source"
                        | "track"
                        | "wbr"
                ))
        {
            stack.push((name, start));
        }
    }
    regions
}

fn indentation_regions(text: &str) -> Vec<Region> {
    let mut regions = Vec::new();
    let mut stack = Vec::new();
    let mut previous = None;
    for (line, text) in text.lines().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let line = line as i32;
        let indent = text
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .map(|ch| if ch == '\t' { 4 } else { 1 })
            .sum::<usize>();
        if let Some((previous_line, previous_indent)) = previous {
            while stack
                .last()
                .is_some_and(|&(header_indent, _)| indent <= header_indent)
            {
                let (_, start) = stack.pop().unwrap();
                regions.push(Region {
                    start,
                    end: previous_line,
                });
            }
            if indent > previous_indent {
                stack.push((previous_indent, previous_line));
            }
        }
        previous = Some((line, indent));
    }
    if let Some((end, _)) = previous {
        regions.extend(stack.into_iter().map(|(_, start)| Region { start, end }));
    }
    regions
}

struct Folding {
    view: glib::WeakRef<sourceview5::View>,
    buffer: glib::WeakRef<sourceview5::Buffer>,
    renderer: glib::WeakRef<sourceview5::GutterRendererText>,
    line_numbers: glib::WeakRef<sourceview5::GutterRendererText>,
    tag: gtk::TextTag,
    regions: RefCell<Vec<Region>>,
    collapsed: RefCell<BTreeSet<i32>>,
    dirty: Cell<bool>,
    refresh: RefCell<Option<glib::SourceId>>,
}

impl Folding {
    fn ensure_regions(&self) {
        if !self.dirty.replace(false) {
            return;
        }
        let Some(buffer) = self.buffer.upgrade() else {
            return;
        };
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
        let language = buffer.language().map(|language| language.id());
        *self.regions.borrow_mut() = regions(&text, language.as_deref());
    }

    fn expand_all(&self) {
        if self.collapsed.borrow().is_empty() {
            return;
        }
        self.collapsed.borrow_mut().clear();
        self.apply();
    }

    fn invalidate(self: &Rc<Self>) {
        self.expand_all();
        self.regions.borrow_mut().clear();
        self.dirty.set(true);
        if let Some(refresh) = self.refresh.borrow_mut().take() {
            refresh.remove();
        }
        let weak = Rc::downgrade(self);
        *self.refresh.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(150),
            move || {
                if let Some(folding) = weak.upgrade() {
                    folding.refresh.borrow_mut().take();
                    folding.ensure_regions();
                    if let Some(renderer) = folding.renderer.upgrade() {
                        renderer.queue_draw();
                    }
                }
            },
        ));
        if let Some(renderer) = self.renderer.upgrade() {
            renderer.queue_draw();
        }
    }

    fn apply(&self) {
        let Some(buffer) = self.buffer.upgrade() else {
            return;
        };
        buffer.remove_tag(&self.tag, &buffer.start_iter(), &buffer.end_iter());
        let collapsed = self.collapsed.borrow();
        if let Some(numbers) = self.line_numbers.upgrade() {
            numbers.queue_draw();
        }
        for region in self
            .regions
            .borrow()
            .iter()
            .filter(|r| collapsed.contains(&r.start))
        {
            let Some(start) = buffer.iter_at_line(region.start + 1) else {
                continue;
            };
            let end = buffer
                .iter_at_line(region.end + 1)
                .unwrap_or_else(|| buffer.end_iter());
            buffer.apply_tag(&self.tag, &start, &end);
        }
        if let Some(renderer) = self.renderer.upgrade() {
            renderer.queue_draw();
        }
        // Invisible tags change the scrollable extent. Request layout before
        // the next snapshot so the scrollbars receive their new allocation.
        if let Some(view) = self.view.upgrade() {
            view.queue_resize();
        }
    }

    fn collapse(&self, region: Region) {
        let Some(buffer) = self.buffer.upgrade() else {
            return;
        };
        let cursor = buffer.iter_at_mark(&buffer.get_insert());
        // Avoid leaving the caret (or a selection) in invisible text.
        if (cursor.line() > region.start && cursor.line() <= region.end)
            || buffer.selection_bounds().is_some()
        {
            let mut header = buffer.iter_at_line(region.start).unwrap();
            header.forward_to_line_end();
            buffer.place_cursor(&header);
        }
        self.collapsed.borrow_mut().insert(region.start);
    }

    fn toggle(&self, line: i32) {
        self.ensure_regions();
        let region = self
            .regions
            .borrow()
            .iter()
            .find(|r| r.start == line)
            .copied();
        if let Some(region) = region {
            if !self.collapsed.borrow_mut().remove(&region.start) {
                self.collapse(region);
            }
            self.apply();
        }
    }

    fn at_cursor(&self, collapse: bool) {
        self.ensure_regions();
        let Some(buffer) = self.buffer.upgrade() else {
            return;
        };
        let line = buffer.iter_at_mark(&buffer.get_insert()).line();
        let region = self
            .regions
            .borrow()
            .iter()
            .rev()
            .find(|r| {
                r.start <= line
                    && line <= r.end
                    && self.collapsed.borrow().contains(&r.start) != collapse
            })
            .copied();
        if let Some(region) = region {
            if collapse {
                self.collapse(region);
            } else {
                self.collapsed.borrow_mut().remove(&region.start);
            }
            self.apply();
        }
    }

    fn collapse_all(&self) {
        self.ensure_regions();
        let regions = self.regions.borrow().clone();
        for region in regions {
            self.collapse(region);
        }
        self.apply();
    }

    fn reveal(&self, line: i32) {
        let mut collapsed = self.collapsed.borrow_mut();
        if collapsed.is_empty() {
            return;
        }
        let before = collapsed.len();
        for region in self.regions.borrow().iter() {
            if region.start < line && line <= region.end {
                collapsed.remove(&region.start);
            }
        }
        let changed = before != collapsed.len();
        drop(collapsed);
        if changed {
            self.apply();
        }
    }
}

pub(super) fn install(view: &sourceview5::View) {
    let buffer = view
        .buffer()
        .downcast::<sourceview5::Buffer>()
        .expect("source view should use a source buffer");
    let renderer = sourceview5::GutterRendererText::builder()
        .name("pakpos-folding-gutter")
        .width_request(20)
        .xpad(2)
        .tooltip_text("Collapse or expand lines (Ctrl+Alt+Left / Right)")
        .build();
    super::set_accessible_label(&renderer, "Collapse or expand source lines");
    let tag = gtk::TextTag::builder()
        .name("pakpos-folded-lines")
        .invisible(true)
        .build();
    buffer.tag_table().add(&tag);
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk::TextWindowType::Left);
    let mut child = gutter.first_child();
    let native_line_numbers = loop {
        let widget = child.expect("line numbers should be enabled before installing folding");
        if let Ok(numbers) = widget.clone().downcast::<sourceview5::GutterRendererText>() {
            break numbers;
        }
        child = widget.next_sibling();
    };
    let line_numbers = sourceview5::GutterRendererText::builder()
        .name("pakpos-folding-line-numbers")
        .xalign(1.0)
        .xpad(4)
        .build();
    line_numbers.add_css_class("dim-label");
    let folding = Rc::new(Folding {
        view: view.downgrade(),
        buffer: buffer.downgrade(),
        renderer: renderer.downgrade(),
        line_numbers: line_numbers.downgrade(),
        tag,
        regions: RefCell::default(),
        collapsed: RefCell::default(),
        dirty: Cell::new(true),
        refresh: RefCell::default(),
    });
    renderer.connect_query_data({
        let folding = folding.clone();
        move |renderer, _, line| {
            let is_region = folding
                .regions
                .borrow()
                .binary_search_by_key(&(line as i32), |r| r.start)
                .is_ok();
            let hidden = folding
                .buffer
                .upgrade()
                .and_then(|buffer| buffer.iter_at_line(line as i32))
                .is_some_and(|iter| iter.has_tag(&folding.tag));
            renderer.set_text(if !is_region || hidden {
                ""
            } else if folding.collapsed.borrow().contains(&(line as i32)) {
                "▸"
            } else {
                "▾"
            });
        }
    });
    renderer.connect_query_activatable({
        let folding = folding.clone();
        move |_, iter, _| {
            !iter.has_tag(&folding.tag)
                && folding
                    .regions
                    .borrow()
                    .binary_search_by_key(&iter.line(), |r| r.start)
                    .is_ok()
        }
    });
    renderer.connect_activate({
        let folding = folding.clone();
        move |_, iter, _, button, _, presses| {
            if button == 1 && presses == 1 {
                folding.toggle(iter.line());
            }
        }
    });
    gutter.insert(&renderer, -20);
    gutter.insert(&line_numbers, -31);
    // Keep one line-number column for both states. Swapping renderers on every
    // fold changed the gutter width and invalidated scrollbar allocations.
    // The native renderer also draws hidden lines over the next visible line.
    native_line_numbers.set_visible(false);
    line_numbers.connect_query_data({
        let folding = folding.clone();
        move |numbers, _, line| {
            let hidden = folding
                .buffer
                .upgrade()
                .and_then(|buffer| buffer.iter_at_line(line as i32))
                .is_some_and(|iter| iter.has_tag(&folding.tag));
            numbers.set_text(
                if hidden {
                    String::new()
                } else {
                    (line + 1).to_string()
                }
                .as_str(),
            );
        }
    });
    line_numbers.connect_query_activatable({
        let native = native_line_numbers.downgrade();
        let tag = folding.tag.clone();
        move |_, iter, area| {
            !iter.has_tag(&tag)
                && native
                    .upgrade()
                    .is_some_and(|numbers| numbers.query_activatable(iter, area))
        }
    });
    line_numbers.connect_activate({
        let native = native_line_numbers.downgrade();
        move |_, iter, area, button, modifiers, presses| {
            if let Some(numbers) = native.upgrade() {
                numbers
                    .emit_by_name::<()>("activate", &[iter, area, &button, &modifiers, &presses]);
            }
        }
    });
    let resize_numbers = {
        let numbers = line_numbers.downgrade();
        move |buffer: &sourceview5::Buffer| {
            if let Some(numbers) = numbers.upgrade() {
                let text = buffer.line_count().max(99).to_string();
                let (width, _) = GutterRendererTextExt::measure(&numbers, &text);
                numbers.set_width_request(width + 8);
            }
        }
    };
    resize_numbers(&buffer);
    buffer.connect_changed(resize_numbers);
    buffer.connect_changed({
        let folding = folding.clone();
        move |_| folding.invalidate()
    });
    buffer.connect_language_notify({
        let folding = folding.clone();
        move |_| folding.invalidate()
    });
    // Reveal before GTK calculates a keyboard deletion range. Revealing only
    // in delete-range is too late: that range may already span hidden lines.
    view.connect_backspace({
        let folding = folding.clone();
        move |view| {
            if view.is_editable() {
                folding.expand_all();
            }
        }
    });
    view.connect_delete_from_cursor({
        let folding = folding.clone();
        move |view, _, _| {
            if view.is_editable() {
                folding.expand_all();
            }
        }
    });
    // Programmatic edits also discard hidden ranges before changing the text.
    buffer.connect_insert_text({
        let folding = folding.clone();
        move |_, _, _| folding.expand_all()
    });
    buffer.connect_delete_range({
        let folding = folding.clone();
        move |_, _, _| folding.expand_all()
    });
    // Search and programmatic navigation move marks into hidden text.
    buffer.connect_mark_set({
        let folding = folding.clone();
        move |_, iter, mark| {
            if matches!(mark.name().as_deref(), Some("insert" | "selection_bound")) {
                folding.reveal(iter.line());
            }
        }
    });

    let actions = gio::SimpleActionGroup::new();
    let shortcuts = gtk::ShortcutController::new();
    let menu = gio::Menu::new();
    for (name, label, trigger) in [
        ("collapse", "Collapse section", "<Control><Alt>Left"),
        ("expand", "Expand section", "<Control><Alt>Right"),
        ("collapse-all", "Collapse all", "<Control><Alt><Shift>Left"),
        ("expand-all", "Expand all", "<Control><Alt><Shift>Right"),
    ] {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate({
            let folding = folding.clone();
            move |_, _| match name {
                "collapse" => folding.at_cursor(true),
                "expand" => folding.at_cursor(false),
                "collapse-all" => folding.collapse_all(),
                _ => folding.expand_all(),
            }
        });
        actions.add_action(&action);
        let detailed = format!("fold.{name}");
        menu.append(Some(label), Some(&detailed));
        shortcuts.add_shortcut(gtk::Shortcut::new(
            Some(gtk::ShortcutTrigger::parse_string(trigger).expect("valid folding shortcut")),
            Some(gtk::NamedAction::new(&detailed)),
        ));
    }
    let extra_menu = gio::Menu::new();
    if let Some(existing) = view.extra_menu() {
        extra_menu.append_section(None, &existing);
    }
    extra_menu.append_section(Some("Line folding"), &menu);
    view.set_extra_menu(Some(&extra_menu));
    view.insert_action_group("fold", Some(&actions));
    view.add_controller(shortcuts);
    folding.ensure_regions();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_folds_nested_arrays_and_objects_and_ignores_strings() {
        let text = "{\n  \"data\": [\n    {\n      \"value\": \"escaped \\\" } [\"\n    }\n  ]\n}";
        assert_eq!(
            regions(text, Some("json")),
            vec![
                Region { start: 0, end: 6 },
                Region { start: 1, end: 5 },
                Region { start: 2, end: 4 },
            ]
        );
    }

    #[test]
    fn json_rejects_single_line_unclosed_and_mismatched_sections() {
        for text in ["{\"value\": []}", "{\n  \"value\": 1", "{\n]\n}"] {
            assert!(regions(text, Some("json")).is_empty());
        }
        assert_eq!(
            regions("{[\n1\n]}", Some("json")),
            vec![Region { start: 0, end: 2 }]
        );
    }

    #[test]
    fn xml_folds_elements_and_skips_markup_in_attributes_comments_and_cdata() {
        let text = "<?xml version=\"1.0\"?>\n<root>\n  <!-- <fake>\n  </fake> -->\n  <child attr=\">\">\n    <![CDATA[<fake>\n    </fake>]]>\n    <empty />\n  </child>\n</root>";
        assert_eq!(
            regions(text, Some("xml")),
            vec![Region { start: 1, end: 9 }, Region { start: 4, end: 8 },]
        );
        assert!(regions("<root>\n</other>", Some("xml")).is_empty());
    }

    #[test]
    fn indentation_folds_nested_blocks_with_blank_lines_and_tabs() {
        let text = "root\n\tchild\n\t\tvalue\n\n\tsibling\nnext\n  value";
        assert_eq!(
            regions(text, None),
            vec![
                Region { start: 0, end: 4 },
                Region { start: 1, end: 2 },
                Region { start: 5, end: 6 },
            ]
        );
        assert!(regions("alpha\nbeta\n", None).is_empty());
    }

    #[test]
    fn html_void_elements_do_not_disable_folding_and_xml_names_stay_regular() {
        assert_eq!(
            regions("<div>\n<img src=\"test\">\n<br>\n</div>", Some("html")),
            vec![Region { start: 0, end: 3 }]
        );
        assert_eq!(
            regions("<input>\nvalue\n</input>", Some("xml")),
            vec![Region { start: 0, end: 2 }]
        );
    }
}
