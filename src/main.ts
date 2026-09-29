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
}

const list = document.querySelector<HTMLElement>("#app-list")!;
const checkButton = document.querySelector<HTMLButtonElement>("#check-all")!;
const iconFor: Record<AppId, { path: string; className: string; alt: string }> = {
  "codex-desktop": { path: codexIcon, className: "app-icon--codex", alt: "" },
  spotifast: { path: spotifastIcon, className: "app-icon--spotifast", alt: "" },
};

const htmlEscapes: Record<string, string> = {
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
};

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => htmlEscapes[character]);
}

function formatVersion(app: AppStatus): string {
  const installed = app.installed ? escapeHtml(app.installed) : "—";
  if (app.updateAvailable && app.latest) {
    return `${installed}<span class="version-arrow" aria-hidden="true">→</span>${escapeHtml(app.latest)}`;
  }
  return installed;
}

function actionFor(app: AppStatus, busy: boolean): string {
  if (app.state === "available") {
    return `<button class="action-button" data-action="update" data-app="${app.id}" ${busy ? "disabled" : ""}>Actualizar</button>`;
  }
  if (app.state === "error") {
    return `<button class="action-button" data-action="retry" data-app="${app.id}" ${busy ? "disabled" : ""}>Reintentar</button>`;
  }
  if (app.state === "updating") return `<button class="action-button" disabled>Actualizando…</button>`;
  if (app.state === "checking" || app.state === "unchecked") return `<span class="current-status">${app.state === "checking" ? "Comprobando…" : "Sin comprobar"}</span>`;
  return `<span class="current-status"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12 4 4 10-10"/></svg>Al día</span>`;
}

function render(snapshot: AppSnapshot): void {
  checkButton.disabled = snapshot.busy;
  checkButton.textContent = snapshot.busy ? "Comprobando…" : "Comprobar";
  list.innerHTML = snapshot.apps.map((app) => {
    const icon = iconFor[app.id];
    const message = app.state === "checking" ? "Comprobando…"
      : app.state === "updating" ? "Instalando actualización…"
      : app.state === "error" ? (app.error ?? "No se pudo comprobar la versión")
      : app.notice ?? "";
    const showVersion = app.state !== "checking" && app.state !== "updating" && app.state !== "unchecked";
    return `<article class="app-row" aria-label="${escapeHtml(app.name)}">
      <span class="app-icon ${icon.className}" aria-hidden="true"><img src="${icon.path}" alt="${icon.alt}" /></span>
      <div class="app-copy"><div class="app-name">${escapeHtml(app.name)}</div><div class="version-line">${showVersion ? formatVersion(app) : "&nbsp;"}</div></div>
      ${actionFor(app, snapshot.busy)}
      ${message ? `<p class="row-message ${app.state === "error" ? "row-message--error" : ""}" role="status">${escapeHtml(message)}</p>` : ""}
    </article>`;
  }).join("");
}

async function checkAll(): Promise<void> {
  await invoke("check_all");
}

checkButton.addEventListener("click", () => { void checkAll().catch(() => {}); });
list.addEventListener("click", (event) => {
  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-action]");
  if (!target || target.disabled) return;
  const id = target.dataset.app as AppId;
  if (target.dataset.action === "update") void invoke("update_app", { id }).catch(() => {});
  if (target.dataset.action === "retry") void checkAll().catch(() => {});
});

void listen<AppSnapshot>("state-changed", ({ payload }) => render(payload)).catch(() => {});
void invoke<AppSnapshot>("get_state").then(render).catch(() => {
  list.innerHTML = '<p class="loading" role="status">No se pudo cargar el estado. Usa «Comprobar» para reintentar.</p>';
});
