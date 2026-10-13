# GTK interface verification

Date: 2026-09-21. Revision: `619c4b5` plus the accessibility changes described
below. Environment: Arch Linux, Hyprland 0.56.2 on Wayland, GTK 4.22.4.

## Results

- The release build was inspected at exactly 900 × 600 in Adwaita light and dark
  themes. The collection sidebar, request controls, header editor, request tabs,
  response tabs, dividers, and window controls remained visible and usable without
  clipping. Both panes remained independently resizable. All application-defined
  margins and text-view padding are capped at 8 pixels.
- Ctrl+O focused the native collection picker and Space opened it. Ctrl+N opened the
  collection dialog with focus in its name field. Enter uses the dialog's default
  action once input is valid, Tab follows native GTK focus order, and Escape now
  closes both custom create and rename dialogs without changing data.
- Ctrl+Enter remains registered for Send and Ctrl+Shift+N for New HTTP Request.
  Destructive request deletion continues to use a confirmation dialog whose safe
  default is Cancel.
- Icon-only buttons, entries, dropdowns, editable body fields, response views, and
  dynamic header/multipart controls now have explicit GTK accessible labels. The
  collection status and transient notification containers use the GTK status role,
  and the Send and collection-picker controls expose their keyboard shortcuts.
- Network, SQLite, import, and export effects continue to execute through background
  workers. Collection loading and the inspected dialogs remained responsive during
  the pass; no GTK callback was found performing those operations synchronously.
- The display-dependent GTK regression test passed. It now checks the explicit
  accessible-label properties and status role in addition to theme selection,
  editor configuration, widget cleanup, and autosave-flag consumption.

The visual checks intentionally use native GTK themes without application color
overrides. They do not replace the separately skipped release RSS and 1 GiB transfer
measurements.

## Reproduction

Build the release binary, launch it once with each GTK theme, and resize the window
to 900 × 600. Run the display-dependent regression test from a desktop session:

```sh
cargo build --release
cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1
```

## Libadwaita integration verification — 2026-10-10

Environment: GTK 4.24.1, libadwaita 1.10.0, and GtkSourceView 5.22.0.
The build targets GTK 4.10+ and libadwaita 1.4+.

Pakpos now initializes through `adw::Application` and uses an
`adw::ApplicationWindow`, `adw::HeaderBar`, and `adw::ToolbarView`. The existing
resizable sidebar and request/response panes remain inside the toolbar content.
Request notifications use `adw::ToastOverlay`; messages are plain text, a new
message dismisses its predecessor, and errors retain the longer timeout.
The interface uses Adwaita typography, and delete actions use its error color.

Formatting, strict Clippy across all targets/features, all 120 headless tests,
and the release build passed. The three display-dependent tests below also
passed with warnings treated as fatal and the Cairo renderer:

```sh
GSK_RENDERER=cairo G_DEBUG=fatal-warnings cargo test --bin pakpos gobject_ownership -- --ignored --test-threads=1 --nocapture
GSK_RENDERER=cairo G_DEBUG=fatal-warnings cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1 --nocapture
GSK_RENDERER=cairo G_DEBUG=fatal-warnings cargo test --bin pakpos collection_switch_lifetimes -- --ignored --test-threads=1 --nocapture
```

The ownership test checks the native shell, live light/dark changes in the JSON,
XML, plain-text, and response editors, toast replacement and disposal, and full
window cleanup. The widget test checks accessibility, formatting-error toasts,
and editor behavior. The collection test checks cleanup across 100 collection
switches. The earlier manual 900 × 600 visual pass above predates this integration.

## Default editor scheme experiment — 2026-10-10

Removed application style-scheme selection and light/dark handlers from both
request and response source editors. The corresponding scheme-selection test
and forced-Adwaita assertions were removed; syntax-language configuration remains.

A temporary diagnostic in the existing GTK ownership test inspected all four
source buffers and captured the application in forced light and dark modes.
GtkSourceView selected `classic` for JSON, XML, plain text, and responses in both
modes. The shell changed appearance, but the editor retained a white background;
response text became unreadable in the dark-mode capture. The diagnostic was
removed after inspection. Application style-scheme selection was subsequently
restored to fix the dark-mode readability failure; editors again use `Adwaita`
and `Adwaita-dark`, synchronized with libadwaita's style manager.

Formatting, strict Clippy, all 119 headless tests, the release build, and the GTK
ownership test passed. The light and dark snapshots are in the temporary directory
as `pakpos-default-style-light.png` and `pakpos-default-style-dark.png`.

After restoring scheme synchronization, formatting, strict Clippy, all 120
headless tests, the release build, and the GTK ownership test passed again. The
GTK test verifies live light/dark switching in all four source editors and
disconnection of their handlers when windows are disposed.

## Folding markers while typing — 2026-10-13

Fold markers remain visible during debounced region discovery. Cached positions
follow inserted and deleted lines; deleted headers and content replacement remove
their markers. Discovery redraws the folding gutter only when regions change.
Folding actions validate the current text before hiding lines, including during
the debounce delay. Edits continue to expand collapsed sections.

Formatting, strict Clippy across all targets/features, all 123 headless tests,
and the release build passed. The display-dependent `widget_lifetimes` and
`gobject_ownership` tests also passed with `GSK_RENDERER=cairo` and
`G_DEBUG=fatal-warnings`. The widget test checks markers
during consecutive keystrokes, newline insertion and undo, header deletion,
debounced boundary validation, folding before the refresh, and content replacement.
