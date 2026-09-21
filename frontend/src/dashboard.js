import { CanvasHud } from "./hud/canvas-hud.js";

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

// Navigation
function navigateTo(section) {
  document.querySelectorAll(".section").forEach((s) => s.classList.remove("active"));
  document.querySelectorAll(".nav-item").forEach((n) => n.classList.remove("active"));
  const target = document.getElementById("section-" + section);
  const nav = document.querySelector(`.nav-item[data-section="${section}"]`);
  if (target) target.classList.add("active");
  if (nav) nav.classList.add("active");
  document.getElementById("page-title").textContent = nav?.querySelector("span")?.textContent || section;
  document.getElementById("sidebar").classList.remove("open");
}
window.navigateTo = navigateTo;

// Sidebar nav clicks
document.querySelectorAll(".nav-item[data-section]").forEach((item) => {
  item.addEventListener("click", (e) => {
    e.preventDefault();
    navigateTo(item.dataset.section);
  });
});

// Mobile menu toggle
document.getElementById("menu-toggle").addEventListener("click", () => {
  document.getElementById("sidebar").classList.toggle("open");
});

// Close sidebar on outside click (mobile)
document.addEventListener("click", (e) => {
  const sidebar = document.getElementById("sidebar");
  if (sidebar.classList.contains("open") && !sidebar.contains(e.target) && e.target.id !== "menu-toggle") {
    sidebar.classList.remove("open");
  }
});

function renderHeatmapRows(container, rows) {
  container.innerHTML = "";
  if (!rows.length) {
    container.innerHTML = '<div class="empty-state">No files scored yet</div>';
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

function fallbackExplanation(evt) {
  const src = evt.source.replace(/_/g, " ");
  if (evt.severity === "critical") return `Critical — ${src} reported "${evt.message}". (Ollama not reachable — start it with \`ollama serve\` for a real explanation.)`;
  if (evt.severity === "warn") return `Degraded — ${src}: "${evt.message}". (Ollama not reachable — start it with \`ollama serve\` for a real explanation.)`;
  return `Nominal — ${src} looks healthy.`;
}

async function init() {
  const logEl = document.getElementById("event-log");
  const overviewEvents = document.getElementById("overview-events");
  const aiDrawer = document.getElementById("ai-drawer");
  const aiText = document.getElementById("ai-drawer-text");
  const aiModelTag = document.getElementById("ai-model-tag");
  let selectedRow = null;
  let llmReachable = false;
  let eventCount = 0;

  document.getElementById("ai-drawer-close").addEventListener("click", () => {
    aiDrawer.hidden = true;
    if (selectedRow) { selectedRow.classList.remove("selected"); selectedRow = null; }
  });

  function renderEvent(evt) {
    eventCount++;
    document.getElementById("stat-events").textContent = eventCount;

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
        aiText.textContent = "thinking...";
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

    // Also add to overview (keep last 10)
    if (overviewEvents.querySelector(".empty-state")) overviewEvents.innerHTML = "";
    const clone = row.cloneNode(true);
    overviewEvents.insertBefore(clone, overviewEvents.firstChild);
    while (overviewEvents.children.length > 10) overviewEvents.removeChild(overviewEvents.lastChild);
  }

  const statusDot = document.getElementById("status-dot");
  const statusText = document.getElementById("status-text");
  const licenseBadge = document.getElementById("license-badge");
  const tauri = window.__TAURI__;
  const tauriRef = tauri;

  if (tauri) {
    statusDot.classList.add("live");
    statusText.textContent = "Live — Tauri core";
    tauri.event.listen("amber://event", (e) => renderEvent(e.payload));

    try {
      const recent = await tauri.core.invoke("get_recent_events", { limit: 50 });
      recent.forEach(renderEvent);
    } catch (e) { console.warn("get_recent_events failed", e); }

    try {
      llmReachable = await tauri.core.invoke("llm_is_reachable");
      aiModelTag.textContent = llmReachable ? "local ollama connected" : "ollama not running";
    } catch (e) { aiModelTag.textContent = "ollama not running"; }

    // License status
    try {
      const ls = await tauri.core.invoke("license_status");
      if (ls.active) {
        licenseBadge.textContent = "Active";
        licenseBadge.classList.add("active");
      }
    } catch (e) {}

    // Processes
    try {
      const procs = await tauri.core.invoke("scan_processes");
      document.getElementById("stat-processes").textContent = procs.length;
    } catch (e) {}

    // Tasks
    try {
      const tasks = await tauri.core.invoke("list_scheduled_tasks");
      document.getElementById("stat-tasks").textContent = tasks.filter(t => t.enabled).length;
    } catch (e) {}

    setupSearch(tauri);
    setupProcesses(tauri);
    setupSettings(tauri);
    setupLicense(tauri);
    setupScheduledTasks(tauri);
    setupScan(tauri);
  } else {
    statusDot.classList.add("demo");
    statusText.textContent = "Demo mode";
    startDemoFeed(renderEvent);

    setupDemoSearch();
    setupDemoProcesses();
    setupDemoSettings();
    setupDemoLicense();
    setupDemoScheduledTasks();
    setupDemoScan();
  }
}

// Search
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
        results.innerHTML = '<div class="search-result-item" style="color:var(--text-muted)">no similar events found</div>';
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
      results.innerHTML = `<div class="search-result-item" style="color:var(--red)">${escapeHtml(String(e))}</div>`;
    }
  }

  btn.addEventListener("click", doSearch);
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") doSearch(); });
}

function setupDemoSearch() {
  document.getElementById("search-btn").addEventListener("click", () => {
    document.getElementById("search-results").innerHTML = '<div class="search-result-item" style="color:var(--text-muted)">(demo mode — requires Tauri + Ollama)</div>';
  });
}

// Processes
function setupProcesses(tauri) {
  const btn = document.getElementById("proc-scan-btn");
  const list = document.getElementById("proc-list");

  btn.addEventListener("click", async () => {
    list.innerHTML = '<div class="empty-state">scanning...</div>';
    try {
      const procs = await tauri.core.invoke("scan_processes");
      document.getElementById("stat-processes").textContent = procs.length;
      list.innerHTML = '<table class="proc-table"><thead><tr><th>PID</th><th>Name</th><th>CPU %</th><th>Memory</th></tr></thead><tbody></tbody></table>';
      const tbody = list.querySelector("tbody");
      const sorted = procs.sort((a, b) => b.memory_bytes - a.memory_bytes).slice(0, 100);
      for (const p of sorted) {
        const row = document.createElement("tr");
        const mb = (p.memory_bytes / 1024 / 1024).toFixed(1);
        row.innerHTML = `<td>${p.pid}</td><td>${escapeHtml(p.name)}</td><td>${p.cpu_usage.toFixed(1)}</td><td>${mb} MB</td>`;
        tbody.appendChild(row);
      }
    } catch (e) {
      list.innerHTML = `<div class="empty-state" style="color:var(--red)">${escapeHtml(String(e))}</div>`;
    }
  });
}

function setupDemoProcesses() {
  document.getElementById("proc-scan-btn").addEventListener("click", () => {
    document.getElementById("proc-list").innerHTML = '<div class="empty-state">(demo mode — requires Tauri)</div>';
  });
}

// Scan
function setupScan(tauri) {
  const btn = document.getElementById("scan-btn");
  const list = document.getElementById("heatmap-list");

  btn.addEventListener("click", async () => {
    const path = document.getElementById("scan-path").value || ".";
    list.innerHTML = '<div class="empty-state">scanning...</div>';
    try {
      const result = await tauri.core.invoke("scan_code_heatmap", { projectPath: path });
      renderHeatmapRows(list, result.heatmap ?? []);
      if (result.scanner_errors?.length) {
        list.insertAdjacentHTML("afterbegin", `<div class="heat-row heat-vulnerable">scanner warnings: ${escapeHtml(result.scanner_errors.join("; "))}</div>`);
      }
    } catch (e) {
      list.innerHTML = `<div class="empty-state" style="color:var(--red)">${escapeHtml(String(e))}</div>`;
    }
  });
}

function setupDemoScan() {
  document.getElementById("scan-btn").addEventListener("click", () => {
    renderHeatmapRows(document.getElementById("heatmap-list"), demoHeatmap());
  });
}

// Settings
function setupSettings(tauri) {
  const textarea = document.getElementById("watch-paths");
  const saveBtn = document.getElementById("config-save-btn");
  const status = document.getElementById("config-status");

  tauri.core.invoke("get_config").then((cfg) => {
    textarea.value = (cfg.watch_paths || []).join("\n");
  }).catch(() => {});

  saveBtn.addEventListener("click", async () => {
    const paths = textarea.value.split("\n").map((l) => l.trim()).filter(Boolean);
    try {
      await tauri.core.invoke("save_config", { config: { watch_paths: paths } });
      status.textContent = "Saved";
      status.style.color = "var(--green)";
      setTimeout(() => { status.textContent = ""; }, 2000);
    } catch (e) {
      status.textContent = `Error: ${e}`;
      status.style.color = "var(--red)";
    }
  });
}

function setupDemoSettings() {
  document.getElementById("config-save-btn").addEventListener("click", () => {
    document.getElementById("config-status").textContent = "(demo mode)";
  });
}

// License / Authorization
function setupLicense(tauri) {
  const activeCard = document.getElementById("auth-active-card");
  const formCard = document.getElementById("auth-form-card");
  const seatIdEl = document.getElementById("auth-seat-id");
  const expiresEl = document.getElementById("auth-expires");
  const fpActive = document.getElementById("auth-fingerprint-active");
  const fpForm = document.getElementById("auth-fingerprint-form");
  const keyInput = document.getElementById("auth-key");
  const activateBtn = document.getElementById("auth-activate-btn");
  const pasteBtn = document.getElementById("auth-paste-btn");
  const rawJson = document.getElementById("auth-raw-json");
  const rawActivateBtn = document.getElementById("auth-raw-activate-btn");
  const msgEl = document.getElementById("auth-msg");
  const deactivateBtn = document.getElementById("auth-deactivate-btn");
  const copyFpBtn = document.getElementById("auth-copy-fp");
  const licenseBadge = document.getElementById("license-badge");

  function showMsg(text, type) {
    msgEl.textContent = text;
    msgEl.className = "auth-msg " + type;
  }

  function clearMsg() {
    msgEl.className = "auth-msg";
    msgEl.textContent = "";
  }

  function formatDate(unix) {
    if (!unix) return "—";
    return new Date(unix * 1000).toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" });
  }

  async function refreshLicense() {
    clearMsg();
    try {
      const status = await tauri.core.invoke("license_status");
      if (status.active) {
        activeCard.style.display = "block";
        formCard.style.display = "none";
        seatIdEl.textContent = status.seat_id || "—";
        expiresEl.textContent = formatDate(status.expires_at_unix);
        licenseBadge.textContent = "Active";
        licenseBadge.classList.add("active");
      } else {
        activeCard.style.display = "none";
        formCard.style.display = "block";
        licenseBadge.textContent = "Inactive";
        licenseBadge.classList.remove("active");
      }
    } catch (e) {
      activeCard.style.display = "none";
      formCard.style.display = "block";
    }
  }

  tauri.core.invoke("hardware_fingerprint").then((fp) => {
    if (fpActive) fpActive.textContent = fp;
    if (fpForm) fpForm.textContent = fp;
  }).catch(() => {
    if (fpActive) fpActive.textContent = "unavailable";
    if (fpForm) fpForm.textContent = "unavailable";
  });

  // Paste from clipboard
  if (pasteBtn) {
    pasteBtn.addEventListener("click", async () => {
      try {
        const text = await navigator.clipboard.readText();
        keyInput.value = text;
        keyInput.focus();
      } catch (e) {
        keyInput.focus();
      }
    });
  }

  // Copy fingerprint
  if (copyFpBtn) {
    copyFpBtn.addEventListener("click", () => {
      const fp = fpForm?.textContent || "";
      navigator.clipboard.writeText(fp).then(() => {
        copyFpBtn.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg>';
        setTimeout(() => {
          copyFpBtn.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
        }, 1500);
      });
    });
  }

  activateBtn.addEventListener("click", async () => {
    const key = keyInput.value.trim();
    if (!key) { showMsg("Enter your activation key", "error"); return; }

    let licenseJson = key;
    if (!key.startsWith("{")) {
      showMsg("Validating key...", "info");
      try {
        const result = await tauri.core.invoke("install_license", { licenseJson: key });
        if (result.active) {
          showMsg("License activated successfully!", "success");
          keyInput.value = "";
          refreshLicense();
          return;
        }
        showMsg(result.error || "Invalid activation key", "error");
        return;
      } catch (e) {}
    }

    showMsg("Installing license...", "info");
    try {
      const result = await tauri.core.invoke("install_license", { licenseJson });
      if (result.active) {
        showMsg("License activated successfully!", "success");
        keyInput.value = "";
        rawJson.value = "";
        refreshLicense();
      } else {
        showMsg(result.error || "Activation failed", "error");
      }
    } catch (e) {
      showMsg(`Error: ${e}`, "error");
    }
  });

  rawActivateBtn.addEventListener("click", async () => {
    const json = rawJson.value.trim();
    if (!json) { showMsg("Paste license JSON first", "error"); return; }
    showMsg("Installing license...", "info");
    try {
      const result = await tauri.core.invoke("install_license", { licenseJson: json });
      if (result.active) {
        showMsg("License activated successfully!", "success");
        rawJson.value = "";
        refreshLicense();
      } else {
        showMsg(result.error || "Activation failed", "error");
      }
    } catch (e) {
      showMsg(`Error: ${e}`, "error");
    }
  });

  deactivateBtn.addEventListener("click", async () => {
    if (!confirm("Deactivate this license?")) return;
    try {
      await tauri.core.invoke("deactivate_license");
      refreshLicense();
    } catch (e) {
      showMsg(`Error: ${e}`, "error");
    }
  });

  keyInput.addEventListener("keydown", (e) => { if (e.key === "Enter") activateBtn.click(); });

  refreshLicense();
}

function setupDemoLicense() {
  document.getElementById("auth-active-card").style.display = "none";
  document.getElementById("auth-form-card").style.display = "block";
  document.getElementById("auth-fingerprint-form").textContent = "(demo mode)";
  document.getElementById("auth-activate-btn").addEventListener("click", () => {
    const msg = document.getElementById("auth-msg");
    msg.textContent = "(demo mode — license requires Tauri)";
    msg.className = "auth-msg info";
  });
  document.getElementById("auth-paste-btn")?.addEventListener("click", () => {});
  document.getElementById("auth-copy-fp")?.addEventListener("click", () => {});
}

// Scheduled Tasks
function setupScheduledTasks(tauri) {
  const list = document.getElementById("task-list");
  const createBtn = document.getElementById("task-create-btn");

  function renderTasks(tasks) {
    list.innerHTML = '<div class="task-row"><span>Name</span><span>Kind</span><span>Schedule</span><span>On</span><span></span></div>';
    if (!tasks.length) {
      list.insertAdjacentHTML("beforeend", '<div class="empty-state" style="padding:20px">No scheduled tasks — create one above</div>');
      return;
    }
    for (const t of tasks) {
      const row = document.createElement("div");
      row.className = "task-row";
      let schedule;
      if (t.cron_expr) {
        schedule = `<span class="task-schedule">${escapeHtml(t.cron_expr)}</span>`;
      } else {
        const secs = t.interval_secs;
        schedule = secs >= 3600 ? `every ${(secs/3600).toFixed(1)}h` : secs >= 60 ? `every ${(secs/60).toFixed(0)}m` : `every ${secs}s`;
      }
      row.innerHTML = `
        <span>${escapeHtml(t.name)}</span>
        <span>${escapeHtml(t.kind)}</span>
        <span>${schedule}</span>
        <input type="checkbox" class="task-enabled" data-id="${t.id}" ${t.enabled ? "checked" : ""} />
        <span>
          <button class="task-btn" data-action="run" data-id="${t.id}">Run</button>
          <button class="task-btn danger" data-action="delete" data-id="${t.id}">Del</button>
        </span>`;
      list.appendChild(row);
    }

    list.querySelectorAll(".task-enabled").forEach((cb) => {
      cb.addEventListener("change", async () => {
        try { await tauri.core.invoke("toggle_scheduled_task", { id: cb.dataset.id, enabled: cb.checked }); } catch (e) {}
      });
    });

    list.querySelectorAll(".task-btn").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const id = btn.dataset.id;
        try {
          if (btn.dataset.action === "run") {
            btn.textContent = "...";
            const msg = await tauri.core.invoke("run_task_now", { id });
            btn.textContent = msg.substring(0, 8);
            setTimeout(() => { btn.textContent = "Run"; }, 2000);
          } else if (btn.dataset.action === "delete") {
            await tauri.core.invoke("delete_scheduled_task", { id });
            refresh();
          }
        } catch (e) {}
      });
    });
  }

  async function refresh() {
    try {
      const tasks = await tauri.core.invoke("list_scheduled_tasks");
      renderTasks(tasks);
      document.getElementById("stat-tasks").textContent = tasks.filter(t => t.enabled).length;
    } catch (e) {}
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
    } catch (e) {}
  });

  refresh();
}

function setupDemoScheduledTasks() {
  document.getElementById("task-create-btn").addEventListener("click", () => {
    document.getElementById("task-list").innerHTML = '<div class="empty-state">(demo mode — requires Tauri)</div>';
  });
}

init();
