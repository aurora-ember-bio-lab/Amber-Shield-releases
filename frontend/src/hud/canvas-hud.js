// Canvas2D "organic veins" HUD - deliberately Canvas2D, not WebGL.
//
// Architecture-review note carried into the code: enterprise fleets often
// lock GPU drivers or disable hardware acceleration, and that's exactly
// the buyer this product is for. Canvas2D covers the data rates a log/
// event HUD actually needs (tens of routes, a handful of updates/sec);
// move to WebGL/wgpu later only if profiling proves this is the
// bottleneck, not before.
//
// Implements the doc's state machine directly:
//   info      -> emerald, steady 1Hz pulse
//   warn      -> amber,   jittered 3Hz
//   critical  -> crimson, broken path + breathing fade pulse

const COLORS = {
  info: "#38f2a3",
  warn: "#ffb648",
  critical: "#ff4d6a",
};

const IDLE_TIMEOUT_MS = 12_000; // routes fade out and get pruned after this

export class CanvasHud {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    /** @type {Map<string, Route>} keyed by event source */
    this.routes = new Map();
    this.dpr = Math.max(1, window.devicePixelRatio || 1);
    this._resize();
    window.addEventListener("resize", () => this._resize());
    this._raf = requestAnimationFrame((t) => this._tick(t));
  }

  _resize() {
    const { canvas, dpr } = this;
    canvas.width = canvas.clientWidth * dpr;
    canvas.height = canvas.clientHeight * dpr;
  }

  /** Feed one normalized `Event` (see core-engine::types::Event) into the HUD. */
  addEvent(event) {
    const key = event.source;
    const severity = event.severity;
    const existing = this.routes.get(key);
    const angle = existing ? existing.angle : this.routes.size * (Math.PI * 2 / 6) - Math.PI / 2;

    this.routes.set(key, {
      source: key,
      severity,
      angle,
      lastSeen: performance.now(),
      // A little per-route jitter phase so multiple amber/crimson routes
      // don't pulse in perfect lockstep, which would read as fake.
      phase: existing ? existing.phase : Math.random() * Math.PI * 2,
    });
  }

  _tick(tMs) {
    const { ctx, canvas, dpr } = this;
    const w = canvas.width, h = canvas.height;
    ctx.clearRect(0, 0, w, h);

    const now = performance.now();
    for (const [key, route] of this.routes) {
      if (now - route.lastSeen > IDLE_TIMEOUT_MS) this.routes.delete(key);
    }

    const cx = w / 2, cy = h / 2;
    const coreR = 26 * dpr;
    const reach = Math.min(w, h) * 0.38;

    // Core node
    ctx.beginPath();
    ctx.arc(cx, cy, coreR, 0, Math.PI * 2);
    ctx.strokeStyle = "rgba(110,231,255,0.6)";
    ctx.lineWidth = 1.5 * dpr;
    ctx.stroke();

    let i = 0;
    const n = Math.max(this.routes.size, 1);
    for (const route of this.routes.values()) {
      const angle = route.angle ?? (i / n) * Math.PI * 2 - Math.PI / 2;
      const ex = cx + Math.cos(angle) * reach;
      const ey = cy + Math.sin(angle) * reach;
      this._drawRoute(ctx, tMs, dpr, cx, cy, ex, ey, route);
      i++;
    }

    this._raf = requestAnimationFrame((t) => this._tick(t));
  }

  _drawRoute(ctx, tMs, dpr, cx, cy, ex, ey, route) {
    const color = COLORS[route.severity] ?? COLORS.info;
    const t = tMs / 1000;

    let alpha = 0.85;
    let broken = false;
    let width = 2 * dpr;

    if (route.severity === "info") {
      // steady 1Hz pulse
      alpha = 0.55 + 0.35 * (0.5 + 0.5 * Math.sin(t * 2 * Math.PI * 1 + route.phase));
    } else if (route.severity === "warn") {
      // jittered 3Hz
      const jitter = (Math.sin(t * 47 + route.phase) * 0.15);
      alpha = 0.5 + 0.5 * Math.abs(Math.sin(t * 2 * Math.PI * 3 + route.phase)) + jitter;
      width = (2 + Math.sin(t * 30 + route.phase) * 0.6) * dpr;
    } else if (route.severity === "critical") {
      // broken path + slow breathing fade
      broken = true;
      alpha = 0.35 + 0.45 * (0.5 + 0.5 * Math.sin(t * 2 * Math.PI * 0.5 + route.phase));
    }
    alpha = Math.max(0, Math.min(1, alpha));

    ctx.save();
    ctx.strokeStyle = hexWithAlpha(color, alpha);
    ctx.lineWidth = width;
    ctx.shadowColor = color;
    ctx.shadowBlur = 12 * dpr;

    if (broken) {
      // Two segments with a visible gap in the middle = "broken path".
      const mx = cx + (ex - cx) * 0.42;
      const my = cy + (ey - cy) * 0.42;
      const mx2 = cx + (ex - cx) * 0.58;
      const my2 = cy + (ey - cy) * 0.58;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.lineTo(mx, my);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(mx2, my2);
      ctx.lineTo(ex, ey);
      ctx.stroke();
    } else {
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.quadraticCurveTo(
        cx + (ex - cx) * 0.5 + Math.sin(t + route.phase) * 8 * dpr,
        cy + (ey - cy) * 0.5 + Math.cos(t + route.phase) * 8 * dpr,
        ex,
        ey
      );
      ctx.stroke();
    }

    // Endpoint node
    ctx.beginPath();
    ctx.arc(ex, ey, 5 * dpr, 0, Math.PI * 2);
    ctx.fillStyle = hexWithAlpha(color, Math.min(1, alpha + 0.2));
    ctx.fill();
    ctx.restore();

    // Label
    ctx.save();
    ctx.font = `${10 * dpr}px ui-monospace, monospace`;
    ctx.fillStyle = "rgba(216,230,242,0.75)";
    ctx.textAlign = ex > cx ? "left" : "right";
    ctx.fillText(route.source.replace("_", " "), ex + (ex > cx ? 8 : -8) * dpr, ey + 3 * dpr);
    ctx.restore();
  }
}

function hexWithAlpha(hex, alpha) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${alpha})`;
}
