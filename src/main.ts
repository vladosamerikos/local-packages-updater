import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import codexIcon from "./assets/codex-desktop.png";
import spotifastIcon from "./assets/spotifast.svg";
import "./styles.css";

type AppId = "codex-desktop" | "spotifast";
type AppState = "unchecked" | "checking" | "updating" | "current" | "available" | "error";
interface AppStatus {
  id: AppId;
  name: string;
  installed: string | null;
  latest: string | null;
  updateAvailable: boolean;
  state: AppState;
  error: string | null;
  notice: string | null;
  checkedAt: number | null;
}
interface AppSnapshot {
  apps: AppStatus[];
  activeApps: AppId[];
  checkingAll: boolean;
  revision: number;
  instanceId: string;
  protocolVersion: number;
}
const list = document.querySelector<HTMLElement>("#app-list")!;
const checkButton = document.querySelector<HTMLButtonElement>("#check-all")!;
const feedback = document.querySelector<HTMLElement>("#feedback")!;
let feedbackTimer: number | undefined;
let pendingAll = false;
const pendingApps = new Map<AppId, "check" | "update">();
let currentSnapshot: AppSnapshot | undefined;
const icons = {
  refresh: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 11a9 9 0 0 1 15.4-6.4L21 7"/><path d="M21 2v5h-5"/><path d="M21 13a9 9 0 0 1-15.4 6.4L3 17"/><path d="M3 22v-5h5"/></svg>',
  check: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12 4 4 10-10"/></svg>',
  warning: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m12 3 10 18H2L12 3Z"/><path d="M12 9v4m0 4h.01"/></svg>',
  download: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m-4-4 4 4 4-4M5 17v4h14v-4"/></svg>',
  spinner: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="8" opacity=".2"/><path d="M12 4a8 8 0 0 1 8 8"/></svg>',
};
const iconFor: Record<AppId, { path: string; className: string }> = {
  "codex-desktop": { path: codexIcon, className: "app-icon--codex" },
  spotifast: { path: spotifastIcon, className: "app-icon--spotifast" },
};
const htmlEscapes: Record<string, string> = {
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
};
function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => htmlEscapes[character]);
}
function versionFor(app: AppStatus): string {
  if (!app.installed) return "Versión no disponible";
  const installed = '<span class="version-number">' + escapeHtml(app.installed) + "</span>";
  if (app.updateAvailable && app.latest && app.latest !== app.installed) {
    return installed + '<span class="version-arrow" aria-label="actualización a">→</span><span class="version-new">' + escapeHtml(app.latest) + "</span>";
  }
  return "Versión " + installed;
}
function statusFor(app: AppStatus, active: boolean): string {
  if (active) return "";
  const statuses = {
    unchecked: { icon: "", text: "Sin verificar" },
    current: { icon: icons.check, text: "Al día" },
    available: { icon: icons.download, text: "Disponible" },
    error: { icon: icons.warning, text: "Atención" },
    checking: { icon: "", text: "" },
    updating: { icon: "", text: "" },
  };
  const status = statuses[app.state];
  return '<span class="app-status app-status--' + app.state + '">' + status.icon + status.text + "</span>";
}
function readableError(error: unknown): string {
  const message = typeof error === "string" ? error.trim() : "";
  if (!message || /\/usr\/|\.sh:|^curl:|^Error:/.test(message)) {
    return "No se pudo consultar la versión. Vuelve a verificar.";
  }
  return message.slice(0, 180);
}
function render(snapshot: AppSnapshot): void {
  // Events and completed commands can arrive in a different order.
  if (snapshot.protocolVersion !== 1) { showFeedback("Protocolo del agente incompatible"); return; }
  if (currentSnapshot?.instanceId === snapshot.instanceId && snapshot.revision < currentSnapshot.revision) return;
  if (currentSnapshot && currentSnapshot.instanceId !== snapshot.instanceId) { pendingAll = false; pendingApps.clear(); }
  currentSnapshot = snapshot;
  const allActive = snapshot.checkingAll || pendingAll;
  checkButton.disabled = false;
  checkButton.setAttribute("aria-busy", String(allActive));
  const checkContents = icons.refresh + "<span>Verificar todas</span>";
  if (checkButton.innerHTML !== checkContents) checkButton.innerHTML = checkContents;
  const focused = document.activeElement as HTMLButtonElement | null;
  const focusApp = focused?.dataset.app;
  const focusAction = focused?.dataset.action;
  const rows = snapshot.apps.map((app) => {
    const icon = iconFor[app.id];
    const pending = pendingApps.get(app.id);
    const active = snapshot.activeApps.includes(app.id) || !!pending || app.state === "checking" || app.state === "updating";
    const updating = app.state === "updating" || pending === "update";
    const checking = active && !updating;
    const hasUpdate = app.updateAvailable || updating;
    const message = active ? null : app.state === "error" ? readableError(app.error) : app.notice;
    const tooltip = app.checkedAt
      ? "Última verificación: " + new Date(app.checkedAt * 1000).toLocaleString("es-ES")
      : "Verificar la versión de esta aplicación";
    const blocked = allActive && !active;
    const verifyDisabled = blocked || updating;
    return '<article class="app-row ' + (message ? "app-row--message" : "") + '" aria-label="' + escapeHtml(app.name) + '">' +
      '<span class="app-icon ' + icon.className + '" aria-hidden="true"><img src="' + icon.path + '" alt="" /></span>' +
      '<div class="app-copy"><h2>' + escapeHtml(app.name) + '</h2><div class="app-meta"><p class="version-line">' + versionFor(app) + "</p>" + statusFor(app, active) + "</div></div>" +
      '<div class="row-actions"><button class="verify-button ' + (hasUpdate ? "icon-button" : "") + '" type="button" data-action="check" data-app="' + app.id +
      '" aria-label="Verificar ' + escapeHtml(app.name) + '" aria-busy="' + checking + '" title="' + escapeHtml(tooltip) + '" ' + (verifyDisabled ? "disabled" : "") + ">" +
      icons.refresh + (hasUpdate ? "" : "<span>Verificar</span>") + "</button>" +
      (hasUpdate ? '<button class="update-button" type="button" data-action="update" data-app="' + app.id + '" aria-label="Actualizar ' +
        escapeHtml(app.name) + '" aria-busy="' + updating + '" ' + (allActive && !updating || checking ? "disabled" : "") + ">" +
        (updating ? icons.spinner : icons.download) + "<span>Actualizar</span></button>" : "") + "</div>" +
      (message ? '<p class="row-message ' + (app.state === "error" ? "row-message--error" : "") + '">' + escapeHtml(message) + "</p>" : "") + "</article>";
  });
  for (const row of rows) {
    const template = document.createElement("template");
    template.innerHTML = row;
    const next = template.content.firstElementChild as HTMLElement;
    const id = next.querySelector<HTMLButtonElement>("button[data-app]")?.dataset.app;
    const existing = id ? list.querySelector<HTMLElement>('button[data-app="' + id + '"]')?.closest<HTMLElement>(".app-row") : null;
    if (!existing) list.append(next);
    else if (existing.outerHTML !== next.outerHTML) existing.replaceWith(next);
  }
  if (focusApp && focusAction) {
    list.querySelector<HTMLButtonElement>('button[data-app="' + focusApp + '"][data-action="' + focusAction + '"]:not(:disabled)')?.focus({ preventScroll: true });
  }
}
function showFeedback(error: unknown): void {
  window.clearTimeout(feedbackTimer);
  feedback.hidden = false;
  feedback.innerHTML = icons.warning + "<span>" + escapeHtml(readableError(error)) + "</span>";
  feedbackTimer = window.setTimeout(() => { feedback.hidden = true; }, 7000);
}
async function request(command: "check_all" | "check_app" | "update_app", id?: AppId): Promise<void> {
  if (pendingAll || currentSnapshot?.checkingAll) return;
  if (id && (pendingApps.has(id) || currentSnapshot?.activeApps.includes(id))) return;
  if (command === "check_all") pendingAll = true;
  else if (id) pendingApps.set(id, command === "update_app" ? "update" : "check");
  if (currentSnapshot) render(currentSnapshot);
  try {
    await invoke<string>(command, id ? { id, requestId: crypto.randomUUID() } : { requestId: crypto.randomUUID() });
  } catch (error) {
    showFeedback(error);
  } finally {
    if (command === "check_all") pendingAll = false;
    else if (id) pendingApps.delete(id);
    if (currentSnapshot) render(currentSnapshot);
  }
}
checkButton.addEventListener("click", () => { void request("check_all"); });
list.addEventListener("click", (event) => {
  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-action]");
  if (!target || target.disabled) return;
  const id = target.dataset.app as AppId;
  void request(target.dataset.action === "update" ? "update_app" : "check_app", id);
});
async function initialize(): Promise<void> {
  try {
    await listen<AppSnapshot>("state-changed", ({ payload }) => render(payload));
    await listen<{ operationId: string; error: string }>("operation-finished", ({ payload }) => {
      if (payload.error) showFeedback(payload.error);
    });
    render(await invoke<AppSnapshot>("get_state"));
  } catch (error) {
    checkButton.disabled = false;
    list.innerHTML = '<p class="loading">No se pudo cargar el estado. Pulsa «Verificar todas» para reintentar.</p>';
    showFeedback(error);
  }
}
void initialize();
