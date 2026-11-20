# Vendored crates

- `windows-reactor` 0.101.0 (MIT OR Apache-2.0), Microsoft's WinUI 3 library from windows-rs at
  commit 63a5e3d9b9afbd57c382d0a4f5a367f134e9dc25, taken whole from Espalier's copy, which carries
  Blurman's changes and its own:
  - `src/native/winui/window/active_backdrop.rs`: Mica and Acrylic stay in their active look
    while another window is in front (WinUI's own backdrops go flat). It uses the composition
    backdrop controllers with a configuration that always says the window is active.
  - `src/native/winui/window/mod.rs`: the controller lets go of the window in its `Closed` event,
    while the window is still whole (released after XAML has torn the window down, it crashes in
    XAML, so a window closed to the tray took the app down). Where Windows cannot draw the
    backdrop (Mica on Windows 10), the window takes WinUI's own backdrop instead. These two came
    from Blurman after Espalier took its copy.
  - `src/declaration.rs`: seven more `ThemeBrush` values (SecondaryText, TertiaryText,
    SubtleSecondary, Divider, SystemCaution, SystemCautionBackground, SystemSuccess).
  - `src/native/winui/relations.rs` (`list_template`): a list item's text is also its name for
    screen readers.
  - `src/generated_declarations.rs` and `src/generated.rs`: `NavigationView::resource_overrides`.

  Its sibling crates (windows-core, windows-window and the others) come from the same git commit.
