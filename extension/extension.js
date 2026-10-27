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

const STATE_VERSION = 3;
const DEFAULT_IDLE_SECONDS = 300;
const HEARTBEAT_SECONDS = 15;
const IDLE_RECHECK_SECONDS = 60;
// org.gnome.SessionManager GSM_INHIBITOR_FLAG_IDLE
const IDLE_INHIBIT_FLAG = 8;
// MPRIS players live on the session bus; nothing here touches the network.
const MPRIS_PREFIX = 'org.mpris.MediaPlayer2.';
const MPRIS_PATH = '/org/mpris/MediaPlayer2';
const MPRIS_PLAYER_INTERFACE = 'org.mpris.MediaPlayer2.Player';

export default class DaymarkExtension extends Extension {
    enable() {
        this._dir = GLib.build_filenamev([GLib.get_user_runtime_dir(), 'daymark']);
        GLib.mkdir_with_parents(this._dir, 0o700);
        this._statePath = GLib.build_filenamev([this._dir, 'state.json']);
        this._configPath = GLib.build_filenamev([this._dir, 'config.json']);
        this._notifyPath = GLib.build_filenamev([this._dir, 'notify.json']);
        this._lastNotificationId = null;

        this._enabled = true;
        this._seq = 0;
        this._focus = null;
        this._idle = false;
        this._inhibited = false;
        this._audio = false;
        this._audioProbe = false;
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
                else if (file && file.get_basename() === 'notify.json')
                    this._showNotification();
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
                this._refreshAudio();
                this._writeState();
                return GLib.SOURCE_CONTINUE;
            });

        this._readFocus();
        this._armIdleWatch();
        this._writeState();
        this._refreshAudio();
        this._showNotification();
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

    _showNotification() {
        if (!this._enabled)
            return;
        let payload;
        try {
            const [ok, bytes] = GLib.file_get_contents(this._notifyPath);
            if (!ok)
                return;
            payload = JSON.parse(new TextDecoder().decode(bytes));
        } catch (e) {
            return;
        }
        if (!payload || typeof payload.id !== 'string' || !payload.id)
            return;
        if (payload.id === this._lastNotificationId)
            return;
        this._lastNotificationId = payload.id;
        const title = typeof payload.title === 'string' && payload.title
            ? payload.title : 'Daymark';
        const body = typeof payload.body === 'string' ? payload.body : '';
        try {
            if (typeof Main.notify === 'function')
                Main.notify(title, body);
            else
                console.debug('[Daymark] Main.notify is unavailable');
        } catch (e) {
            console.debug(`[Daymark] could not show the digest: ${e}`);
        }
        try {
            Gio.File.new_for_path(this._notifyPath).delete(null);
        } catch (e) {
            // The app can replace it next week; a stale file is harmless.
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
        // A session inhibitor or a playing MPRIS player means the machine may
        // still be in use even though the core idle monitor saw no input.
        // Neither source is trusted forever: the tracker applies its own
        // two-hour cap from the last input.
        this._probePresence((inhibited, audio) => {
            if (!this._enabled || this._idle)
                return;
            this._inhibited = !!inhibited;
            this._audio = !!audio;
            this._writeState();
            if (this._inhibited || this._audio) {
                this._scheduleIdleRecheck();
                return;
            }
            const idleMs = this._idleMonitor ? this._idleMonitor.get_idletime() : 0;
            if (idleMs < this._idleSeconds * 1000) {
                // The timer was a recheck after an inhibitor or player ended,
                // and the user is active again. Resume the normal watch.
                this._armIdleWatch();
                return;
            }
            this._idle = true;
            this._setAway(true, Math.max(0, Date.now() - idleMs));
            this._armActiveWatch();
            this._writeState();
        });
    }

    _scheduleIdleRecheck() {
        if (this._idleRecheckId)
            GLib.source_remove(this._idleRecheckId);
        this._idleRecheckId = GLib.timeout_add_seconds(
            GLib.PRIORITY_LOW, IDLE_RECHECK_SECONDS, () => {
                this._idleRecheckId = 0;
                this._onIdle();
                return GLib.SOURCE_REMOVE;
            });
    }

    _sessionBus() {
        try {
            return Gio.DBus.session;
        } catch (e) {
            // A session without a bus simply has no inhibitors or players.
            return null;
        }
    }

    _probePresence(callback) {
        let pending = 2;
        let inhibited = false;
        let audio = false;
        const done = () => {
            pending -= 1;
            if (pending === 0)
                callback(inhibited, audio);
        };
        this._queryInhibited(value => {
            inhibited = value;
            done();
        });
        this._queryAudio(value => {
            audio = value;
            done();
        });
    }

    _queryInhibited(callback) {
        const bus = this._sessionBus();
        if (!bus) {
            callback(false);
            return;
        }
        try {
            bus.call(
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
                    callback(!!inhibited);
                });
        } catch (e) {
            callback(false);
        }
    }

    _refreshAudio() {
        this._queryAudio(() => {});
    }

    // Cache the player result. It is refreshed on the heartbeat and on every
    // idle recheck, so a missing bus, a vanished player or a player that does
    // not answer can only make audio look inactive.
    _queryAudio(callback) {
        if (!this._enabled) {
            callback(false);
            return;
        }
        if (this._audioProbe) {
            callback(this._audio);
            return;
        }
        const bus = this._sessionBus();
        if (!bus) {
            this._setAudio(false, callback);
            return;
        }
        this._audioProbe = true;
        try {
            bus.call(
                'org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus', 'ListNames',
                null, null, Gio.DBusCallFlags.NONE, 1000, this._cancellable,
                (connection, result) => {
                    let names = [];
                    try {
                        [names] = connection.call_finish(result).deepUnpack();
                    } catch (e) {
                        names = [];
                    }
                    const players = (names || []).filter(
                        name => typeof name === 'string' &&
                            name.startsWith(MPRIS_PREFIX));
                    if (players.length === 0) {
                        this._audioProbe = false;
                        this._setAudio(false, callback);
                        return;
                    }
                    let remaining = players.length;
                    let playing = false;
                    for (const name of players) {
                        this._checkPlayer(bus, name, status => {
                            if (status === 'Playing')
                                playing = true;
                            remaining -= 1;
                            if (remaining === 0) {
                                this._audioProbe = false;
                                this._setAudio(playing, callback);
                            }
                        });
                    }
                });
        } catch (e) {
            this._audioProbe = false;
            this._setAudio(false, callback);
        }
    }

    _checkPlayer(bus, name, callback) {
        try {
            bus.call(
                name, MPRIS_PATH, 'org.freedesktop.DBus.Properties', 'Get',
                new GLib.Variant('(ss)', [MPRIS_PLAYER_INTERFACE, 'PlaybackStatus']),
                null, Gio.DBusCallFlags.NONE, 1000, this._cancellable,
                (connection, result) => {
                    let status = '';
                    try {
                        const [value] = connection.call_finish(result).deepUnpack();
                        if (typeof value === 'string')
                            status = value;
                        else if (value && value.deepUnpack)
                            status = value.deepUnpack();
                    } catch (e) {
                        status = '';
                    }
                    callback(status);
                });
        } catch (e) {
            callback('');
        }
    }

    _setAudio(value, callback) {
        const audio = !!value;
        const changed = this._audio !== audio;
        this._audio = audio;
        if (changed && this._enabled)
            this._writeState();
        callback(audio);
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
            audio: this._audio,
            idle_ms: this._idleMonitor ? this._idleMonitor.get_idletime() : 0,
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
