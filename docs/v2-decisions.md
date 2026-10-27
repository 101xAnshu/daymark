# Daymark V2 decisions

Status: V2 implementation runs locally on `v2`. Nothing on `v2` is pushed until the
branch is approved. `main` contains the merged V1 release.

## Scope

V2 includes the seven workstreams below, in dependency order:

1. Foundation: optional systemd user unit and this decision log.
2. Reliability: inotify state watch, audio/inhibitor cap, timeline and metrics row,
   aliases/ignore/share, retention rollup.
3. Tray with pause presets.
4. Weekly digest notification.
5. APT-aware update notice.
6. arm64 `.deb` and `.rpm` packages.
7. Opt-in, domain-only site names.

## Decisions

- **No network code.** The weekly digest is a local notification. The APT notice reads
  local package metadata and never makes an HTTP request.
- **The systemd unit is optional.** Packages install `daymark.service` but do not enable
  it. XDG autostart remains the default. Users who enable the unit should turn off the
  Start at login setting so only one launcher starts Daymark.
- **The tray needs a StatusNotifier host.** Ubuntu ships the AppIndicator extension;
  Fedora may need `gnome-shell-extension-appindicator`. The package documents the
  dependency and does not assume the tray is always visible.
- **Audio and inhibitor-only presence is capped.** Time is credited for at most two hours
  after the last real input, and the session records whether input or audio kept it alive.
- **Retention keeps daily per-app totals.** Raw sessions still expire, but the daily
  totals survive so older history remains answerable.
- **Aliases and ignore rules are local settings.** They change display names and filtering,
  never the recorded app identity.
- **Site names ship last and are opt-in.** They are domain-only, never store window titles,
  and exclude private browsing by design. V2 stays title-free until that layer is enabled.
- **arm64 is cross-built.** GitHub's free plan has no native arm64 runners, so the release
  workflow cross-compiles and tests what it can under emulation.
- **The Rhythm view is the first V2 feature.** It is a 7×24 heatmap of the last seven days.

## Verification

Every workstream must pass `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, and `cargo test --lib` before it is merged
into `v2`. Packaging changes must also be built locally before they are pushed.
