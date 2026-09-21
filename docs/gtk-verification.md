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
