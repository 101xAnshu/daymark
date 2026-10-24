# Vendored Slint 1.8 sources

These are copies of `i-slint-core` and `i-slint-backend-winit` from Slint 1.8.0.
They contain one small functional backport and three compile-warning cleanups
that do not change behaviour:

- `i-slint-core`: add the no-op `WindowAdapterInternal::start_window_move()` hook
  and the public `Window::start_window_move()` method.
- `i-slint-backend-winit`: implement that hook by calling
  `winit::window::Window::drag_window()`.

Later Slint versions provide this through `WindowMoveArea`; Slint 1.8 does not.
The backport lets Daymark provide a frameless custom title bar with native
Wayland/X11 window dragging while staying on the size-optimized Slint version.

The warning cleanups are an unnecessary `mut`, and two explicit `'_` lifetimes
in model iterator signatures required by newer compilers.

The vendored sources retain their upstream license files and copyright notices
(Slint is dual-licensed GPL-3.0-only / royalty-free; see the files in each
crate).
