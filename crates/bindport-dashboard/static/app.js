const content = document.getElementById("content");
const generatedAt = document.getElementById("generated-at");
const actionStatus = document.getElementById("action-status");
const serviceSearch = document.getElementById("service-search");
const stateFilterButtons = Array.from(document.querySelectorAll("[data-state-filter]"));
const authPanel = document.getElementById("auth-panel");
const authForm = document.getElementById("auth-form");
const authToken = document.getElementById("auth-token");
const tokenLogout = document.getElementById("token-logout");
const REFRESH_INTERVAL_MS = 5000;
const TOKEN_STORAGE_KEY = "bindport.dashboard.token";
const CLEAN_ACTION_HEADER = "X-BindPort-Dashboard-Action";
const HTML_ESCAPES = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  "\"": "&quot;",
  "'": "&#039;"
};
let lastSnapshot = null;
let lastRefreshAt = null;
let searchQuery = "";
let activeStateFilter = "all";
let dashboardToken = sessionStorage.getItem(TOKEN_STORAGE_KEY) || "";
let refreshTimer = null;
const expandedServiceKeys = new Set();
const groups = [
  { key: "active", label: "Active" },
  { key: "stopped", label: "Stopped" },
  { key: "stale", label: "Stale" },
  { key: "conflict", label: "Conflict" },
  { key: "other", label: "Other" }
];
const ICONS = {
  alert: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 9v4"/><path d="M12 17h.01"/><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z"/></svg>',
  check: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6 9 17l-5-5"/></svg>',
  chevron: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6"/></svg>',
  copy: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>',
  external: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/></svg>',
  lock: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>',
  trash: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/></svg>'
};

tokenLogout.innerHTML = ICONS.lock;

function text(value) {
  return value === null || value === undefined || value === "" ? "-" : String(value);
}

function escapeHtml(value) {
  return text(value).replace(/[&<>"']/g, (character) => HTML_ESCAPES[character]);
}

function serviceUrl(service) {
  return service.route_url || service.url || "";
}

function safeLink(value) {
  if (!value) return "";
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : "";
  } catch {
    return "";
  }
}

function stateKey(service) {
  const state = text(service.state).toLowerCase();
  return ["active", "stopped", "stale", "conflict"].includes(state) ? state : "other";
}

function stateLabel(state) {
  return groups.find((group) => group.key === state)?.label.toLowerCase() || state;
}

function stateCount(state) {
  if (!lastSnapshot) return 0;
  return (lastSnapshot.services || []).filter((service) => stateKey(service) === state).length;
}

function groupServices(services) {
  const grouped = Object.fromEntries(groups.map((group) => [group.key, []]));
  for (const service of services) {
    grouped[stateKey(service)].push(service);
  }
  return grouped;
}

function refreshSeconds() {
  return Math.round(REFRESH_INTERVAL_MS / 1000);
}

function formatTime(date) {
  return date.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  });
}

function setRefreshMeta(message, failed = false) {
  generatedAt.className = failed ? "meta meta-error" : "meta";
  generatedAt.textContent = message;
}

function setActionStatus(message, failed = false) {
  actionStatus.className = failed ? "action-status action-status-error" : "action-status";
  actionStatus.textContent = message;
  actionStatus.hidden = !message;
}

function setDashboardToken(token) {
  dashboardToken = token;
  if (dashboardToken) {
    sessionStorage.setItem(TOKEN_STORAGE_KEY, dashboardToken);
  } else {
    sessionStorage.removeItem(TOKEN_STORAGE_KEY);
  }
  updateTokenLogoutVisibility();
}

function updateTokenLogoutVisibility() {
  tokenLogout.hidden = !dashboardToken;
}

updateTokenLogoutVisibility();

function renderRefreshMeta(snapshot) {
  const parts = [];
  if (lastRefreshAt) {
    parts.push(`Updated ${formatTime(lastRefreshAt)}`);
  }
  if (snapshot.generated_at) {
    parts.push(`registry ${snapshot.generated_at}`);
  }
  parts.push(`refreshes every ${refreshSeconds()}s`);
  setRefreshMeta(parts.join(" - "));
}

function serviceSearchText(service) {
  return [
    service.state,
    service.project,
    service.service,
    service.port,
    serviceUrl(service),
    service.worktree_path,
    service.branch_label,
    service.branch,
    service.pid,
    service.command,
    service.cwd,
    service.health,
    proxyStatus(service)
  ].map(text).join(" ").toLowerCase();
}

function serviceKey(service) {
  return [
    service.project,
    service.service,
    serviceUrl(service),
    service.worktree_path,
    service.branch_label || service.branch,
    service.pid,
    service.command,
    service.cwd
  ].map(text).join("\u001f");
}

function detailIdForKey(key) {
  let hash = 0;
  for (let index = 0; index < key.length; index += 1) {
    hash = (hash * 31 + key.charCodeAt(index)) >>> 0;
  }
  return `details-${hash.toString(36)}`;
}

function matchesFilters(service) {
  if (activeStateFilter !== "all" && stateKey(service) !== activeStateFilter) {
    return false;
  }
  return !searchQuery || serviceSearchText(service).includes(searchQuery);
}

function filteredServices(services) {
  return services.filter(matchesFilters);
}

function updateFilterButtons() {
  for (const button of stateFilterButtons) {
    button.setAttribute(
      "aria-pressed",
      button.dataset.stateFilter === activeStateFilter ? "true" : "false"
    );
  }
}

function renderLastSnapshot() {
  if (lastSnapshot) render(lastSnapshot);
}

function renderSummary(grouped) {
  return `<section class="summary" aria-label="Service summary">
    ${groups.map((group) => `
      <div class="summary-item">
        <span class="summary-label">${group.label}</span>
        <span class="summary-value state-${group.key}">${grouped[group.key].length}</span>
      </div>
    `).join("")}
  </section>`;
}

function renderUrl(service) {
  const url = serviceUrl(service);
  const link = safeLink(url);
  if (!url) return "-";
  const display = link
    ? `<a class="url-text" href="${escapeHtml(link)}">${escapeHtml(url)}</a>`
    : `<span class="url-text">${escapeHtml(url)}</span>`;
  const open = link
    ? `<a class="icon-button action-link" href="${escapeHtml(link)}" target="_blank" rel="noreferrer noopener" aria-label="Open URL" title="Open URL">${ICONS.external}</a>`
    : "";
  return `<div class="url-cell">
    ${display}
    <span class="actions">
      ${open}
      <button class="icon-button action-button" type="button" data-copy-url="${escapeHtml(url)}" aria-label="Copy URL" title="Copy URL">${ICONS.copy}</button>
    </span>
  </div>`;
}

function renderCopyableValue(value, label) {
  if (!value) return "-";
  return `<div class="copyable-cell">
    <span class="copyable-text">${escapeHtml(value)}</span>
    <button class="icon-button action-button" type="button" data-copy-url="${escapeHtml(value)}" aria-label="Copy ${label}" title="Copy ${label}">${ICONS.copy}</button>
  </div>`;
}

function proxyStatus(service) {
  if (!service.proxy) return "Not rendered";
  const adapter = service.proxy.adapter || "proxy";
  const state = service.proxy.rendered ? "rendered" : "pending";
  const target = service.proxy.target ? ` to ${service.proxy.target}` : "";
  return `${adapter} ${state}${target}`;
}

function renderDetailToggle(detailsId, key, expanded) {
  return `<button class="icon-button detail-toggle" type="button" data-details-id="${detailsId}" data-service-key="${escapeHtml(key)}" aria-expanded="${expanded}" aria-label="${expanded ? "Hide details" : "Show details"}" title="${expanded ? "Hide details" : "Show details"}">${ICONS.chevron}</button>`;
}

function renderServiceDetails(service) {
  return `<dl class="detail-grid">
    <div class="detail-item">
      <dt>State</dt>
      <dd class="state-${stateKey(service)}">${escapeHtml(service.state)}</dd>
    </div>
    <div class="detail-item">
      <dt>PID</dt>
      <dd>${escapeHtml(service.pid)}</dd>
    </div>
    <div class="detail-item">
      <dt>Port</dt>
      <dd>${escapeHtml(service.port)}</dd>
    </div>
    <div class="detail-item">
      <dt>Health</dt>
      <dd>${escapeHtml(service.health)}</dd>
    </div>
    <div class="detail-item">
      <dt>Proxy</dt>
      <dd>${escapeHtml(proxyStatus(service))}</dd>
    </div>
    <div class="detail-item">
      <dt>CWD</dt>
      <dd>${escapeHtml(service.cwd)}</dd>
    </div>
    <div class="detail-item command-detail">
      <dt>Command</dt>
      <dd><code>${escapeHtml(service.command)}</code></dd>
    </div>
  </dl>`;
}

function renderServiceRow(service) {
  const key = serviceKey(service);
  const detailsId = detailIdForKey(key);
  const expanded = expandedServiceKeys.has(key);
  return `<tr class="service-row">
    <td class="detail-cell" data-label="Details">${renderDetailToggle(detailsId, key, expanded)}</td>
    <td data-label="Project">${escapeHtml(service.project)}</td>
    <td data-label="Service">${escapeHtml(service.service)}</td>
    <td data-label="URL">${renderUrl(service)}</td>
    <td data-label="Branch">${renderCopyableValue(service.branch_label || service.branch, "branch")}</td>
    <td data-label="Root">${renderCopyableValue(service.worktree_path, "root")}</td>
  </tr>
  <tr id="${detailsId}" class="detail-row"${expanded ? "" : " hidden"}>
    <td class="detail-panel-cell" colspan="6">${renderServiceDetails(service)}</td>
  </tr>`;
}

function renderGroup(group, services) {
  if (services.length === 0) return "";
  const cleanup = ["stopped", "stale"].includes(group.key)
    ? `<button class="icon-button group-action-button" type="button" data-clean-state="${group.key}" aria-label="Remove ${group.label} entries" title="Remove ${group.label} entries">${ICONS.trash}</button>`
    : "";
  return `<section class="service-group" aria-labelledby="group-${group.key}">
    <div class="group-heading">
      <h2 id="group-${group.key}">${group.label}</h2>
      <div class="group-actions">
        <span class="group-count">${services.length}</span>
        ${cleanup}
      </div>
    </div>
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th><span class="visually-hidden">Details</span></th>
            <th>Project</th>
            <th>Service</th>
            <th>URL</th>
            <th>Branch</th>
            <th>Root</th>
          </tr>
        </thead>
        <tbody>${services.map(renderServiceRow).join("")}</tbody>
      </table>
    </div>
  </section>`;
}

function render(snapshot) {
  authPanel.hidden = true;
  updateTokenLogoutVisibility();
  renderRefreshMeta(snapshot);
  const services = snapshot.services || [];
  syncExpandedServiceKeys(services);
  if (services.length === 0) {
    content.className = "empty";
    content.textContent = "No BindPort runs recorded yet.";
    return;
  }

  const visibleServices = filteredServices(services);
  if (visibleServices.length === 0) {
    content.className = "empty";
    content.textContent = "No services match the current filters.";
    return;
  }

  const grouped = groupServices(visibleServices);
  content.className = "";
  content.innerHTML = `
    ${renderSummary(grouped)}
    ${groups.map((group) => renderGroup(group, grouped[group.key])).join("")}
  `;
}

function syncExpandedServiceKeys(services) {
  const currentKeys = new Set(services.map(serviceKey));
  for (const key of expandedServiceKeys) {
    if (!currentKeys.has(key)) {
      expandedServiceKeys.delete(key);
    }
  }
}

async function copyText(value) {
  if (navigator.clipboard && window.isSecureContext) {
    await navigator.clipboard.writeText(value);
    return;
  }

  const input = document.createElement("textarea");
  input.value = value;
  input.setAttribute("readonly", "");
  input.style.position = "fixed";
  input.style.left = "-9999px";
  document.body.appendChild(input);
  input.select();
  try {
    if (!document.execCommand("copy")) {
      throw new Error("copy failed");
    }
  } finally {
    input.remove();
  }
}

function resetCopyButton(button, previous) {
  window.setTimeout(() => {
    button.disabled = false;
    button.innerHTML = previous.html;
    restoreAttribute(button, "aria-label", previous.label);
    restoreAttribute(button, "title", previous.title);
  }, 1200);
}

function restoreAttribute(element, name, value) {
  if (value === null) {
    element.removeAttribute(name);
    return;
  }
  element.setAttribute(name, value);
}

content.addEventListener("click", async (event) => {
  const toggle = event.target.closest("[data-details-id]");
  if (toggle) {
    const row = document.getElementById(toggle.dataset.detailsId);
    const key = toggle.dataset.serviceKey;
    const expanded = toggle.getAttribute("aria-expanded") === "true";
    if (key) {
      if (expanded) {
        expandedServiceKeys.delete(key);
      } else {
        expandedServiceKeys.add(key);
      }
    }
    toggle.setAttribute("aria-expanded", String(!expanded));
    toggle.setAttribute("aria-label", expanded ? "Show details" : "Hide details");
    toggle.setAttribute("title", expanded ? "Show details" : "Hide details");
    if (row) row.hidden = expanded;
    return;
  }

  const cleanButton = event.target.closest("[data-clean-state]");
  if (cleanButton) {
    await cleanRegistryEntries(cleanButton.dataset.cleanState, cleanButton);
    return;
  }

  const button = event.target.closest("[data-copy-url]");
  if (!button) return;

  const previous = {
    html: button.innerHTML,
    label: button.getAttribute("aria-label"),
    title: button.getAttribute("title")
  };
  button.disabled = true;
  try {
    await copyText(button.getAttribute("data-copy-url"));
    button.innerHTML = ICONS.check;
    button.setAttribute("aria-label", "Copied URL");
    button.setAttribute("title", "Copied URL");
  } catch {
    button.innerHTML = ICONS.alert;
    button.setAttribute("aria-label", "Copy failed");
    button.setAttribute("title", "Copy failed");
  }
  resetCopyButton(button, previous);
});

async function cleanRegistryEntries(state, button) {
  const count = stateCount(state);
  const label = stateLabel(state);
  if (count === 0 || !window.confirm(`Remove ${count} ${label} registry entries?`)) {
    return;
  }

  const previous = {
    html: button.innerHTML,
    label: button.getAttribute("aria-label"),
    title: button.getAttribute("title")
  };
  button.disabled = true;
  setActionStatus("");

  try {
    const headers = {
      [CLEAN_ACTION_HEADER]: "clean"
    };
    if (dashboardToken) {
      headers.Authorization = `Bearer ${dashboardToken}`;
    }
    const response = await fetch(`/api/clean/${state}`, {
      cache: "no-store",
      headers,
      method: "POST"
    });
    if (!response.ok) {
      const error = new Error(`clean failed with status ${response.status}`);
      error.status = response.status;
      throw error;
    }

    const result = await response.json();
    button.innerHTML = ICONS.check;
    setActionStatus(`Removed ${result.leases} registry entries`);
    await refreshStatus();
  } catch (error) {
    if (error.status === 401) {
      authPanel.hidden = false;
    }
    button.innerHTML = ICONS.alert;
    setActionStatus(error.message, true);
  } finally {
    resetCopyButton(button, previous);
  }
}

serviceSearch.addEventListener("input", () => {
  searchQuery = serviceSearch.value.trim().toLowerCase();
  renderLastSnapshot();
});

for (const button of stateFilterButtons) {
  button.addEventListener("click", () => {
    activeStateFilter = button.dataset.stateFilter;
    updateFilterButtons();
    renderLastSnapshot();
  });
}

authForm.addEventListener("submit", (event) => {
  event.preventDefault();
  setDashboardToken(authToken.value.trim());
  refreshStatus();
});

tokenLogout.addEventListener("click", () => {
  setDashboardToken("");
  authToken.value = "";
  expandedServiceKeys.clear();
  lastSnapshot = null;
  content.className = "empty";
  content.textContent = "Checking dashboard access...";
  refreshStatus();
});

function renderRefreshError(error) {
  if (error.status === 401) {
    authPanel.hidden = false;
    updateTokenLogoutVisibility();
    setRefreshMeta("Dashboard token required", true);
    content.className = "empty";
    content.textContent = "Enter the dashboard token to view registry data.";
    return;
  }

  if (lastSnapshot && lastRefreshAt) {
    setRefreshMeta(
      `Refresh failed: ${error.message} - last updated ${formatTime(lastRefreshAt)}`,
      true
    );
    return;
  }

  setRefreshMeta(`Refresh failed: ${error.message}`, true);
  content.className = "error";
  content.textContent = `Dashboard status unavailable: ${error.message}`;
}

async function refreshStatus() {
  if (refreshTimer) {
    window.clearTimeout(refreshTimer);
    refreshTimer = null;
  }

  try {
    const headers = {};
    if (dashboardToken) {
      headers.Authorization = `Bearer ${dashboardToken}`;
    }
    const response = await fetch("/api/status", {
      cache: "no-store",
      headers
    });
    if (!response.ok) {
      const error = new Error(`status ${response.status}`);
      error.status = response.status;
      throw error;
    }

    const snapshot = await response.json();
    lastSnapshot = snapshot;
    lastRefreshAt = new Date();
    render(snapshot);
  } catch (error) {
    renderRefreshError(error);
  } finally {
    refreshTimer = window.setTimeout(refreshStatus, REFRESH_INTERVAL_MS);
  }
}

refreshStatus();
