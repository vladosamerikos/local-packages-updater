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
  busy: boolean;
  checkingAll: boolean;
}
const list = document.querySelector<HTMLElement>("#app-list")!;
const checkButton = document.querySelector<HTMLButtonElement>("#check-all")!;
const feedback = document.querySelector<HTMLElement>("#feedback")!;
const defaultFeedback = feedback.innerHTML;
let feedbackTimer: number | undefined;
let pendingRequest = false;
let currentSnapshot: AppSnapshot | undefined;
const icons = {
  refresh: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5"/><path d="M6 7a7 7 0 0 1 12-1l2 6M4 12l2 6a7 7 0 0 0 12-1"/></svg>',
  check: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12 4 4 10-10"/></svg>',
  warning: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m12 3 10 18H2L12 3Z"/><path d="M12 9v4m0 4h.01"/></svg>',
  download: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m-4-4 4 4 4-4M5 17v4h14v-4"/></svg>',
  clock: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="8"/><path d="M12 7v5l3 2"/></svg>',
  spinner: '<span class="spinner" aria-hidden="true"></span>',
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
  const installed = `<span class="version-number">${escapeHtml(app.installed)}</span>`;
  if (app.state === "available" && app.latest) {
    return `${installed}<span class="version-arrow" aria-label="actualización a">→</span><span class="version-new">${escapeHtml(app.latest)}</span>`;
  }
  return `Versión ${installed}`;
}
function statusFor(app: AppStatus): string {
  const statuses: Record<AppState, { icon: string; text: string }> = {
    unchecked: { icon: icons.clock, text: "Sin verificar" },
    checking: { icon: icons.spinner, text: "Verificando…" },
    updating: { icon: icons.spinner, text: "Instalando…" },
    current: { icon: icons.check, text: "Al día" },
    available: { icon: icons.download, text: "Nueva versión disponible" },
    error: { icon: icons.warning, text: "Requiere atención" },
  };
  const status = statuses[app.state];
  return `<span class="app-status app-status--${app.state}">${status.icon}${status.text}</span>`;
}
function readableError(error: unknown): string {
  const message = typeof error === "string" ? error.trim() : "";
  // Old persisted errors can contain shell paths and line numbers.
  if (!message || /\/usr\/|\.sh:|^curl:|^Error:/.test(message)) {
    return "No se pudo consultar la versión. Vuelve a verificar.";
  }
  return message.slice(0, 180);
}
function render(snapshot: AppSnapshot): void {
  currentSnapshot = snapshot;
  const busy = snapshot.busy || pendingRequest;
  checkButton.disabled = busy;
  checkButton.setAttribute("aria-busy", String(snapshot.checkingAll));
  checkButton.innerHTML = `${snapshot.checkingAll ? icons.spinner : icons.refresh}<span>Verificar todas</span>`;
  const focused = document.activeElement as HTMLButtonElement | null;
  const focusApp = focused?.dataset.app;
  const focusAction = focused?.dataset.action;
  list.innerHTML = snapshot.apps.map((app) => {
    const icon = iconFor[app.id];
    const active = app.state === "checking" || app.state === "updating";
    const message = app.state === "error" ? readableError(app.error) : app.notice;
    const tooltip = app.checkedAt
      ? `Última verificación: ${new Date(app.checkedAt * 1000).toLocaleString("es-ES")}`
      : "Verificar la versión de esta aplicación";
    return `<article class="app-row ${message ? "app-row--message" : ""}" aria-label="${escapeHtml(app.name)}" aria-busy="${active}">
      <span class="app-icon ${icon.className}" aria-hidden="true"><img src="${icon.path}" alt="" /></span>
      <div class="app-copy">
        <h2>${escapeHtml(app.name)}</h2>
        <p class="version-line">${versionFor(app)}</p>
        ${statusFor(app)}
      </div>
      <div class="row-actions">
        <button class="verify-button" type="button" data-action="check" data-app="${app.id}" aria-label="Verificar ${escapeHtml(app.name)}" title="${escapeHtml(tooltip)}" ${busy ? "disabled" : ""}>${icons.refresh}<span>Verificar</span></button>
        ${app.state === "available" ? `<button class="update-button" type="button" data-action="update" data-app="${app.id}" aria-label="Actualizar ${escapeHtml(app.name)}" ${busy ? "disabled" : ""}>${icons.download}<span>Actualizar</span></button>` : ""}
      </div>
      ${message ? `<p class="row-message ${app.state === "error" ? "row-message--error" : ""}">${escapeHtml(message)}</p>` : ""}
    </article>`;
  }).join("");
  if (focusApp && focusAction) {
    list.querySelector<HTMLButtonElement>(`button[data-app="${focusApp}"][data-action="${focusAction}"]:not(:disabled)`)?.focus({ preventScroll: true });
  }
}
function showFeedback(error: unknown): void {
  window.clearTimeout(feedbackTimer);
  feedback.classList.add("app-footer--error");
  feedback.innerHTML = `${icons.warning}<span>${escapeHtml(readableError(error))}</span>`;
  feedbackTimer = window.setTimeout(() => {
    feedback.classList.remove("app-footer--error");
    feedback.innerHTML = defaultFeedback;
  }, 7000);
}
async function request(command: "check_all" | "check_app" | "update_app", id?: AppId): Promise<void> {
  if (pendingRequest || currentSnapshot?.busy) return;
  pendingRequest = true;
  if (currentSnapshot) render(currentSnapshot);
  try {
    await invoke(command, id ? { id } : undefined);
  } catch (error) {
    showFeedback(error);
  } finally {
    pendingRequest = false;
    // Refresh after completion even if an event was missed while opening the window.
    await invoke<AppSnapshot>("get_state").then(render).catch(showFeedback);
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
    render(await invoke<AppSnapshot>("get_state"));
  } catch (error) {
    checkButton.disabled = false;
    list.innerHTML = '<p class="loading">No se pudo cargar el estado. Pulsa «Verificar todas» para reintentar.</p>';
    showFeedback(error);
  }
}
void initialize();
