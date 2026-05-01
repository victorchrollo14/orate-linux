import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

// Animation parameters ported verbatim from macOS OverlayPanel.swift.
const IDLE_W = 28;
const IDLE_H = 10;
const ACTIVE_H = 34;

const BAR_COUNT = 11;
const BAR_WIDTH = 2.5;
const BAR_GAP = 2;
const BAR_MIN_H = 3;
const BAR_MAX_H = ACTIVE_H - 6;
const SMOOTHING = 0.2;
const PHASE_STEP = 0.15;
const LEVEL_BOOST = 0.4;

const DOT_COUNT = 3;
const DOT_SIZE = 4;
const DOT_GAP = 6;
const DOT_OPACITY_LOW = 76;   // 0.3 * 255
const DOT_OPACITY_HIGH = 255;
const DOT_PULSE_MS = 500;

const ERROR_PILL_W = 70;
const ERROR_DISMISS_MS = 3000;

const EDGE_MARGIN = 8;

// Safety net: hard cap on a single recording.
const MAX_RECORDING_MS = 120000;

const ORATE_IFACE = `<node>
  <interface name="com.orate.App1">
    <method name="StartRecording"/>
    <method name="StopRecording"/>
    <method name="Cancel"/>
    <signal name="StateChanged"><arg type="s" name="state"/></signal>
    <signal name="LevelUpdate"><arg type="d" name="level"/></signal>
    <signal name="ErrorOccurred"><arg type="s" name="message"/></signal>
    <signal name="PasteRequested"/>
  </interface>
</node>`;

// Window classes that need Ctrl+Shift+V (terminal emulators).
const TERMINAL_WM_CLASSES = [
    'gnome-terminal',
    'org.gnome.terminal',
    'org.gnome.console',
    'kgx',
    'ptyxis',
    'org.gnome.ptyxis',
    'kitty',
    'alacritty',
    'wezterm',
    'org.wezfurlong.wezterm',
    'foot',
    'footclient',
    'xterm',
    'urxvt',
    'rxvt',
    'tilix',
    'konsole',
    'org.kde.konsole',
];

// Delay between clipboard set and paste keystroke. Gives the Wayland clipboard
// daemon a moment to register the new selection before the focused app reads it.
const PASTE_DELAY_MS = 60;

const Pill = GObject.registerClass(
class Pill extends St.Widget {
    _init() {
        super._init({
            style_class: 'orate-pill',
            reactive: false,
            can_focus: false,
            track_hover: false,
        });
        this._content = null;
        this._bars = [];
        this._dots = [];
        this._smoothedHeights = [];
        this._waveformPhase = 0;
        this.showIdle();
    }

    _clearContent() {
        if (this._content) {
            this._content.destroy();
            this._content = null;
        }
        this._bars = [];
        this._dots = [];
    }

    showIdle() {
        this._clearContent();
        this.set_size(IDLE_W, IDLE_H);
        this.style_class = 'orate-pill';
    }

    showWaveform() {
        this._clearContent();
        const barsWidth = BAR_COUNT * BAR_WIDTH + (BAR_COUNT - 1) * BAR_GAP;
        const pillWidth = barsWidth + 20;
        this.set_size(pillWidth, ACTIVE_H);
        this.style_class = 'orate-pill orate-active';

        this._content = new St.Widget();
        this._content.set_size(pillWidth, ACTIVE_H);
        this.add_child(this._content);

        this._smoothedHeights = [];
        this._waveformPhase = 0;
        const startX = (pillWidth - barsWidth) / 2;
        for (let i = 0; i < BAR_COUNT; i++) {
            const bar = new St.Widget({
                style_class: 'orate-bar',
                width: BAR_WIDTH,
                height: BAR_MIN_H,
            });
            const cx = startX + i * (BAR_WIDTH + BAR_GAP);
            bar.set_position(cx, (ACTIVE_H - BAR_MIN_H) / 2);
            this._content.add_child(bar);
            this._bars.push(bar);
            this._smoothedHeights.push(BAR_MIN_H);
        }
    }

    showLoading() {
        this._clearContent();
        const dotsWidth = DOT_COUNT * DOT_SIZE + (DOT_COUNT - 1) * DOT_GAP;
        const pillWidth = dotsWidth + 28;
        this.set_size(pillWidth, ACTIVE_H);
        this.style_class = 'orate-pill orate-active';

        this._content = new St.Widget();
        this._content.set_size(pillWidth, ACTIVE_H);
        this.add_child(this._content);

        const startX = (pillWidth - dotsWidth) / 2;
        for (let i = 0; i < DOT_COUNT; i++) {
            const dot = new St.Widget({
                style_class: 'orate-dot',
                width: DOT_SIZE,
                height: DOT_SIZE,
            });
            const cx = startX + i * (DOT_SIZE + DOT_GAP);
            dot.set_position(cx, (ACTIVE_H - DOT_SIZE) / 2);
            dot.set_opacity(DOT_OPACITY_LOW);
            this._content.add_child(dot);
            this._dots.push(dot);

            const delayMs = i * 200;
            GLib.timeout_add(GLib.PRIORITY_DEFAULT, delayMs, () => {
                this._pulseDot(dot);
                return GLib.SOURCE_REMOVE;
            });
        }
    }

    _pulseDot(dot) {
        if (!this._dots.includes(dot)) return;
        dot.ease({
            opacity: DOT_OPACITY_HIGH,
            duration: DOT_PULSE_MS,
            mode: Clutter.AnimationMode.EASE_IN_OUT_SINE,
            onComplete: () => {
                if (!this._dots.includes(dot)) return;
                dot.ease({
                    opacity: DOT_OPACITY_LOW,
                    duration: DOT_PULSE_MS,
                    mode: Clutter.AnimationMode.EASE_IN_OUT_SINE,
                    onComplete: () => this._pulseDot(dot),
                });
            },
        });
    }

    showError() {
        this._clearContent();
        this.set_size(ERROR_PILL_W, ACTIVE_H);
        this.style_class = 'orate-pill orate-error';

        const label = new St.Label({
            text: 'Error',
            style_class: 'orate-error-label',
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
        });
        label.set_size(ERROR_PILL_W, ACTIVE_H);
        this._content = label;
        this.add_child(label);
    }

    updateLevel(level) {
        if (!this._bars.length) return;
        const boosted = Math.pow(level, LEVEL_BOOST);
        this._waveformPhase += PHASE_STEP;
        const center = (this._bars.length - 1) / 2;

        for (let i = 0; i < this._bars.length; i++) {
            const distFromCenter = Math.abs(i - center) / center;
            const sine = Math.sin(this._waveformPhase + i * 0.8);
            const variation = 0.7 + 0.3 * sine;
            const shape = 1 - distFromCenter * 0.4;
            const target = Math.min(boosted * variation * shape, 1);
            const targetH = BAR_MIN_H + (BAR_MAX_H - BAR_MIN_H) * target;
            const prev = this._smoothedHeights[i];
            const smoothed = prev + (targetH - prev) * SMOOTHING;
            this._smoothedHeights[i] = smoothed;

            const bar = this._bars[i];
            bar.height = smoothed;
            bar.y = (ACTIVE_H - smoothed) / 2;
        }
    }
});

export default class OrateExtension extends Extension {
    enable() {
        this._state = 'idle';
        this._recording = false;
        this._errorTimeoutId = 0;
        this._maxDurationId = 0;
        this._pasteTimeoutId = 0;

        const seat = Clutter.get_default_backend().get_default_seat();
        this._virtualKeyboard = seat.create_virtual_device(
            Clutter.InputDeviceType.KEYBOARD_DEVICE
        );

        this._pill = new Pill();
        Main.layoutManager.addChrome(this._pill, {
            affectsInputRegion: false,
            trackFullscreen: true,
        });
        this._positionPill();

        this._monitorsChangedId = Main.layoutManager.connect(
            'monitors-changed',
            () => this._positionPill()
        );

        this._settings = this.getSettings();
        Main.wm.addKeybinding(
            'toggle-recording',
            this._settings,
            Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW | Shell.ActionMode.POPUP,
            () => {
                console.log(`orate: toggle keybinding (recording=${this._recording})`);
                this._toggleRecording();
            }
        );

        this._capturedEventId = global.stage.connect(
            'captured-event',
            (_actor, event) => this._onCapturedEvent(event)
        );

        const Proxy = Gio.DBusProxy.makeProxyWrapper(ORATE_IFACE);
        new Proxy(
            Gio.DBus.session,
            'com.orate.App.Service',
            '/com/orate/App',
            (proxy, error) => {
                if (error) {
                    console.error(`orate: failed to create proxy: ${error.message}`);
                    return;
                }
                this._proxy = proxy;
                this._stateChangedSig = proxy.connectSignal(
                    'StateChanged',
                    (_p, _s, [state]) => this._onStateChanged(state)
                );
                this._levelUpdateSig = proxy.connectSignal(
                    'LevelUpdate',
                    (_p, _s, [level]) => this._pill.updateLevel(level)
                );
                this._errorSig = proxy.connectSignal(
                    'ErrorOccurred',
                    (_p, _s, [msg]) => this._onError(msg)
                );
                this._pasteSig = proxy.connectSignal(
                    'PasteRequested',
                    () => this._schedulePaste()
                );
            }
        );
    }

    disable() {
        Main.wm.removeKeybinding('toggle-recording');
        this._settings = null;

        if (this._capturedEventId) {
            global.stage.disconnect(this._capturedEventId);
            this._capturedEventId = 0;
        }
        if (this._monitorsChangedId) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = 0;
        }
        if (this._errorTimeoutId) {
            GLib.source_remove(this._errorTimeoutId);
            this._errorTimeoutId = 0;
        }
        if (this._maxDurationId) {
            GLib.source_remove(this._maxDurationId);
            this._maxDurationId = 0;
        }
        if (this._pasteTimeoutId) {
            GLib.source_remove(this._pasteTimeoutId);
            this._pasteTimeoutId = 0;
        }
        if (this._proxy) {
            if (this._stateChangedSig)
                this._proxy.disconnectSignal(this._stateChangedSig);
            if (this._levelUpdateSig)
                this._proxy.disconnectSignal(this._levelUpdateSig);
            if (this._errorSig)
                this._proxy.disconnectSignal(this._errorSig);
            if (this._pasteSig)
                this._proxy.disconnectSignal(this._pasteSig);
            this._proxy = null;
        }
        this._virtualKeyboard = null;
        if (this._pill) {
            Main.layoutManager.removeChrome(this._pill);
            this._pill.destroy();
            this._pill = null;
        }
    }

    _positionPill() {
        if (!this._pill) return;
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor) return;
        const workArea = Main.layoutManager.getWorkAreaForMonitor(monitor.index);
        const [w, h] = this._pill.get_size();
        const x = workArea.x + Math.floor((workArea.width - w) / 2);
        const y = workArea.y + workArea.height - h - EDGE_MARGIN;
        this._pill.set_position(x, y);
    }

    _onCapturedEvent(event) {
        if (event.type() !== Clutter.EventType.KEY_PRESS)
            return Clutter.EVENT_PROPAGATE;

        if (event.get_key_symbol() === Clutter.KEY_Escape
            && this._state !== 'idle'
            && this._proxy) {
            this._recording = false;
            this._cancelMaxDuration();
            this._proxy.CancelRemote();
            return Clutter.EVENT_STOP;
        }

        return Clutter.EVENT_PROPAGATE;
    }

    _toggleRecording() {
        if (!this._proxy) return;
        const player = global.display.get_sound_player();
        if (this._recording) {
            this._recording = false;
            this._cancelMaxDuration();
            this._proxy.StopRecordingRemote();
            player.play_from_theme('complete', 'Orate recording stopped', null);
        } else {
            this._recording = true;
            this._proxy.StartRecordingRemote();
            this._armMaxDuration();
            player.play_from_theme('screen-capture', 'Orate recording started', null);
        }
    }

    _armMaxDuration() {
        this._cancelMaxDuration();
        this._maxDurationId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            MAX_RECORDING_MS,
            () => {
                this._maxDurationId = 0;
                if (this._recording && this._proxy) {
                    console.log('orate: max-duration timeout, force-stopping');
                    this._recording = false;
                    this._proxy.StopRecordingRemote();
                }
                return GLib.SOURCE_REMOVE;
            }
        );
    }

    _cancelMaxDuration() {
        if (this._maxDurationId) {
            GLib.source_remove(this._maxDurationId);
            this._maxDurationId = 0;
        }
    }

    _schedulePaste() {
        if (this._pasteTimeoutId) GLib.source_remove(this._pasteTimeoutId);
        this._pasteTimeoutId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            PASTE_DELAY_MS,
            () => {
                this._pasteTimeoutId = 0;
                this._sendPaste();
                return GLib.SOURCE_REMOVE;
            }
        );
    }

    _sendPaste() {
        const vk = this._virtualKeyboard;
        if (!vk) return;

        const win = global.display.focus_window;
        if (!win) {
            console.log('orate: no focused window, skipping auto-paste');
            return;
        }
        const wmClass = (win.get_wm_class() || '').toLowerCase();
        const wmInst = (win.get_wm_class_instance() || '').toLowerCase();
        if (wmClass.includes('orate') || wmInst.includes('orate')) {
            console.log('orate: focused window is orate itself, skipping paste');
            return;
        }
        const isTerminal = TERMINAL_WM_CLASSES.some(
            kw => wmClass.includes(kw) || wmInst.includes(kw)
        );
        console.log(`orate: paste into "${wmClass}" (terminal=${isTerminal})`);

        const t = Clutter.get_current_event_time();
        const PRESSED = Clutter.KeyState.PRESSED;
        const RELEASED = Clutter.KeyState.RELEASED;
        try {
            vk.notify_keyval(t, Clutter.KEY_Control_L, PRESSED);
            if (isTerminal)
                vk.notify_keyval(t, Clutter.KEY_Shift_L, PRESSED);
            vk.notify_keyval(t, Clutter.KEY_v, PRESSED);
            vk.notify_keyval(t, Clutter.KEY_v, RELEASED);
            if (isTerminal)
                vk.notify_keyval(t, Clutter.KEY_Shift_L, RELEASED);
            vk.notify_keyval(t, Clutter.KEY_Control_L, RELEASED);
        } catch (e) {
            console.error(`orate: paste keystroke failed: ${e}`);
        }
    }

    _onStateChanged(state) {
        this._state = state;
        if (this._errorTimeoutId) {
            GLib.source_remove(this._errorTimeoutId);
            this._errorTimeoutId = 0;
        }
        switch (state) {
            case 'listening':
                this._pill.showWaveform();
                break;
            case 'transcribing':
                this._pill.showLoading();
                break;
            case 'idle':
                this._pill.showIdle();
                break;
            default:
                return;
        }
        this._positionPill();
    }

    _onError(message) {
        console.error(`orate: ${message}`);
        this._pill.showError();
        this._positionPill();
        if (this._errorTimeoutId)
            GLib.source_remove(this._errorTimeoutId);
        this._errorTimeoutId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            ERROR_DISMISS_MS,
            () => {
                this._errorTimeoutId = 0;
                if (this._state === 'idle') {
                    this._pill.showIdle();
                    this._positionPill();
                }
                return GLib.SOURCE_REMOVE;
            }
        );
    }
}
