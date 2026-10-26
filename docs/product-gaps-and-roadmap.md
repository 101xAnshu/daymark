# Daymark product gaps and roadmap

Status: V1 implementation complete on branch `v1-release`, pending final review. `main` is
untouched at `df28aef` (Daymark 0.1.1), and nothing has been pushed.

Goal: a .deb and .rpm product for Ubuntu and Fedora with GNOME. Ubuntu is the first target.
The product should be reliable, private, native, and easy to install and keep running.

## Current state at review

- All six P0 items are implemented on `v1-release` as separate merge commits: timed pause,
  packaging and the release pipeline, `doctor`/`--status`/logs, the 5 second noise floor,
  the first-run flow, and the corrected footprint claims.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --lib`
  pass; the library has **69 tests**.
- CI passes on Ubuntu 22.04: formatting, clippy, 69 tests, release build, `.deb` with
  lintian, `.rpm`, shellcheck, and a Fedora 42 install smoke test. The tag-driven release
  workflow has not run yet because no release tag exists.
- The supported runtime floors are Ubuntu 24.04 (GNOME 46+) and Fedora 42 (GNOME 48+).
  The `.deb` is built on Ubuntu 22.04 for the glibc 2.35 floor; 22.04 itself ships GNOME 42
  and is not a supported runtime.
- A 60 second headless run measures about 8 MB RSS, 0.00% CPU, and about 0.5 wakeups/s.
  After the window has been opened and closed, RSS is about 24 MB and the tracker still
  polls. The README states both states separately.
- The repository is public. The release uses GitHub Actions, Releases, and package links on
  the free plan.

## V2 progress

- The Rhythm view is implemented on `v2`: a 7×24 heatmap for the last seven days,
  with hour labels, per-day totals, and a legend. The library has **107 tests**.
- The Day view now has a metrics row and a 24-hour timeline on `v2`. The metrics row
  reports the longest uninterrupted stretch, distinct apps, time after midnight
  (00:00–05:00), the change from the same weekday last week, and a live Now cell for
  today. The timeline draws app-tinted sessions clipped to the selected day.
- The optional systemd user unit is implemented on `v2`: packages install it, but do not
  enable it. The decision log is in `docs/v2-decisions.md`.
- The inotify watch is implemented on `v2`: focus changes wake the tracker immediately,
  with a 30-second safety check and a 2-second fallback when no watch can be created.
- Retention rollup is implemented in the store: old sessions fold into daily per-app totals
  before pruning. The history view still needs to read those totals.
- Audio and inhibitor presence is capped on `v2`. The companion reports `idle_ms`, whether
  an idle inhibitor is active, and whether a session-bus MPRIS player is `Playing`. A
  session whose only presence is audio or an inhibitor closes at the last input plus two
  hours; the cap latches until input returns. Schema v4 adds `sessions.presence`
  (`input`, `inhibitor`, `audio` or `mixed`) without changing rollups or app identity, and
  the Day view shows a quiet presence note under the timeline.
- V2 is local only. Nothing on `v2` is pushed until the branch is approved.

## 1. Executive summary

Daymark already does the core job well. It works on stock GNOME 45 to 50, on Wayland and
Xorg. It stores no window titles, browser history, or URLs, and it contains no network
code. It ships as one 8.5 MiB native binary with no WebView and no WebKitGTK or GTK
runtime. The headless process uses about 8 MB, and the background mode creates no window
or event loop until the interface is requested.

At the start of V1, what was missing was release work, diagnostics, capture accuracy, and
more ways to read the stored data.

- There was no .deb, no .rpm, and no CI release, so a user without Rust could not install Daymark.
- There was no `doctor`, no status command, and no log file, so a silent failure was invisible.
- Capture polled every 2 seconds and did not merge short focus changes.
- Pause was a saved boolean with no timer and no auto-resume.
- An idle inhibitor could keep a session alive indefinitely.
- There was no tray, day timeline, weekly heatmap, digest, aliases, or rollup.
- Some performance claims did not match the real process behavior.

The roadmap below made Daymark easy to install, trustworthy in daily use, and a strong
default for Ubuntu and Fedora. All six P0 items are now implemented; the scorecard records
the starting point and the sections after it note what shipped.

V1 is the six P0 items below. It targets Ubuntu 24.04 and newer on amd64 (GNOME 46+) and
Fedora 42 and newer on x86_64 (GNOME 48+), is built on Ubuntu 22.04 for the glibc 2.35
floor, ships through GitHub Releases, and stays title-free. The tray, site names, heatmap,
digest, aliases, and rollup are post-V1 work.

## Commit policy

- One logical change per commit.
- Do not amend or squash work that has already been shared.
- Merge each worktree branch with a merge commit so the increments stay visible.
- Keep each commit buildable where practical.

## 2. Scorecard

The scorecard records the state before V1. The implemented result is in Current state.

| Area | Before V1 | Target | Priority |
|---|---|---|---|
| Ubuntu, Fedora, and GNOME support | Native, GNOME 45 to 50, Wayland and Xorg | Keep | Keep |
| Install path | `git clone` and `install.sh` | A .deb for Ubuntu and an .rpm for Fedora from GitHub Releases, no build step | P0 |
| Package quality | None yet | Small .deb and .rpm with a working first run | P0 |
| Privacy model | No titles, URLs, history, or network | Keep, and keep site data opt-in and domain-only | Keep |
| Footprint | 8.5 MiB binary, about 8 MB headless, about 24 MB open | Measure both hidden states and keep the native build | Keep |
| Capture accuracy | 2 second polling, no noise floor | Event-driven with a 30 second fallback and a 5 second merge | P1 |
| Idle, lock, suspend | Exact input-stop timestamp and GNOME signals | Keep, and add a presence flag and a cap | Keep and P1 |
| Pause | Saved boolean with no timed pause | 15 minutes, 1 hour, or until resumed, with a safe resume | P0 |
| Diagnostics | In-window status only | `doctor`, `--status`, and rotating logs | P0 |
| Data depth | Day, week, hour, apps, history | Timeline, metrics row, heatmap, digest | P1 and P2 |
| Tests | 38 tests, some based on sleeps | Deterministic core and CI | P0 and P1 |
| Docs and benchmarks | Strong README, no design log or bench script | Decision log, release notes, measured numbers | P2 |

## 3. What is missing, in priority order

Effort: S is one day or less, M is two to five days, L is one to three weeks. Risk is
implementation or product risk.

| # | Gap | Why it matters | Effort | Risk | Priority |
|---|---|---|---|---|---|
| 1 | CI-built .deb and .rpm with a tag-driven release | A user without Rust cannot install Daymark today. This is the main product goal. | M | Low | P0 |
| 2 | First-run package experience | Installing files is not enough. Ship the extension, enable it for the user, offer autostart, and explain the one re-login. | M | Low | P0 |
| 3 | Timed pause and a safe pause state | Pausing before something private is the common privacy action. A forgotten pause must not stop tracking for days. | S | Low | P0 |
| 4 | `doctor`, `--status`, and logs | Silent failure is the worst tracker failure. Report heartbeat age, rows today, extension status, database size, and the last error. | M | Low | P0 |
| 5 | 5 second noise floor and contiguous merge | Short focus changes fragment the journal and inflate switch counts. Merge them into the preceding session. | S | Low | P0 |
| 6 | Event-driven capture | Polling credits a switch up to 2 seconds late and wakes the process 30 times a minute. Use inotify with a 30 second fallback. | M | Medium | P1 |
| 7 | Audio and inhibitor cap with a presence flag | Trusting an inhibitor indefinitely lets a stuck video or presentation count all night. Cap it and record what kept the session alive. | M | Medium | P1 |
| 8 | Tray with pause presets | One-click pause before something private, plus a visible recording state. Ubuntu ships the AppIndicator host. | M | Medium | P1 |
| 9 | 24 hour session timeline | Answers when time was used, not only how much. The intervals are already stored. | M | Low | P1 |
| 10 | Metrics row: longest stretch, distinct apps, after-midnight time, week delta, and Now | Uses existing data and closes the gap between how much and when. | S or M | Low | P1 |
| 11 | App aliases, ignore, share percentage, and icon backfill | Users need to fix bad desktop names, mute helper windows, and see share. | M | Low | P1 |
| 12 | Retention rollup instead of delete | Daily totals should survive raw-session expiry, so last year does not become unanswerable. | M | Medium | P2 |
| 13 | Per-row timezone offset | Travel must not move past sessions into another day. | S or M | Low | P2 |
| 14 | 7 by 24 weekly heatmap | A denser week view for late-night and early-morning patterns. | M | Low | P2 |
| 15 | Weekly digest notification | Monday total, top five apps, and the week-over-week delta. This is engagement work, not correctness. | M | Low | P3 |
| 16 | APT-aware update notice | Ubuntu-native updates with no HTTP client and no new privacy caveat. | S | Low | P3 |
| 17 | Reproducible benchmarks | Publish measured numbers instead of manual README samples. | S | Low | P2 |
| 18 | Design notes, decision log, and release notes | Make the product easy to understand and keep decisions honest. | S | Low | P2 |
| 19 | Collector and GUI split | Crash isolation for the collector. Optional. A systemd restart unit gives most of the benefit at low cost. | L | High | P3 |
| 20 | Opt-in, domain-only site names | Time on github.com is useful, but only if the privacy model stays intact. | L | High | V2 and V3 |

## 4. What needs to be more reliable

### Capture accuracy

Daymark still polls every 2 seconds with a `park_timeout` loop, adds
`mono_delta.clamp(0, 5)`, and discards sessions under 1 second. V1 added the 5 second noise
floor and the contiguous merge: a short focus is folded into the preceding session only
when the two are contiguous, and never across an idle gap. Daymark can still credit a
switch to the app just left for up to 2 seconds, and rapid Alt-Tabs can still be missed.

The remaining P1 target is an inotify watch on the runtime directory with a 30 second
fallback poll.

### Pause behavior

Before V1, pause was one boolean in settings. It had no timer, it survived a reboot, and in
`--background` the paused banner was never seen. A forgotten pause could stop tracking for
days.

V1 added 15 minute, 1 hour, and until-resumed presets. A timed pause ends on its own, even
while the window is hidden, and an open-ended pause resumes at the next app start so it
never silently outlives a restart. The window shows the remaining pause time. A tray
indicator remains post-V1.

### Diagnostics

Before V1, status lived in the window, and errors went to `eprintln!`. There was no log file
and no command-line status.

V1 added `daymark doctor` and `--status`. They report the database path, schema version,
heartbeat age, open session, rows today, state-file age, extension installed and enabled
state, extension version, autostart state, retention horizon, database and WAL size, and the
last error. The installed/version fields follow the copy Daymark actually enables. Logs
rotate daily in `$XDG_STATE_HOME/daymark/logs` and hold counts, timings, and errors only.

### Packaging and release

Before V1, `install.sh` was the only path. There was no .deb, no .rpm, no CI, and no release.

V1 added a .deb built on Ubuntu 22.04 for the glibc floor, an .rpm for Fedora 42 and newer
on x86_64, a tarball, and `SHA256SUMS`. The supported runtime floors are Ubuntu 24.04
(GNOME 46+) and Fedora 42 (GNOME 48+); Ubuntu 22.04 is only the build floor. A tag-driven
workflow creates a draft release and runs `shellcheck`, `cargo fmt --check`,
`cargo clippy -D warnings`, and `cargo test`. CI passes; the release workflow still needs a
release tag.

### App identity and icons

Today, the Shell app id is strong on GNOME, but a window-backed Electron or web app can
still split. Daymark learns an icon only when the app is next focused.

The target is a one-pass icon backfill at startup, a web-app host fallback, alias and
ignore fields, and a share percentage in the app list.

### Data depth

Today, Daymark has a calm day view with a metrics row, a 24-hour session timeline,
7 day bars, a by-hour chart, an app list, a 14 day history, and a 7×24 rhythm view.
A quiet note under the timeline reports time with audio or an inhibitor when present,
without adding another metric cell at the 900 px minimum width.

The heatmap shipped first; the metrics row and day timeline follow it. Keep the calm
visual language. Do not turn the timeline into hundreds of tiny segments.

### Retention and time

Today, retention deletes rows, and day queries use today's local timezone.

The target is a daily rollup per app and per domain if domains are added, with a "totals
only before this date" state. Add `tz_offset_min` to sessions and use it in day queries.

### Tests and measurement

The tree now has 107 tests covering date handling, storage, recovery, migration, pruning,
daily rollup preservation, settings, single instance, tracker behavior, the inotify watch
and fallback, the noise floor and merge, timed pause, log rotation, extension selection,
the first-run decision, autostart consent, seven-day hourly heatmap aggregation, day
metrics and timeline segmentation across midnight and idle gaps, idle_ms parsing, the
two-hour presence boundary, input return, lock and idle precedence, presence persistence
and the v3 to v4 migration, and read-only doctor/status output. Some tracker tests still
use real sleeps and wall time.

The remaining target is a `tick` function that takes `now_wall`, `now_mono`, and state, with
tests that replay event sequences. Cover switch, blip, merge, idle backdate, suspend,
clock jump, pause, midnight, stale, and recovery. `scripts/measure.sh` now exists and the
README publishes headless and hidden-after-close numbers.

## 5. What to keep

- Stock Ubuntu and Fedora GNOME. Support GNOME 45 to 50 on Wayland and Xorg without a
  compositor-specific capture path. Supported distro floors are Ubuntu 24.04 (GNOME 46)
  and Fedora 42 (GNOME 48).
- The privacy model. No window titles, no browser files, no URLs, and no network code.
- The native UI. Slint with no WebView, no WebKitGTK or GTK runtime, and AccessKit roles.
- The single binary. 8.5 MiB, no Node, no Chromium tree, and a simple package.
- Crash and corruption behavior. The 30 second heartbeat, the one-open-row rule, the
  corrupt database quarantine, and monotonic duration accounting.
- GNOME identity handling. Shell app id, Shell surface filtering, legacy row merge, icon
  theme and SVG handling, and the live-tested migration.
- Idle backdating. The exact input-stop timestamp from Mutter, not a sampled guess.

## 6. Out of scope

These are product decisions, not backlog items.

| Do not build | Why |
|---|---|
| Stored window titles | Direct conflict with the privacy promise. |
| Default reads of every browser profile's history | High-sensitivity data. Only opt-in, domain-only, hash-matched, and read-only, if ever. |
| Default reads of browser favicon databases | More browser files to read for a cosmetic gain. |
| A daily GitHub update request | Network code and a third-party dependency. APT is the Ubuntu update channel. |
| A WebView or Tauri GUI | Heavy runtime and more code exposed to attacks. Slint is an advantage. |
| Windows or Hyprland capture | Wrong platform. GNOME is the target. |
| A product with no delete action | Confirm-delete is fine and expected. |
| A systemd-only lifecycle | Offer it as an option. XDG autostart is the GNOME-native default. |

Domains are a special case. They are useful, but only as an opt-in V2 or V3 layer. Store
the domain only, hash transient titles, read history read-only, and exclude private
windows by design. Stage 1 can be a small title-only site map with no history read.

## 7. Ordered roadmap

### P0: before the V1 release (implemented)

1. Add the .deb, .rpm, and CI release pipeline. Build the .deb on Ubuntu 22.04 for amd64
   and the .rpm for Fedora 42 and newer on x86_64. The runtime floors are Ubuntu 24.04
   (GNOME 46+) and Fedora 42 (GNOME 48+). Produce a tarball fallback, `SHA256SUMS`, and a
   draft release. Publish through GitHub Releases only. Add an APT repository later.
   **Implemented; CI passes.**
2. Build the first-run setup. Ship the extension system-wide, enable it for the user,
   offer autostart, and explain the one re-login. Let `postinst` refresh desktop and icon
   caches only. It must not start a tracker as root. **Implemented.**
3. Fix pause. Add 15 minutes, 1 hour, and until resumed, with auto-resume at the next app
   start and a visible paused indicator. **Implemented.**
4. Add `doctor`, `--status`, and logs. **Implemented.**
5. Make the claims about hidden behavior accurate. The tracker wakes every 2 seconds and
   the 700 ms UI timer kept running after hide. Measure headless and hidden-after-close,
   then fix the timer or the text. **Implemented: the timer stops while hidden and the
   README separates the two states.**
6. Add the 5 second noise floor and contiguous merge. **Implemented.**

### P1: V1.1 reliability

7. Add an inotify state watch with a 30 second fallback. **Implemented on `v2`.**
8. Cap audio and inhibitor-only presence at 2 hours from the last input. Add a presence
   flag that records whether time came from input or audio playback, and show it in the UI.
   **Implemented on `v2`: schema v4 stores input, inhibitor, audio or mixed, the cap
   latches until input returns, and the Day view shows a quiet note.**
9. Add the day timeline and metrics row. **Implemented on `v2`.**
10. Add aliases, ignore, share percentage, and icon backfill.
11. Add retention rollup and per-row timezone offset. **Retention rollup store layer
    implemented on `v2`; the history view and timezone offset remain.**
12. Add release notes, a changelog, and the bench script.

### P2: V2 insights

13. Add a tray with open, pause 15 minutes, pause 1 hour, pause until resumed, and quit.
    The tray icon should show the paused state, and it should reappear when the bar restarts.
14. Add the 7 by 24 heatmap. **Implemented on `v2` as the Rhythm view.**
15. Add the weekly digest notification.
16. Add the APT-aware update notice.
17. Write the design token and decision notes, and review animation and transitions.
18. Add an optional systemd user unit with `Restart=on-failure`. **Implemented on `v2`.**
19. Build arm64 .deb and .rpm packages.

### P3 and V3

20. Add opt-in, privacy-preserving domain naming with title hashes and read-only history,
    or a browser companion that reports the active domain.
21. Add a weekly `VACUUM INTO` backup and keep two copies.
22. Add a signed APT repository.
23. Split the collector and GUI only if domains or the tray make the collector too large.
24. Add AI agent and background activity as a separate layer. Never add it to foreground
    time.

## 8. V1 release checklist

Checked items are verified locally and in CI. Unchecked items need a live packaged-install
test or the tag-driven release run.

- [x] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`
      pass.
- [x] The packages target Ubuntu 24.04 and newer on amd64 (GNOME 46+) and Fedora 42 and
      newer on x86_64 (GNOME 48+), built on Ubuntu 22.04 for the glibc 2.35 floor.
- [x] V1 uses GitHub Releases only. No APT repository is required.
- [ ] The .deb builds on Ubuntu 22.04 and installs on 24.04 and 26.04.
- [x] The .rpm builds and installs on Fedora 42 or newer on x86_64.
- [ ] `dpkg -i` starts no root daemon. The first run enables the extension and autostart.
- [ ] The extension installs and updates from `/usr/share/gnome-shell/extensions`.
- [x] Timed pause and resume work. A reboot with pause active behaves as designed.
- [x] `daymark doctor` reports healthy and unhealthy states correctly.
- [x] Measure and document headless and hidden-after-close memory and wakeups.
- [x] Uninstall keeps data. `--purge` removes it, and the README says which.
- [ ] Release notes explain the one re-login for the extension.

## 9. Claims corrected in V1

1. "0 wakeups/s" was wrong. The tracker polls every 2 seconds and the hidden-after-close
   process still polls. The README now says about 0.5 wakeups/s headless, about 2.5/s
   hidden after close, and about 3.9/s with the window open. It never claims zero.
2. "8 MB hidden" is true for `--background` before the window is ever opened. After the
   window is opened and closed, the interface stays in memory: about 24 MB, with the
   700 ms timer stopped. The README states both states separately.
3. "One-time sign-out" is true for the first install and for companion updates on Wayland.
   The README says both, and notes that disabling and re-enabling the extension or
   restarting Daymark does not reload it.
4. "No network" must stay true. Do not add a GitHub update check. Use APT metadata.
5. "Never reads titles" must stay true. If domains ship, change the sentence precisely and
   make the feature opt-in. Do not store titles by default.

## 10. Decisions locked

These choices are fixed for V1.

1. The GitHub repository is public.
2. V1 includes all six P0 items.
3. The .deb targets Ubuntu 24.04 and newer on amd64 (GNOME 46+), built on Ubuntu 22.04 for
   the glibc 2.35 floor. The .rpm targets Fedora 42 and newer on x86_64 (GNOME 48+).
4. V1 ships through GitHub Releases only. The APT repository comes later.
5. V1 stays title-free. Site names are not in V1.
6. The tray ships in V2.
7. Pause offers 15 minutes, 1 hour, and until resumed. A timed pause ends by itself; an
   open-ended pause resumes at the next app start, which is the next login when autostart
   is on.
8. Implementation runs on `v1-release`. `main` stays untouched.

## 11. Evidence notes

- `src/tracker.rs` holds the 2 second park, the 30 second heartbeat, the 1 second discard,
  the 5 second delta clamp, the midnight split, and the gap detection.
- `src/db.rs` holds the schema, recovery, migration, and prune logic.
- `src/settings.rs` stores pause, idle, retention, and theme settings.
- `src/main.rs` starts the 700 ms UI timer, stops it while the window is hidden, and runs
  the `--background` loop.
- `src/platform.rs` handles autostart and extension install, preferring the packaged
  system-wide copy.
- `src/instance.rs` holds the show and quit socket.
- `extension/extension.js` reports focus, idle, shield, and sleep state and queries idle
  inhibitors.
- `ui/app.slint` holds the day metrics row, 24-hour timeline, week, hourly, history,
  search, settings, first-run, and banner views.
- The current tree has 93 tests, CI, .deb and .rpm packaging, `doctor`, timed pause, the
  first-run flow, the noise floor, the Rhythm heatmap, the inotify watch, and the day
  metrics and timeline. It still has no tray.
- This document is the plan and the review record. V1 is merged on `main`; V2 runs on
  `v2`.
