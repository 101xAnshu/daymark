# Changelog

## 0.2.0 — 2026-10-27

### Added

- Optional systemd user unit.
- Event-driven inotify watch with a 30-second safety check and a two-second fallback.
- Two-hour cap for audio/inhibitor-only presence, with a stored presence flag.
- Day timeline and a metrics row: longest stretch, apps, after midnight, week delta and Now.
- App aliases, ignore rules, share percentages and a startup icon backfill.
- Retention rollups in the Day and History views, with a quiet **Totals only** state.
- StatusNotifierItem tray with Open, pause presets, Resume and Quit.
- Opt-in weekly digest with one local notification and a Settings toggle.
- Local APT-aware update notice.
- Opt-in, domain-only site names read from local browser history.
- arm64 `.deb` and aarch64 `.rpm` packages, cross-built with QEMU install smoke tests.

### Changed

- The companion reports idle milliseconds and MPRIS playback.
- The database moves to schema v6: session presence, app preferences and domain totals.
- The release binary is about 10.25 MiB; headless RSS is about 6.7–8.2 MiB.

### Upgrade notes

- The companion changed. On Wayland, sign out and back in once after upgrading so GNOME loads it.
- Site names is off by default. Enabling it reads local browser history read-only and stores only domains and approximate daily seconds. Turning it off deletes the stored domains.
- The tray needs a StatusNotifier/AppIndicator host. Ubuntu ships one; Fedora may need `gnome-shell-extension-appindicator`.
- There is no APT repository yet, so the update notice only reports a candidate when one is available locally.

## 0.1.1 — 2026-10-26

- First packaged release: `.deb`, `.rpm`, tarball, checksums, first-run setup, timed pause, `doctor`, `--status`, rotating logs, the 5-second noise floor and measured claims.
