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

    setupSearch(tauri);
    setupProcesses(tauri);
    setupSettings(tauri);
    setupLicense(tauri);
    setupScheduledTasks(tauri);
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
    setupDemoSearch();
    setupDemoProcesses();
    setupDemoSettings();
    setupDemoLicense();
    setupDemoScheduledTasks();
  }
}

// -----------------------------------------------------------------------
// Semantic Search (Tauri mode)
// -----------------------------------------------------------------------
function setupSearch(tauri) {
  const input = document.getElementById("search-input");
  const btn = document.getElementById("search-btn");
  const results = document.getElementById("search-results");

  async function doSearch() {
    const query = input.value.trim();
    if (!query) return;
    results.innerHTML = '<div class="search-result-item">searching...</div>';
    try {
      const hits = await tauri.core.invoke("vector_search", { query, limit: 10 });
      if (!hits.length) {
        results.innerHTML = '<div class="search-result-item" style="color:var(--text-dim)">no similar events found</div>';
        return;
      }
      results.innerHTML = "";
      for (const h of hits) {
        const div = document.createElement("div");
        div.className = "search-result-item";
        div.innerHTML = `<div class="sr-sim">${escapeHtml(h.source)} · ${escapeHtml(h.severity)}</div><div class="sr-msg">${escapeHtml(h.message)}</div><div class="sr-meta">${escapeHtml(h.ts)}</div>`;
        results.appendChild(div);
      }
    } catch (e) {
      results.innerHTML = `<div class="search-result-item" style="color:#f44">${escapeHtml(String(e))}</div>`;
    }
  }

  btn.addEventListener("click", doSearch);
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") doSearch(); });
}

function setupDemoSearch() {
  const results = document.getElementById("search-results");
  document.getElementById("search-btn").addEventListener("click", () => {
    results.innerHTML = '<div class="search-result-item" style="color:var(--text-dim)">(demo mode — semantic search requires Tauri + Ollama)</div>';
  });
}

// -----------------------------------------------------------------------
// Process Monitor (Tauri mode)
// -----------------------------------------------------------------------
function setupProcesses(tauri) {
  const btn = document.getElementById("proc-scan-btn");
  const list = document.getElementById("proc-list");

  btn.addEventListener("click", async () => {
    list.innerHTML = '<div class="proc-row" style="color:var(--text-dim)">scanning...</div>';
    try {
      const procs = await tauri.core.invoke("scan_processes");
      list.innerHTML = '<div class="proc-row proc-header"><span>PID</span><span>Name</span><span>CPU %</span><span>Memory</span></div>';
      const sorted = procs.sort((a, b) => b.memory_bytes - a.memory_bytes).slice(0, 100);
      for (const p of sorted) {
        const row = document.createElement("div");
        row.className = "proc-row";
        const mb = (p.memory_bytes / 1024 / 1024).toFixed(1);
        row.innerHTML = `<span>${p.pid}</span><span>${escapeHtml(p.name)}</span><span>${p.cpu_usage.toFixed(1)}</span><span>${mb} MB</span>`;
        list.appendChild(row);
      }
    } catch (e) {
      list.innerHTML = `<div class="proc-row" style="color:#f44">${escapeHtml(String(e))}</div>`;
    }
  });
}

function setupDemoProcesses() {
  document.getElementById("proc-scan-btn").addEventListener("click", () => {
    document.getElementById("proc-list").innerHTML = '<div class="proc-row" style="color:var(--text-dim)">(demo mode — process scan requires Tauri)</div>';
  });
}

// -----------------------------------------------------------------------
// Log Watch Config (Tauri mode)
// -----------------------------------------------------------------------
function setupSettings(tauri) {
  const textarea = document.getElementById("watch-paths");
  const saveBtn = document.getElementById("config-save-btn");
  const status = document.getElementById("config-status");

  // Load current config
  tauri.core.invoke("get_config").then((cfg) => {
    textarea.value = (cfg.watch_paths || []).join("\n");
  }).catch(() => {});

  saveBtn.addEventListener("click", async () => {
    const paths = textarea.value.split("\n").map((l) => l.trim()).filter(Boolean);
    try {
      await tauri.core.invoke("save_config", { config: { watch_paths: paths } });
      status.textContent = "saved";
      status.style.color = "#4caf50";
      setTimeout(() => { status.textContent = ""; }, 2000);
    } catch (e) {
      status.textContent = `error: ${e}`;
      status.style.color = "#f44";
    }
  });
}

function setupDemoSettings() {
  document.getElementById("config-save-btn").addEventListener("click", () => {
    document.getElementById("config-status").textContent = "(demo mode)";
  });
}

// -----------------------------------------------------------------------
// License Flow (Tauri mode)
// -----------------------------------------------------------------------
function setupLicense(tauri) {
  const statusText = document.getElementById("license-status-text");
  const statusDot = document.querySelector(".license-dot");
  const input = document.getElementById("license-input");
  const installBtn = document.getElementById("license-install-btn");
  const msg = document.getElementById("license-msg");
  const fpDisplay = document.getElementById("fingerprint-display");

  // Check license status + fingerprint
  async function refreshLicense() {
    try {
      const status = await tauri.core.invoke("license_status");
      if (status.active) {
        statusDot.className = "license-dot active";
        statusText.textContent = `Active — seat: ${status.seat_id || "—"}`;
      } else {
        statusDot.className = "license-dot inactive";
        statusText.textContent = status.error || "No license installed";
      }
    } catch (e) {
      statusDot.className = "license-dot inactive";
      statusText.textContent = `Error: ${e}`;
    }
  }

  try {
    tauri.core.invoke("hardware_fingerprint").then((fp) => {
      fpDisplay.textContent = fp;
    }).catch(() => { fpDisplay.textContent = "unavailable"; });
  } catch (_) {}

  refreshLicense();

  installBtn.addEventListener("click", async () => {
    const json = input.value.trim();
    if (!json) { msg.textContent = "paste license JSON first"; msg.style.color = "#f44"; return; }
    msg.textContent = "installing...";
    msg.style.color = "var(--text-dim)";
    try {
      const result = await tauri.core.invoke("install_license", { licenseJson: json });
      if (result.active) {
        msg.textContent = "license installed successfully";
        msg.style.color = "#4caf50";
        input.value = "";
      } else {
        msg.textContent = result.error || "installation failed";
        msg.style.color = "#f44";
      }
      refreshLicense();
    } catch (e) {
      msg.textContent = `error: ${e}`;
      msg.style.color = "#f44";
    }
  });
}

function setupDemoLicense() {
  document.getElementById("license-status-text").textContent = "(demo mode — license requires Tauri)";
  document.getElementById("license-install-btn").addEventListener("click", () => {
    document.getElementById("license-msg").textContent = "(demo mode)";
  });
}

// -----------------------------------------------------------------------
// Scheduled Tasks (Tauri mode)
// -----------------------------------------------------------------------
function setupScheduledTasks(tauri) {
  const list = document.getElementById("task-list");
  const createBtn = document.getElementById("task-create-btn");

  function renderTasks(tasks) {
    list.innerHTML = '<div class="task-row task-header"><span>Name</span><span>Kind</span><span>Schedule</span><span>On</span><span></span></div>';
    if (!tasks.length) {
      list.insertAdjacentHTML("beforeend", '<div class="task-row" style="color:var(--text-dim)">no scheduled tasks — create one above</div>');
      return;
    }
    for (const t of tasks) {
      const row = document.createElement("div");
      row.className = "task-row";
      const lastRun = t.last_run ? new Date(t.last_run).toLocaleString() : "never";
      let schedule;
      if (t.cron_expr) {
        schedule = `<span class="task-schedule">${escapeHtml(t.cron_expr)}</span>`;
      } else {
        const secs = t.interval_secs;
        schedule = secs >= 3600 ? `every ${(secs/3600).toFixed(1)}h` : secs >= 60 ? `every ${(secs/60).toFixed(0)}m` : `every ${secs}s`;
      }
      row.innerHTML = `
        <span>${escapeHtml(t.name)} <span class="task-last-run">· last: ${lastRun}</span></span>
        <span>${escapeHtml(t.kind)}</span>
        <span>${schedule}</span>
        <input type="checkbox" class="task-enabled" data-id="${t.id}" ${t.enabled ? "checked" : ""} />
        <span>
          <button class="task-btn" data-action="run" data-id="${t.id}">Run now</button>
          <button class="task-btn danger" data-action="delete" data-id="${t.id}">Delete</button>
        </span>`;
      list.appendChild(row);
    }

    list.querySelectorAll(".task-enabled").forEach((cb) => {
      cb.addEventListener("change", async () => {
        try {
          await tauri.core.invoke("toggle_scheduled_task", { id: cb.dataset.id, enabled: cb.checked });
        } catch (e) { console.warn("toggle failed", e); }
      });
    });

    list.querySelectorAll(".task-btn").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const id = btn.dataset.id;
        try {
          if (btn.dataset.action === "run") {
            btn.textContent = "running...";
            const msg = await tauri.core.invoke("run_task_now", { id });
            btn.textContent = msg.substring(0, 20);
            setTimeout(() => { btn.textContent = "Run now"; }, 2000);
          } else if (btn.dataset.action === "delete") {
            await tauri.core.invoke("delete_scheduled_task", { id });
            refresh();
          }
        } catch (e) { console.warn("task action failed", e); }
      });
    });
  }

  async function refresh() {
    try {
      const tasks = await tauri.core.invoke("list_scheduled_tasks");
      renderTasks(tasks);
    } catch (e) { console.warn("list tasks failed", e); }
  }

  createBtn.addEventListener("click", async () => {
    const name = document.getElementById("task-name").value.trim();
    const kind = document.getElementById("task-kind").value;
    const interval = parseInt(document.getElementById("task-interval").value, 10) || 300;
    const cronExpr = document.getElementById("task-cron").value.trim() || null;
    const target = document.getElementById("task-target").value.trim() || null;
    if (!name) return;
    try {
      await tauri.core.invoke("create_scheduled_task", {
        request: { name, kind, interval_secs: cronExpr ? 60 : interval, cron_expr: cronExpr, target }
      });
      document.getElementById("task-name").value = "";
      document.getElementById("task-target").value = "";
      document.getElementById("task-cron").value = "";
      refresh();
    } catch (e) { console.warn("create task failed", e); }
  });

  refresh();
}

function setupDemoScheduledTasks() {
  document.getElementById("task-create-btn").addEventListener("click", () => {
    document.getElementById("task-list").innerHTML = '<div class="task-row" style="color:var(--text-dim)">(demo mode — scheduling requires Tauri)</div>';
  });
}

init();
