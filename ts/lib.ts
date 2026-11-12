enum LogLevel {
    ERROR = 0,
    WARN,
    INFO,
    DEBUG,
    TRACE,
}

let log_pre: HTMLPreElement;
let log_level: LogLevel = LogLevel.ERROR;
let no_log_messages: boolean = true;

function run(level: string) {
    window.onload = () => {
        log_pre = document.getElementById("log") as HTMLPreElement;
        if (log_pre) log_pre.textContent = "";
        if (LogLevel[level] !== undefined) {
            log_level = LogLevel[level];
        }
        window.addEventListener("error", (e: ErrorEvent) => {
            if (e.error) {
                log(LogLevel.ERROR, `${e.filename}:L${e.lineno}:${e.colno}: ${e.message}`);
            }
            return false;
        }, true);
        init();
    };
}

function log(level: LogLevel, msg: string) {
    if (level > log_level) return;
    if (level === LogLevel.TRACE) console.trace(msg);
    else if (level === LogLevel.DEBUG) console.debug(msg);
    else if (level === LogLevel.INFO) console.info(msg);
    else if (level === LogLevel.WARN) console.warn(msg);
    else if (level === LogLevel.ERROR) console.error(msg);

    if (no_log_messages && log_pre) {
        no_log_messages = false;
        let logSec = document.getElementById("log_section");
        if (logSec) logSec.classList.remove("hide");
    }
    if (log_pre) {
        log_pre.textContent += `${LogLevel[level]}: ${msg}\n`;
    }
}

class Rect {
    x: number = 0;
    y: number = 0;
    w: number = 1;
    h: number = 1;
}

class Settings {
    webSocket: WebSocket;
    capturable_select: HTMLSelectElement;
    stylus_only: HTMLInputElement;
    lock_aspect_ratio: HTMLInputElement;
    orientation_select: HTMLSelectElement;
    canvas_bg_select: HTMLSelectElement;
    enable_hud: HTMLInputElement;
    stroke_preview: HTMLInputElement;
    pressure_curve_preset: HTMLSelectElement;
    pressure_sensitivity: HTMLInputElement;
    gamma_out: HTMLOutputElement;
    min_pressure: HTMLInputElement;
    settings_panel: HTMLElement;
    visible: boolean = true;

    constructor(webSocket: WebSocket) {
        this.webSocket = webSocket;
        this.capturable_select = document.getElementById("window") as HTMLSelectElement;
        this.stylus_only = document.getElementById("stylus_only") as HTMLInputElement;
        this.lock_aspect_ratio = document.getElementById("lock_aspect_ratio") as HTMLInputElement;
        this.orientation_select = document.getElementById("orientation") as HTMLSelectElement;
        this.canvas_bg_select = document.getElementById("canvas_bg") as HTMLSelectElement;
        this.enable_hud = document.getElementById("enable_hud") as HTMLInputElement;
        this.stroke_preview = document.getElementById("stroke_preview") as HTMLInputElement;
        this.pressure_curve_preset = document.getElementById("pressure_curve_preset") as HTMLSelectElement;
        this.pressure_sensitivity = document.getElementById("pressure_sensitivity") as HTMLInputElement;
        this.gamma_out = document.getElementById("gamma_out") as HTMLOutputElement;
        this.min_pressure = document.getElementById("min_pressure") as HTMLInputElement;
        this.settings_panel = document.getElementById("settings");

        let btn_toggle = document.getElementById("btn_toggle_sidebar");
        if (btn_toggle) {
            btn_toggle.onclick = () => this.toggle();
        }

        let btn_close = document.getElementById("btn_close_sidebar");
        if (btn_close) {
            btn_close.onclick = () => this.toggle();
        }

        let lefty = document.getElementById("lefty") as HTMLInputElement;
        if (lefty) {
            lefty.onchange = () => {
                document.body.classList.toggle("lefty", lefty.checked);
                this.save_settings();
                if (tabletEngine) tabletEngine.resize();
            };
        }

        let fullscreen = document.getElementById("fullscreen");
        if (fullscreen) {
            fullscreen.onclick = () => {
                if (!document.fullscreenElement) {
                    document.documentElement.requestFullscreen().catch(() => {});
                } else {
                    document.exitFullscreen().catch(() => {});
                }
            };
        }

        let refresh = document.getElementById("refresh");
        if (refresh) {
            refresh.onclick = () => this.webSocket.send('"GetCapturableList"');
        }

        let btn_cycle = document.getElementById("btn_cycle_monitor");
        if (btn_cycle) {
            btn_cycle.onclick = () => {
                let len = this.capturable_select.options.length;
                if (len > 1) {
                    this.capturable_select.selectedIndex = (this.capturable_select.selectedIndex + 1) % len;
                    this.save_and_sync();
                }
            };
        }

        let debug_check = document.getElementById("enable_debug_overlay") as HTMLInputElement;
        if (debug_check) {
            debug_check.onchange = () => {
                let dbg = document.getElementById("debug_overlay");
                if (dbg) dbg.classList.toggle("hide", !debug_check.checked);
                this.save_settings();
            };
        }

        this.pressure_sensitivity.oninput = () => {
            this.gamma_out.value = this.pressure_sensitivity.value;
            this.pressure_curve_preset.value = "custom";
        };

        this.pressure_curve_preset.onchange = () => {
            let val = this.pressure_curve_preset.value;
            if (val === "linear") {
                this.pressure_sensitivity.value = "1.0";
            } else if (val === "soft") {
                this.pressure_sensitivity.value = "1.8";
            } else if (val === "hard") {
                this.pressure_sensitivity.value = "0.55";
            }
            this.gamma_out.value = this.pressure_sensitivity.value;
            this.save_and_sync();
        };

        let auto_sync = () => this.save_and_sync();
        this.capturable_select.onchange = auto_sync;
        this.stylus_only.onchange = auto_sync;
        this.lock_aspect_ratio.onchange = auto_sync;
        this.orientation_select.onchange = auto_sync;
        this.canvas_bg_select.onchange = auto_sync;
        this.enable_hud.onchange = () => {
            let hud = document.getElementById("hud_overlay");
            if (hud) hud.classList.toggle("hide", !this.enable_hud.checked);
            this.save_settings();
        };
        this.stroke_preview.onchange = auto_sync;
        this.pressure_sensitivity.onchange = auto_sync;
        this.min_pressure.onchange = auto_sync;

        this.load_settings();
    }

    toggle() {
        this.visible = !this.visible;
        this.settings_panel.classList.toggle("hide", !this.visible);
        let btn_toggle = document.getElementById("btn_toggle_sidebar");
        if (btn_toggle) {
            btn_toggle.style.display = this.visible ? "none" : "flex";
        }
        if (tabletEngine) {
            tabletEngine.resize();
        }
    }

    save_and_sync() {
        this.updateTargetMonitorAspect();
        this.save_settings();
        this.send_server_config();
        if (tabletEngine) {
            tabletEngine.updateVisuals();
        }
    }

    updateTargetMonitorAspect() {
        let opt = this.capturable_select.options[this.capturable_select.selectedIndex];
        if (opt) {
            let m = opt.text.match(/(\d+)x(\d+)/);
            if (m && tabletEngine) {
                let w = parseInt(m[1], 10);
                let h = parseInt(m[2], 10);
                if (w > 0 && h > 0) {
                    tabletEngine.targetAspectRatio = w / h;
                    tabletEngine.updateVisuals();
                }
            }
            if (pointerHandler && pointerHandler.hud) {
                pointerHandler.hud.setMonitor(opt.text);
            }
        }
    }

    send_server_config() {
        let conf = {
            capturable_id: Number(this.capturable_select.value) || 0,
            tablet_mode: true,
            stylus_only: this.stylus_only.checked,
            lock_aspect_ratio: this.lock_aspect_ratio.checked,
            orientation: Number(this.orientation_select.value) || 0,
            pressure_curve: this.pressure_curve_preset.value,
            pressure_sensitivity: this.pressure_sensitivity.valueAsNumber || 1.0,
            max_width: 1920,
            max_height: 1080,
            frame_rate: 0,
            capture_cursor: false,
        };
        this.webSocket.send(JSON.stringify({ Config: conf }));
    }

    save_settings() {
        let data = {
            capturable_id: this.capturable_select.value,
            stylus_only: this.stylus_only.checked,
            lock_aspect_ratio: this.lock_aspect_ratio.checked,
            orientation: this.orientation_select.value,
            canvas_bg: this.canvas_bg_select.value,
            enable_hud: this.enable_hud.checked,
            stroke_preview: this.stroke_preview.checked,
            pressure_curve_preset: this.pressure_curve_preset.value,
            pressure_sensitivity: this.pressure_sensitivity.value,
            min_pressure: this.min_pressure.value,
            lefty: (document.getElementById("lefty") as HTMLInputElement)?.checked || false,
        };
        localStorage.setItem("weylus_tablet_settings", JSON.stringify(data));
        localStorage.setItem("weylus_saved_capturable_id", this.capturable_select.value);
    }

    load_settings() {
        let s = localStorage.getItem("weylus_tablet_settings");
        if (!s) return;
        try {
            let data = JSON.parse(s);
            if (data.lefty !== undefined) {
                let lefty = document.getElementById("lefty") as HTMLInputElement;
                if (lefty) {
                    lefty.checked = data.lefty;
                    document.body.classList.toggle("lefty", data.lefty);
                }
            }
            if (data.stylus_only !== undefined) this.stylus_only.checked = data.stylus_only;
            if (data.lock_aspect_ratio !== undefined) this.lock_aspect_ratio.checked = data.lock_aspect_ratio;
            if (data.orientation !== undefined) this.orientation_select.value = data.orientation;
            if (data.canvas_bg !== undefined) this.canvas_bg_select.value = data.canvas_bg;
            if (data.enable_hud !== undefined) {
                this.enable_hud.checked = data.enable_hud;
                let hud = document.getElementById("hud_overlay");
                if (hud) hud.classList.toggle("hide", !data.enable_hud);
            }
            if (data.stroke_preview !== undefined) this.stroke_preview.checked = data.stroke_preview;
            if (data.pressure_curve_preset !== undefined) this.pressure_curve_preset.value = data.pressure_curve_preset;
            if (data.pressure_sensitivity !== undefined) {
                this.pressure_sensitivity.value = data.pressure_sensitivity;
                this.gamma_out.value = data.pressure_sensitivity;
            }
            if (data.min_pressure !== undefined) this.min_pressure.value = data.min_pressure;
        } catch (_) {}
    }
}

interface StrokePoint {
    x: number;
    y: number;
    pressure: number;
    time: number;
}

class TabletEngine {
    canvas: HTMLCanvasElement;
    ctx: CanvasRenderingContext2D;
    activeRect: { x: number; y: number; w: number; h: number } = { x: 0, y: 0, w: 0, h: 0 };
    targetAspectRatio: number = 16 / 9;
    strokes: StrokePoint[][] = [];
    currentStrokes: Map<number, StrokePoint[]> = new Map();
    fadeDuration: number = 1500; // ms

    constructor(canvas: HTMLCanvasElement) {
        this.canvas = canvas;
        this.ctx = canvas.getContext("2d");
        this.resize();
        window.addEventListener("resize", () => {
            this.resize();
            this.updateVisuals();
        });
        requestAnimationFrame(() => this.renderLoop());
    }

    resize() {
        let dpr = window.devicePixelRatio || 1;
        let rect = this.canvas.getBoundingClientRect();
        this.canvas.width = Math.round(rect.width * dpr);
        this.canvas.height = Math.round(rect.height * dpr);
        this.calculateActiveArea();
    }

    calculateActiveArea() {
        let cw = this.canvas.width;
        let ch = this.canvas.height;
        if (!settings || !settings.lock_aspect_ratio.checked) {
            this.activeRect = { x: 0, y: 0, w: cw, h: ch };
            return;
        }

        let tabletAR = cw / ch;
        let targetAR = this.targetAspectRatio;
        let aw = cw;
        let ah = ch;
        if (tabletAR > targetAR) {
            aw = Math.round(ch * targetAR);
        } else {
            ah = Math.round(cw / targetAR);
        }
        let ax = Math.round((cw - aw) / 2);
        let ay = Math.round((ch - ah) / 2);
        this.activeRect = { x: ax, y: ay, w: aw, h: ah };
    }

    updateVisuals() {
        this.calculateActiveArea();
    }

    addPoint(pointerId: number, x: number, y: number, pressure: number) {
        if (!settings.stroke_preview.checked) return;
        let dpr = window.devicePixelRatio || 1;
        let rect = this.canvas.getBoundingClientRect();
        let px = (x - rect.left) * dpr;
        let py = (y - rect.top) * dpr;
        let pt: StrokePoint = { x: px, y: py, pressure, time: performance.now() };
        let stroke = this.currentStrokes.get(pointerId);
        if (!stroke) {
            stroke = [];
            this.currentStrokes.set(pointerId, stroke);
        }
        stroke.push(pt);
    }

    endStroke(pointerId: number) {
        let stroke = this.currentStrokes.get(pointerId);
        if (stroke && stroke.length > 0) {
            this.strokes.push(stroke);
        }
        this.currentStrokes.delete(pointerId);
    }

    renderLoop() {
        let now = performance.now();
        let ctx = this.ctx;
        let cw = this.canvas.width;
        let ch = this.canvas.height;

        ctx.clearRect(0, 0, cw, ch);

        // 1. Draw Active Area Boundary if aspect ratio is locked
        let ar = this.activeRect;
        let isLocked = settings && settings.lock_aspect_ratio.checked;
        if (isLocked && (ar.w < cw || ar.h < ch)) {
            // Draw dimmed letterbox / pillarbox outside active area
            ctx.fillStyle = "rgba(0, 0, 0, 0.4)";
            if (ar.x > 0) {
                ctx.fillRect(0, 0, ar.x, ch);
                ctx.fillRect(ar.x + ar.w, 0, cw - (ar.x + ar.w), ch);
            }
            if (ar.y > 0) {
                ctx.fillRect(0, 0, cw, ar.y);
                ctx.fillRect(0, ar.y + ar.h, cw, ch - (ar.y + ar.h));
            }

            // Draw active border
            ctx.strokeStyle = "rgba(59, 130, 246, 0.5)";
            ctx.lineWidth = 2 * (window.devicePixelRatio || 1);
            ctx.strokeRect(ar.x, ar.y, ar.w, ar.h);

            // Draw corner markers
            let cornerLen = 16 * (window.devicePixelRatio || 1);
            ctx.strokeStyle = "rgba(59, 130, 246, 0.9)";
            ctx.lineWidth = 3 * (window.devicePixelRatio || 1);
            ctx.beginPath();
            // Top-left
            ctx.moveTo(ar.x, ar.y + cornerLen);
            ctx.lineTo(ar.x, ar.y);
            ctx.lineTo(ar.x + cornerLen, ar.y);
            // Top-right
            ctx.moveTo(ar.x + ar.w - cornerLen, ar.y);
            ctx.lineTo(ar.x + ar.w, ar.y);
            ctx.lineTo(ar.x + ar.w, ar.y + cornerLen);
            // Bottom-left
            ctx.moveTo(ar.x, ar.y + ar.h - cornerLen);
            ctx.lineTo(ar.x, ar.y + ar.h);
            ctx.lineTo(ar.x + cornerLen, ar.y + ar.h);
            // Bottom-right
            ctx.moveTo(ar.x + ar.w - cornerLen, ar.y + ar.h);
            ctx.lineTo(ar.x + ar.w, ar.y + ar.h);
            ctx.lineTo(ar.x + ar.w, ar.y + ar.h - cornerLen);
            ctx.stroke();
        }

        // 2. Draw Background Pattern inside active area
        let bg = settings ? settings.canvas_bg_select.value : "dots";
        if (bg === "dots" || bg === "grid") {
            let step = 32 * (window.devicePixelRatio || 1);
            ctx.save();
            ctx.beginPath();
            ctx.rect(ar.x, ar.y, ar.w, ar.h);
            ctx.clip();

            if (bg === "dots") {
                ctx.fillStyle = "rgba(160, 160, 160, 0.25)";
                let r = 1.5 * (window.devicePixelRatio || 1);
                for (let x = ar.x + (step / 2); x < ar.x + ar.w; x += step) {
                    for (let y = ar.y + (step / 2); y < ar.y + ar.h; y += step) {
                        ctx.beginPath();
                        ctx.arc(x, y, r, 0, Math.PI * 2);
                        ctx.fill();
                    }
                }
            } else if (bg === "grid") {
                ctx.strokeStyle = "rgba(160, 160, 160, 0.12)";
                ctx.lineWidth = 1;
                ctx.beginPath();
                for (let x = ar.x; x <= ar.x + ar.w; x += step) {
                    ctx.moveTo(x, ar.y);
                    ctx.lineTo(x, ar.y + ar.h);
                }
                for (let y = ar.y; y <= ar.y + ar.h; y += step) {
                    ctx.moveTo(ar.x, y);
                    ctx.lineTo(ar.x + ar.w, y);
                }
                ctx.stroke();
            }
            ctx.restore();
        }

        // 3. Draw Fading Pen Strokes Preview
        if (settings && settings.stroke_preview.checked) {
            // Filter out fully faded strokes
            this.strokes = this.strokes.filter(s => {
                let lastPt = s[s.length - 1];
                return now - lastPt.time < this.fadeDuration;
            });

            let allStrokes = [...this.strokes, ...Array.from(this.currentStrokes.values())];
            ctx.lineCap = "round";
            ctx.lineJoin = "round";

            for (let stroke of allStrokes) {
                if (stroke.length < 2) continue;
                for (let i = 1; i < stroke.length; i++) {
                    let p0 = stroke[i - 1];
                    let p1 = stroke[i];
                    let age = now - p1.time;
                    let alpha = Math.max(0, 1.0 - (age / this.fadeDuration));
                    if (alpha <= 0) continue;

                    let avgPressure = (p0.pressure + p1.pressure) / 2;
                    let width = Math.max(1.5, avgPressure * 8 * (window.devicePixelRatio || 1));

                    ctx.strokeStyle = `rgba(59, 130, 246, ${alpha.toFixed(2)})`;
                    ctx.lineWidth = width;
                    ctx.beginPath();
                    ctx.moveTo(p0.x, p0.y);
                    ctx.lineTo(p1.x, p1.y);
                    ctx.stroke();
                }
            }
        }

        requestAnimationFrame(() => this.renderLoop());
    }
}

class HudOverlay {
    hudMonitor: HTMLElement;
    hudType: HTMLElement;
    hudState: HTMLElement;
    hudPressureVal: HTMLElement;
    hudPressureBar: HTMLElement;
    hudCoord: HTMLElement;

    constructor() {
        this.hudMonitor = document.getElementById("hud_monitor");
        this.hudType = document.getElementById("hud_type");
        this.hudState = document.getElementById("hud_state");
        this.hudPressureVal = document.getElementById("hud_pressure_val");
        this.hudPressureBar = document.getElementById("hud_pressure_bar");
        this.hudCoord = document.getElementById("hud_coord");
    }

    setMonitor(name: string) {
        if (!this.hudMonitor) return;
        let m = name.match(/Display\s+(\d+)/i);
        if (m) {
            this.hudMonitor.textContent = `DISP ${m[1]}`;
        } else if (name.toLowerCase().includes("all")) {
            this.hudMonitor.textContent = "ALL";
        } else {
            this.hudMonitor.textContent = name.slice(0, 6).toUpperCase();
        }
    }

    update(pointerType: string, state: string, pressure: number, normX: number, normY: number) {
        if (!this.hudType) return;
        this.hudType.textContent = pointerType.toUpperCase();
        this.hudState.textContent = state.toUpperCase();
        this.hudState.className = "hud-item";
        if (state === "contact") this.hudState.classList.add("contact");
        else if (state === "hover") this.hudState.classList.add("hover");
        else if (state === "barrel") this.hudState.classList.add("barrel");
        else if (state === "eraser") this.hudState.classList.add("eraser");

        let pct = Math.round(pressure * 100);
        this.hudPressureVal.textContent = `${pct}%`;
        this.hudPressureBar.style.width = `${pct}%`;
        this.hudCoord.textContent = `X: ${normX.toFixed(3)} | Y: ${normY.toFixed(3)}`;
    }
}

function eventTypeToByte(t: string): number {
    switch (t) {
        case "pointerdown": return 1;
        case "pointerup": return 2;
        case "pointermove": return 3;
        case "pointercancel": return 4;
        case "pointerenter": return 5;
        case "pointerleave": return 6;
        case "pointerover": return 7;
        case "pointerout": return 8;
        default: return 3;
    }
}

function pointerTypeToByte(t: string): number {
    switch (t) {
        case "pen": return 1;
        case "touch": return 2;
        case "mouse": return 3;
        default: return 0;
    }
}

class PointerHandler {
    webSocket: WebSocket;
    canvas: HTMLCanvasElement;
    hud: HudOverlay;

    constructor(webSocket: WebSocket) {
        this.webSocket = webSocket;
        this.canvas = document.getElementById("tablet_canvas") as HTMLCanvasElement;
        this.hud = new HudOverlay();

        let handleEvent = (e: PointerEvent, eventType: string) => this.onPointer(e, eventType);

        this.canvas.onpointerdown = (e) => handleEvent(e, "pointerdown");
        this.canvas.onpointerup = (e) => handleEvent(e, "pointerup");
        this.canvas.onpointercancel = (e) => handleEvent(e, "pointercancel");
        this.canvas.onpointermove = (e) => handleEvent(e, "pointermove");
        this.canvas.onpointerenter = (e) => handleEvent(e, "pointerenter");
        this.canvas.onpointerleave = (e) => handleEvent(e, "pointerleave");
        this.canvas.onpointerover = (e) => handleEvent(e, "pointerover");
        this.canvas.onpointerout = (e) => handleEvent(e, "pointerout");

        this.canvas.ontouchmove = (e) => e.preventDefault();
        this.canvas.oncontextmenu = (e) => e.preventDefault();
    }

    onPointer(event: PointerEvent, eventType: string) {
        // Palm rejection / Stylus only mode filter
        if (settings.stylus_only.checked && event.pointerType !== "pen") {
            return;
        }

        let dpr = window.devicePixelRatio || 1;
        let rect = this.canvas.getBoundingClientRect();
        let ar = tabletEngine.activeRect;
        let minP = settings.min_pressure.valueAsNumber || 0;
        let gamma = settings.pressure_sensitivity.valueAsNumber || 1.0;

        // Collect coalesced events if available for pointermove, else single event
        let rawEvents: PointerEvent[] = [];
        if (eventType === "pointermove" && typeof (event as any).getCoalescedEvents === "function") {
            try {
                rawEvents = (event as any).getCoalescedEvents();
            } catch (_) {}
        }
        if (!rawEvents || rawEvents.length === 0) {
            rawEvents = [event];
        }

        let count = rawEvents.length;
        let buffer = new ArrayBuffer(count * 18);
        let view = new DataView(buffer);

        for (let i = 0; i < count; i++) {
            let ev = rawEvents[i];
            let clientX = (ev.clientX - rect.left) * dpr;
            let clientY = (ev.clientY - rect.top) * dpr;

            let normX = (clientX - ar.x) / ar.w;
            let normY = (clientY - ar.y) / ar.h;
            normX = Math.max(0, Math.min(1, normX));
            normY = Math.max(0, Math.min(1, normY));

            // Pressure curve & sensitivity mapping
            let rawPressure = ev.pressure;
            let pressure = rawPressure;
            if (pressure > 0) {
                if (gamma > 0 && Math.abs(gamma - 1.0) > 0.01) {
                    pressure = Math.pow(pressure, 1.0 / gamma);
                }
                pressure = Math.max(minP, Math.min(1.0, pressure));
            }

            // Determine state for HUD
            let state = "hover";
            if (ev.buttons & 32) state = "eraser";
            else if (ev.buttons & 2) state = "barrel";
            else if (ev.buttons & 1 || eventType === "pointerdown" || pressure > 0) state = "contact";
            else if (eventType === "pointerleave" || eventType === "pointerout" || eventType === "pointercancel") state = "idle";

            // Update HUD on last event in batch
            if (i === count - 1) {
                this.hud.update(ev.pointerType || "pen", state, pressure, normX, normY);
            }

            if (eventType === "pointerdown" || (eventType === "pointermove" && state === "contact")) {
                tabletEngine.addPoint(ev.pointerId, ev.clientX, ev.clientY, pressure);
            } else if (eventType === "pointerup" || eventType === "pointercancel" || eventType === "pointerleave") {
                tabletEngine.endStroke(ev.pointerId);
            }

            let btn = ev.button;
            if (btn === 2) btn = 1;
            else if (btn === 1) btn = 2;
            let buttonBit = btn < 0 ? 0 : 1 << btn;

            let offset = i * 18;
            view.setUint8(offset + 0, eventTypeToByte(eventType));
            view.setUint8(offset + 1, pointerTypeToByte(ev.pointerType));
            view.setUint16(offset + 2, ev.pointerId & 0xFFFF, true);
            view.setUint8(offset + 4, ev.buttons & 0xFF);
            view.setUint8(offset + 5, buttonBit & 0xFF);
            view.setUint16(offset + 6, Math.max(0, Math.min(65535, Math.round(normX * 65535))), true);
            view.setUint16(offset + 8, Math.max(0, Math.min(65535, Math.round(normY * 65535))), true);
            view.setUint16(offset + 10, Math.max(0, Math.min(65535, Math.round(pressure * 65535))), true);
            view.setInt8(offset + 12, Math.max(-128, Math.min(127, Math.round(ev.tiltX || 0))));
            view.setInt8(offset + 13, Math.max(-128, Math.min(127, Math.round(ev.tiltY || 0))));
            view.setUint32(offset + 14, Math.round(ev.timeStamp) & 0xFFFFFFFF, true);
        }

        if (this.webSocket.readyState === WebSocket.OPEN) {
            this.webSocket.send(buffer);
        }
    }
}

let settings: Settings;
let tabletEngine: TabletEngine;
let pointerHandler: PointerHandler;
let activeWs: WebSocket | null = null;
let reconnectTimer: any = null;

function init() {
    if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
    }
    if (activeWs && (activeWs.readyState === WebSocket.OPEN || activeWs.readyState === WebSocket.CONNECTING)) {
        return;
    }

    let wsUri = `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws${location.search}`;
    let ws = new WebSocket(wsUri);
    ws.binaryType = "arraybuffer";
    activeWs = ws;

    ws.onopen = () => {
        log(LogLevel.INFO, "Connected to Weylus tablet server");
        if (!settings) {
            settings = new Settings(ws);
        } else {
            settings.webSocket = ws;
        }

        if (!tabletEngine) {
            let canvas = document.getElementById("tablet_canvas") as HTMLCanvasElement;
            tabletEngine = new TabletEngine(canvas);
        }

        if (!pointerHandler) {
            pointerHandler = new PointerHandler(ws);
        } else {
            pointerHandler.webSocket = ws;
        }

        ws.send('"GetCapturableList"');
    };

    ws.onmessage = (e) => {
        if (typeof e.data !== "string") return;
        try {
            let msg = JSON.parse(e.data);
            if (msg.CapturableList) {
                let select = document.getElementById("window") as HTMLSelectElement;
                if (!select) return;
                select.innerHTML = "";
                let savedId = localStorage.getItem("weylus_saved_capturable_id");
                let matchedIdx = -1;
                msg.CapturableList.forEach((name: string, idx: number) => {
                    let opt = document.createElement("option");
                    opt.value = idx.toString();
                    opt.textContent = name;
                    select.appendChild(opt);
                    if (savedId !== null && idx.toString() === savedId) {
                        matchedIdx = idx;
                    }
                });
                if (matchedIdx >= 0) {
                    select.selectedIndex = matchedIdx;
                } else {
                    select.selectedIndex = 0;
                }
                settings.updateTargetMonitorAspect();
                settings.send_server_config();
            } else if (msg.ConfigOk) {
                log(LogLevel.DEBUG, "Server config confirmed");
            } else if (msg.Error) {
                log(LogLevel.ERROR, `Server error: ${msg.Error}`);
            }
        } catch (_) {}
    };

    ws.onerror = () => {
        log(LogLevel.WARN, "WebSocket error encountered");
    };

    ws.onclose = () => {
        log(LogLevel.WARN, "WebSocket disconnected. Reconnecting in 2s...");
        activeWs = null;
        reconnectTimer = setTimeout(() => init(), 2000);
    };
}

document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") {
        if (!activeWs || activeWs.readyState === WebSocket.CLOSED) {
            init();
        }
    }
});
