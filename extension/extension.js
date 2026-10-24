// Daymark — a very small GNOME Shell companion that reports the focused
// application, idle state and lock state to the Daymark desktop app.
//
// It deliberately never reads window titles, keystrokes, clipboard contents or
// any data inside applications. The only output is a small JSON state file in
// $XDG_RUNTIME_DIR/daymark, readable only by the current user.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as LoginManager from 'resource:///org/gnome/shell/misc/loginManager.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

const STATE_VERSION = 2;
const DEFAULT_IDLE_SECONDS = 300;
const HEARTBEAT_SECONDS = 15;
const IDLE_RECHECK_SECONDS = 60;
// org.gnome.SessionManager GSM_INHIBITOR_FLAG_IDLE
const IDLE_INHIBIT_FLAG = 8;

export default class DaymarkExtension extends Extension {
    enable() {
        this._dir = GLib.build_filenamev([GLib.get_user_runtime_dir(), 'daymark']);
        GLib.mkdir_with_parents(this._dir, 0o700);
        this._statePath = GLib.build_filenamev([this._dir, 'state.json']);
        this._configPath = GLib.build_filenamev([this._dir, 'config.json']);

        this._enabled = true;
        this._seq = 0;
        this._focus = null;
        this._idle = false;
        this._inhibited = false;
        this._awaySince = 0;
        this._idleSeconds = DEFAULT_IDLE_SECONDS;
        this._idleWatchId = 0;
        this._activeWatchId = 0;
        this._idleRecheckId = 0;
        this._cancellable = new Gio.Cancellable();

        this._loadConfig();

        // A directory monitor works even before the app has written config.json.
        try {
            this._configMonitor = Gio.File.new_for_path(this._dir).monitor_directory(
                Gio.FileMonitorFlags.WATCH_MOVES, null);
            this._configMonitorId = this._configMonitor.connect('changed', (_monitor, file) => {
                if (file && file.get_basename() === 'config.json')
                    this._loadConfig();
            });
        } catch (e) {
            this._configMonitor = null;
            this._configMonitorId = 0;
        }

        this._focusId = global.display.connect('notify::focus-window', () => {
            this._readFocus();
            this._writeState();
        });

        this._shield = Main.screenShield ?? null;
        if (this._shield) {
            this._shieldActiveId = this._shield.connect('active-changed', () => {
                this._readFocus();
                this._recomputeAway();
                this._writeState();
            });
            this._shieldLockedId = this._shield.connect('locked-changed', () => {
                this._readFocus();
                this._recomputeAway();
                this._writeState();
            });
        } else {
            this._shieldActiveId = 0;
            this._shieldLockedId = 0;
        }

        this._loginManager = LoginManager.getLoginManager();
        this._sleepId = this._loginManager.connect('prepare-for-sleep', (_manager, aboutToSuspend) => {
            if (aboutToSuspend) {
                this._setAway(true, Date.now());
            } else {
                this._readFocus();
                this._recomputeAway();
            }
            this._writeState();
        });

        this._heartbeatId = GLib.timeout_add_seconds(
            GLib.PRIORITY_LOW, HEARTBEAT_SECONDS,
            () => {
                this._writeState();
                return GLib.SOURCE_CONTINUE;
            });

        this._readFocus();
        this._armIdleWatch();
        this._writeState();
    }

    disable() {
        this._enabled = false;
        if (this._focusId) {
            global.display.disconnect(this._focusId);
            this._focusId = 0;
        }
        if (this._shield) {
            if (this._shieldActiveId)
                this._shield.disconnect(this._shieldActiveId);
            if (this._shieldLockedId)
                this._shield.disconnect(this._shieldLockedId);
        }
        this._shieldActiveId = 0;
        this._shieldLockedId = 0;
        this._shield = null;
        if (this._sleepId) {
            this._loginManager.disconnect(this._sleepId);
            this._sleepId = 0;
        }
        if (this._heartbeatId) {
            GLib.source_remove(this._heartbeatId);
            this._heartbeatId = 0;
        }
        this._clearWatches();
        if (this._configMonitorId) {
            this._configMonitor.disconnect(this._configMonitorId);
            this._configMonitorId = 0;
        }
        this._configMonitor = null;
        this._cancellable?.cancel();
        this._cancellable = null;
        try {
            Gio.File.new_for_path(this._statePath).delete(null);
        } catch (e) {
            // The file may already be gone; that is the desired end state.
        }
    }

    _loadConfig() {
        let seconds = DEFAULT_IDLE_SECONDS;
        try {
            const [ok, bytes] = GLib.file_get_contents(this._configPath);
            if (ok) {
                const parsed = JSON.parse(new TextDecoder().decode(bytes));
                if (Number.isFinite(parsed.idle_seconds))
                    seconds = Math.max(30, Math.min(3600, Math.floor(parsed.idle_seconds)));
            }
        } catch (e) {
            // Keep the default if the file is missing or malformed.
        }
        if (seconds !== this._idleSeconds) {
            this._idleSeconds = seconds;
            this._armIdleWatch();
        }
    }

    _clearWatches() {
        if (this._idleWatchId) {
            this._idleMonitor.remove_watch(this._idleWatchId);
            this._idleWatchId = 0;
        }
        if (this._activeWatchId) {
            this._idleMonitor.remove_watch(this._activeWatchId);
            this._activeWatchId = 0;
        }
        if (this._idleRecheckId) {
            GLib.source_remove(this._idleRecheckId);
            this._idleRecheckId = 0;
        }
    }

    _armIdleWatch() {
        this._idleMonitor = global.backend.get_core_idle_monitor();
        this._clearWatches();
        if (this._idleSeconds <= 0)
            return;
        this._idleWatchId = this._idleMonitor.add_idle_watch(
            this._idleSeconds * 1000,
            () => {
                this._idleWatchId = 0;
                this._onIdle();
            });
    }

    _onIdle() {
        if (!this._enabled || this._idle)
            return;
        // Ask the session manager whether something (a video, a presentation)
        // is inhibiting idle. If so, this is still active use and we check
        // again a minute later.
        Gio.DBus.session.call(
            'org.gnome.SessionManager', '/org/gnome/SessionManager',
            'org.gnome.SessionManager', 'IsInhibited',
            new GLib.Variant('(u)', [IDLE_INHIBIT_FLAG]), null,
            Gio.DBusCallFlags.NONE, 2000, this._cancellable,
            (connection, result) => {
                let inhibited = false;
                try {
                    [inhibited] = connection.call_finish(result).deepUnpack();
                } catch (e) {
                    // No session manager answer: use plain idleness.
                }
                if (!this._enabled)
                    return;
                this._inhibited = !!inhibited;
                if (this._inhibited) {
                    if (this._idleRecheckId)
                        GLib.source_remove(this._idleRecheckId);
                    this._idleRecheckId = GLib.timeout_add_seconds(
                        GLib.PRIORITY_LOW, IDLE_RECHECK_SECONDS, () => {
                            this._idleRecheckId = 0;
                            this._onIdle();
                            return GLib.SOURCE_REMOVE;
                        });
                    return;
                }
                const idleMs = this._idleMonitor.get_idletime();
                if (idleMs < this._idleSeconds * 1000) {
                    // The timer was a recheck after an idle inhibitor ended, and
                    // the user is active again. Resume the normal watch.
                    this._armIdleWatch();
                    return;
                }
                this._idle = true;
                this._setAway(true, Math.max(0, Date.now() - idleMs));
                this._armActiveWatch();
                this._writeState();
            });
    }

    _armActiveWatch() {
        if (this._activeWatchId)
            return;
        this._activeWatchId = this._idleMonitor.add_user_active_watch(() => {
            this._activeWatchId = 0;
            this._idle = false;
            this._inhibited = false;
            this._recomputeAway();
            this._armIdleWatch();
            this._writeState();
        });
    }

    _readFocus() {
        if (!this._enabled)
            return;
        let focus = null;
        try {
            const win = global.display.focus_window;
            if (win && !this._is_shell_surface(win)) {
                const app = Shell.WindowTracker.get_default().get_window_app(win);
                if (app) {
                    let id = app.get_id();
                    if (app.is_window_backed() || !id) {
                        const wmClass = win.get_wm_class() || '';
                        id = wmClass ? `wmclass:${wmClass}` : '';
                    }
                    if (id) {
                        let pid = null;
                        try {
                            pid = win.get_pid();
                        } catch (e) {
                            pid = null;
                        }
                        focus = {
                            id: id,
                            name: app.get_name() || null,
                            wm_class: win.get_wm_class() || null,
                            pid: pid,
                        };
                    }
                }
            }
        } catch (e) {
            focus = null;
        }
        this._focus = focus;
    }

    // Mutter can hand us GNOME Shell's own surfaces: the desktop-icons window,
    // notification bubbles, the screen shield, docks and menus. They are all
    // hosted by gjs or gnome-shell and must never be recorded as applications.
    _is_shell_surface(win) {
        try {
            const type = win.get_window_type?.();
            if (type !== undefined && type !== null) {
                const allowed = [
                    Meta.WindowType.NORMAL,
                    Meta.WindowType.DIALOG,
                    Meta.WindowType.MODAL_DIALOG,
                    Meta.WindowType.UTILITY,
                ];
                if (!allowed.includes(type))
                    return true;
            }
            const wmClass = (win.get_wm_class() || '').toLowerCase();
            if (['gjs', 'gnome-shell', 'gnome-shell-extension', 'ding'].includes(wmClass))
                return true;
        } catch (e) {
            // A transient/override-redirect window can throw here; ignoring it
            // is safer than recording an unknown identity.
        }
        return false;
    }

    _isLocked() {
        if (this._shield)
            return !!(this._shield.locked || this._shield.active);
        return false;
    }

    _recomputeAway() {
        if (this._isLocked()) {
            if (!this._awaySince)
                this._setAway(true, Date.now());
            return;
        }
        if (this._idle) {
            return; // Keep the precise idle start from the idle callback.
        }
        this._setAway(false, 0);
    }

    _setAway(away, since) {
        if (away) {
            if (!this._awaySince)
                this._awaySince = since || Date.now();
        } else {
            this._awaySince = 0;
        }
    }

    _getAway() {
        return this._idle || this._isLocked() || this._awaySince > 0;
    }

    _writeState() {
        if (!this._enabled)
            return;
        // While away due to a lock or suspend, focus may still point at the
        // last window. Keep it for context, but the app will not count it.
        const away = this._getAway();
        this._seq += 1;
        const payload = {
            v: STATE_VERSION,
            seq: this._seq,
            stamp: Date.now(),
            focus: this._focus,
            away: away,
            away_since: away ? Math.floor((this._awaySince || Date.now()) / 1000) : 0,
            idle: this._idle,
            locked: this._isLocked(),
            inhibited: this._inhibited,
            idle_seconds: this._idleSeconds,
        };
        try {
            const bytes = new TextEncoder().encode(JSON.stringify(payload));
            const file = Gio.File.new_for_path(this._statePath);
            file.replace_contents(
                bytes, null, false,
                Gio.FileCreateFlags.REPLACE_DESTINATION, null);
            GLib.chmod(this._statePath, 0o600);
        } catch (e) {
            console.debug(`[Daymark] could not write state: ${e}`);
        }
    }
}
