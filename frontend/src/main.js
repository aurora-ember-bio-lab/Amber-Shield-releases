import { CanvasHud } from "./hud/canvas-hud.js";

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

function renderHeatmapRows(container, rows) {
  container.innerHTML = "";
  if (!rows.length) {
    container.innerHTML = `<div class="heat-row">no files scored yet</div>`;
    return;
  }
  for (const row of rows) {
    const el = document.createElement("div");
    el.className = `heat-row heat-${row.category}`;
    el.innerHTML = `<span>${escapeHtml(row.path)}</span><span class="score">${row.score}</span>`;
    container.appendChild(el);
  }
}

function demoHeatmap() {
  return [
    { path: "src/legacy/session_cache.js", score: 18, category: "stale" },
    { path: "src/core/types.rs", score: 3, category: "stable" },
  ];
}

function startDemoFeed(renderEvent) {
  const sources = ["log_watch", "security", "code_map"];
  const severities = ["info", "info", "info", "warn", "critical"];
  setInterval(() => {
    const source = sources[Math.floor(Math.random() * sources.length)];
    const severity = severities[Math.floor(Math.random() * severities.length)];
    renderEvent({
      id: crypto.randomUUID(),
      timestamp: new Date().toISOString(),
      source,
      severity,
      message: `(demo) synthetic ${severity} event from ${source}`,
      latency_ms: severity === "warn" ? 1800 + Math.random() * 400 : null,
      metadata: {},
    });
  }, 1400);
}

// Canned fallback for when Ollama isn't reachable (or we're in demo mode
// outside Tauri entirely) - the AI drawer still explains what it *would*
// have asked, rather than just failing silently.
function fallbackExplanation(evt) {
  const src = evt.source.replace(/_/g, " ");
  if (evt.severity === "critical") return `Critical — ${src} reported "${evt.message}". (Ollama not reachable — start it with \`ollama serve\` for a real explanation.)`;
  if (evt.severity === "warn") return `Degraded — ${src}: "${evt.message}". (Ollama not reachable — start it with \`ollama serve\` for a real explanation.)`;
  return `Nominal — ${src} looks healthy.`;
}

async function init() {
  const canvas = document.getElementById("hud-canvas");
  const hud = new CanvasHud(canvas);
  const logEl = document.getElementById("event-log");

  const aiDrawer = document.getElementById("ai-drawer");
  const aiText = document.getElementById("ai-drawer-text");
  const aiModelTag = document.getElementById("ai-model-tag");
  let selectedRow = null;
  let llmReachable = false;

  document.getElementById("ai-drawer-close").addEventListener("click", () => {
    aiDrawer.hidden = true;
    if (selectedRow) { selectedRow.classList.remove("selected"); selectedRow = null; }
  });

  function renderEvent(evt) {
    hud.addEvent(evt);
    const row = document.createElement("div");
    row.className = `event-row sev-${evt.severity}`;
    row._evt = evt;
    row.innerHTML = `<span class="dot"></span><span class="src">${escapeHtml(evt.source)}</span><span class="msg">${escapeHtml(evt.message)}</span>`;
    row.addEventListener("click", async () => {
      if (selectedRow) selectedRow.classList.remove("selected");
      selectedRow = row;
      row.classList.add("selected");
      aiDrawer.hidden = false;

      if (tauriRef && llmReachable) {
        aiText.textContent = "thinking…";
        try {
          aiText.textContent = await tauriRef.core.invoke("explain_event_with_llm", { event: evt });
        } catch (e) {
          aiText.textContent = fallbackExplanation(evt);
        }
      } else {
        aiText.textContent = fallbackExplanation(evt);
      }
    });
    logEl.appendChild(row);
    while (logEl.children.length > 200) logEl.removeChild(logEl.firstChild);
    logEl.scrollTop = logEl.scrollHeight;
  }

  const statusPill = document.getElementById("status-pill");
  const tauri = window.__TAURI__;
  const tauriRef = tauri; // captured for the row click handler above

  if (tauri) {
    statusPill.textContent = "live · tauri core";
    tauri.event.listen("amber://event", (e) => renderEvent(e.payload));

    try {
      const recent = await tauri.core.invoke("get_recent_events", { limit: 50 });
      recent.forEach(renderEvent);
    } catch (e) {
      console.warn("get_recent_events failed", e);
    }

    try {
      llmReachable = await tauri.core.invoke("llm_is_reachable");
      aiModelTag.textContent = llmReachable ? "· local ollama connected" : "· ollama not running";
    } catch (e) {
      aiModelTag.textContent = "· ollama not running";
    }

    document.getElementById("fingerprint-btn").addEventListener("click", async () => {
      const out = document.getElementById("fingerprint-out");
      try {
        out.textContent = await tauri.core.invoke("hardware_fingerprint");
      } catch (e) {
        out.textContent = `error: ${e}`;
      }
    });

    document.getElementById("scan-btn").addEventListener("click", async () => {
      const path = document.getElementById("scan-path").value || ".";
      const list = document.getElementById("heatmap-list");
      list.innerHTML = `<div class="heat-row">scanning…</div>`;
      try {
        const result = await tauri.core.invoke("scan_code_heatmap", { projectPath: path });
        renderHeatmapRows(list, result.heatmap ?? []);
        if (result.scanner_errors?.length) {
          list.insertAdjacentHTML(
            "afterbegin",
            `<div class="heat-row heat-stale">scanner warnings: ${escapeHtml(result.scanner_errors.join("; "))}</div>`,
          );
        }
        if (typeof result.scans_remaining === "number") {
          list.insertAdjacentHTML(
            "beforeend",
            `<div class="heat-row">${result.scans_remaining} free scan${result.scans_remaining === 1 ? "" : "s"} left — <a href="https://www.ambershield.app" target="_blank" rel="noreferrer">upgrade to Pro</a> for unlimited</div>`,
          );
        }
      } catch (e) {
        list.innerHTML = `<div class="heat-row heat-vulnerable">scan failed: ${escapeHtml(String(e))}</div>`;
      }
    });
  } else {
    // Running outside the Tauri shell (e.g. previewed in a plain browser).
    // Demo mode keeps the UI legible instead of sitting empty.
    statusPill.textContent = "demo mode · not running inside Tauri";
    statusPill.parentElement.style.color = "var(--amber)";
    startDemoFeed(renderEvent);

    document.getElementById("fingerprint-btn").addEventListener("click", () => {
      document.getElementById("fingerprint-out").textContent =
        "(demo mode - open inside the Tauri app for a real hardware fingerprint)";
    });
    document.getElementById("scan-btn").addEventListener("click", () => {
      renderHeatmapRows(document.getElementById("heatmap-list"), demoHeatmap());
    });
  }
}

init();
