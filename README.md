# Daymark

A quiet, local screen time journal for Ubuntu GNOME — the useful part of Screen
Time, without an account, a cloud, a web view or a permanent performance tax.

[Source and issues](https://github.com/101xAnshu/daymark)

![Daymark, light theme](docs/daymark-light.png)

Daymark shows one day at a time: how long you used the computer, how that
compares with your own history, the shape of the week, the rhythm of the day,
and which applications received the time. Everything is stored on this machine.

| Dark | History | Settings |
| --- | --- | --- |
| ![Dark theme](docs/daymark-dark.png) | ![History](docs/daymark-history.png) | ![Settings](docs/daymark-settings.png) |
| **Setup** | **First run** | |
| ![Setup](docs/daymark-setup.png) | ![First run](docs/daymark-first-run.png) | |

## Install

### Requirements

- Ubuntu 26.04 with GNOME Shell 50.1 was used for the tested build. The source
  targets GNOME Shell 45–50; build from source on older distributions (the
  prebuilt artifact links the build machine's glibc).
- Wayland is the default target; the same build also includes the X11 window
  backend.
- To build: Rust 1.85+, `build-essential`, and `pkg-config`.

```bash
sudo apt install build-essential pkg-config
cargo build --release
./target/release/daymark
```

For a user-level installation (no root):

```bash
./install.sh
```

This copies the binary to `~/.local/bin/daymark`, installs the icon, the
desktop entry, an autostart entry, and the small GNOME companion extension. It
also enables the extension for your user.

**First install needs one sign-out and sign-in.** GNOME on Wayland only scans
for new Shell extensions at login. Daymark's setup sheet explains this; the
extension is already installed and will begin reporting after the next login.
Closing the window (the ✕ control or Alt+F4) does not quit Daymark: tracking
continues in a genuinely invisible background process, and the window can be
reopened from the application grid. **Quit Daymark** in Settings stops it.

The same build includes both the Wayland and X11 window backends, so it also
runs in a GNOME on Xorg session. `./install.sh --x11` is still accepted for
compatibility but is no longer required.

### Remove

```bash
./install.sh --uninstall          # keeps recorded history
./install.sh --uninstall --purge  # deletes local history too
```

## Use

- Click a day in the weekly chart, or use `←` / `→` to move one day.
- `T` returns to today, `Ctrl+1` / `Ctrl+2` switch between Day and History.
- `Ctrl+,` opens Settings; `Ctrl+W` hides the window; `Ctrl+Q` quits completely.
- Daymark uses a custom frameless title bar. Drag the empty header or brand area to move
  the window, drag the window edges to resize, and use the minimize, maximize
  and close controls at the right. Close hides to the background; **Quit
  Daymark** is in Settings.
- History lists the last 14 days; click any row to inspect that day.
- Search filters the application list by name.
- Settings holds the idle timeout, history retention, appearance, autostart,
  pause/resume, an "open data folder" shortcut, and quit/delete actions.

## How tracking works

A GNOME Shell companion (`extension/`, UUID `daymark@daymark.app`) watches the
focused application with Mutter's `focus_window` and `Shell.WindowTracker`. It
publishes only this to `$XDG_RUNTIME_DIR/daymark/state.json`:

- application identity (desktop id or WM class) and display name
- tethered idle/locked/away state and the precise moment input stopped
- a heartbeat

It never reads window titles, keystrokes, clipboard data, screenshots or
application content. There is no networking code.

GNOME Shell's own surfaces are filtered out: the `gjs` host processes
(notifications, screen saver), the desktop-icons window, docks and other
non-application window types are never recorded as apps. A one-time migration on
first launch also merges old fallback rows into their real desktop entries and
removes shell-internal rows.

The Rust tracker reads that small file every two seconds. It records
non-overlapping intervals in SQLite, closes them on focus change, idle, lock,
sleep or pause, and writes a heartbeat every 30 seconds. Idle time is
retroactively trimmed to the moment the extension says input stopped. Sleep and
wall-clock jumps do not accrue because durations are accumulated with a
monotonic clock. Sessions that cross midnight are split visually by day; the
tracker also starts a fresh row at midnight. Closed intervals are aggregated
with overlap clamping, so a day's totals are always computed from stored data.

On startup any open interval left by a crash is closed at its last heartbeat,
so hours spent with the machine off or the app dead are never credited. A
partial unique index on `sessions(is_open)` makes two simultaneous open
intervals impossible.

Idle detection uses Mutter's core idle monitor with configurable watchers
(1–15 minutes in the UI, default 5). Lock/presence uses GNOME's screen shield
signals; suspend uses systemd-logind's prepare-for-sleep signal; idle inhibitors
(a video, a presentation) are respected. The tracker also independently closes
the current interval on a wall-clock/monotonic discontinuity, so a missed
suspend signal cannot turn sleep into usage.

### Why an extension?

On GNOME Wayland, arbitrary clients cannot ask which window has focus. X11
tools such as `_NET_ACTIVE_WINDOW` see only XWayland clients, not native
Wayland windows. GNOME's own focus API is Shell-internal. A small Shell
extension is therefore the supported, stable mechanism; it is deliberately
limited to the identity/state above. The app honestly reports when the
companion is missing or has not yet been activated and walks through the
one-time login step.

## Data and privacy

- Database: `~/.local/share/daymark/daymark.db` (mode 0700 directory).
- Runtime state: `$XDG_RUNTIME_DIR/daymark/state.json`, mode 0600.
- Settings: stored in the same SQLite database.
- Autostart: `~/.config/autostart/daymark.desktop` (user-level only).
- Retention: 90 days, 1 year (default), or forever. Pruning deletes old
  sessions and orphaned application rows.
- No analytics, telemetry, network access, titles, keystrokes, screenshots, or
  clipboard contents.

A fresh database is 4 KB; the WAL is typically about 40 KB while Daymark runs
and is checkpointed normally.

## Implementation notes

- Rust 2024, Slint 1.8 with the **software renderer** (no WebView, no Node, no
  browser engine, no GPU requirement) and SQLite compiled into the binary
  through `rusqlite`/`libsqlite3-sys`. The invisible background process is three
  threads (main, tracker, IPC); showing the window adds the Slint and
  accessibility threads.
- The system's Ubuntu Sans and installed application icons are used. Both PNG
  and SVG icons are supported; raster icons are downscaled once to the size the
  tile needs instead of keeping multi-megabyte originals in memory.
- Slint 1.8 is pinned on purpose. Slint 1.18's Parley/ICU text stack plus its
  image/SVG dependencies made even a minimal build about 10 MB, and the 1.8
  backport in `vendor/` is what makes the frameless title bar draggable on
  Wayland.
- Slint 1.8 does not expose the native window-move hook used by later versions'
  `WindowMoveArea`. A tiny local backport in `vendor/` adds
  `Window::start_window_move()` and wires it to winit's `drag_window()`. It also
  sets the Wayland `app_id`/X11 `WM_CLASS` to `daymark` (so GNOME matches the
  window to the desktop entry) and provides `hide_without_quitting()`, which is
  what lets “close” keep tracking instead of terminating the process.
- Accessibility (AccessKit) is enabled, so the interface exposes roles, names
  and labels to assistive technology.
- The search icon is a small embedded raster because the software renderer does
  not implement `Path` drawing (it is used only as a tinted 16px asset).
- SQLite is compiled in, so the build does not need a separate
  `libsqlite3-dev` package and Daymark always runs against the SQLite version it
  was built and tested with.
- One local Unix socket plus `flock` provides single-instance behavior; a second
  launch shows the existing window instead of starting another tracker. In
  `--background` mode no window or event loop exists at all until it is asked
  for.

## Measured results

Measured on Ubuntu 26.04, GNOME Shell 50.1, Wayland, x86_64, release build:

| Item | Result |
| --- | ---: |
| Stripped executable | **8,828,912 bytes (8.42 MiB)** |
| Installed application files (binary, icon, entries, extension) | ≈ 8.85 MB |
| Fresh database | 4 KB |
| Hidden RSS | ≈ 8.2 MB |
| Hidden CPU, 20 s sample | 0.0% (0 ticks at 100 Hz) |
| Hidden wakeups | 0/s (blocked on IPC) |
| Visible RSS | ≈ 27 MB |
| Visible CPU, 10 s sample | 0.0% |
| Visible wakeups | ≈ 1.4/s |
| Threads | 3 hidden / 7 visible |

CPU is from `/proc/<pid>/stat` (`utime+stime` ticks / 100 Hz / interval); RSS
from `/proc/<pid>/status`; samples were taken after a 4 second settle. Wakeups
are voluntary context switches per second. Dynamic dependencies:
`libc`, `libm`, `libgcc_s`; the GNOME/Wayland libraries are already part of the
desktop session. SQLite is compiled in. The binary sits below the intended
10 MB budget; that budget paid for SVG icons, AccessKit accessibility and the
X11 backend.

## Verification

- `cargo test --lib`: **38 passed** — date/DST arithmetic and midnight bounds,
  interval overlap and hourly aggregation, idle-gap exclusion, per-app totals,
  averages, JSON parsing (including the seconds/milliseconds compatibility
  path), desktop-entry parsing (quoted `Exec`, `NoDisplay`, hidden entries),
  shell-window filtering, SQLite persistence, legacy-row migration, corruption
  recovery, crash recovery, at-most-one-open-session, pruning, settings, single
  instance, and tracker focus/pause/idle behavior.
- The release binary was run on the live GNOME 50.1 Wayland session. The window
  now reaches GNOME with `app_id`/WM class `daymark`: the companion reports the
  focused window as `daymark.desktop` with the Daymark icon and name instead of
  an “Unknown” window-backed app.
- `--background` was verified to create no window and no Slint event loop, with
  ≈ 8.2 MB RSS, 0% CPU and 0 wakeups/s over a 20 second sample; `--quit` stops
  it cleanly and a second launch shows the existing (headless) instance.
- The one-time migration ran on the live database (with a backup copy taken
  first): the `gjs` row and its sessions were removed and the
  `wm_class:com.t3tools.t3code` fallback row was merged into
  `com.t3tools.T3Code.desktop`, keeping its icon and session duration.
- Interactive drag/resize, close-to-background and maximize round-trips were
  verified by code path and single-instance tests; drag/resize itself was not
  exercised by an automated pointer on Wayland.
- The companion was loaded by a headless GNOME Shell 50.1 session: it created
  the state file, advanced its heartbeat, honored a 30-second idle threshold,
  reported `away`, and shut down cleanly. Active-window focus transitions could
  not be exercised in that headless seat (no focused window exists), so the
  final focus switch awaits the user's first real login with the new extension.
- Visual states (light, dark, populated, empty, first run, setup, history,
  settings, error, and focused search) were rendered through the application's
  own software renderer at 1120×780 and 900×620 and inspected. Snapshots are
  now written opaque, so the images in `docs/` display correctly in browsers
  and Markdown viewers.

## Known limitations

- **Wayland focus tracking needs the GNOME companion and one logout/login after
  installation.** This is a GNOME extension-loading rule, not a Daymark
  workaround; the app says so in its setup flow. Locked/suspended periods are
  intentionally not counted. Updating an already-loaded extension also needs a
  re-login before GNOME executes the new file.
- GNOME Shell surfaces are filtered, but a future Shell version could introduce
  a new internal surface type; the app also keeps a small deny-list as a second
  line of defence.
- Lock and suspend handling is implemented against GNOME's screen shield and
  logind signals; only the idle path was exercised in the headless integration
  test and a live lock/unlock was not possible here.
- Accessibility exposes roles and labels (AccessKit), not a hand-tuned
  screen-reader reading order. Contrast, large targets and full keyboard
  shortcuts are provided.
- If the GNOME companion stops reporting, Daymark stops recording rather than
  guessing; the dashboard explains this and links to setup again.

## Source layout

```
src/main.rs           lifecycle, single instance, event loop
src/app.rs            UI controller and model building
src/tracker.rs        focus state file → intervals
src/db.rs             SQLite schema, recovery, queries
src/stats.rs          pure day/hour/app aggregation
src/desktop.rs        .desktop metadata and icon lookup
src/date.rs           local dates, DST-safe boundaries, formatting
src/json.rs           tiny JSON reader for the companion state
src/platform.rs       autostart, extension install, theme detection
src/instance.rs       flock + Unix socket IPC
src/bin/render.rs     deterministic screenshot renderer (dev tool)
ui/                   Slint dashboard and design tokens
extension/            GNOME Shell companion (GJS)
packaging/            reference desktop entries
docs/                 rendered screenshots
```

To regenerate the screenshots:

```bash
cargo build --bin render
./target/release/render --out docs/daymark-light.png --state light
./target/release/render --out docs/daymark-dark.png --state dark
./target/release/render --out docs/daymark-history.png --state history
./target/release/render --out docs/daymark-settings.png --state light-settings
./target/release/render --out docs/daymark-setup.png --state setup
./target/release/render --out docs/daymark-first-run.png --state empty-banner
```

## License

MIT. See [LICENSE](LICENSE).
