import {
  Activity,
  BarChart3,
  Clock3,
  Download,
  FileInput,
  Gauge,
  History,
  Home,
  Languages,
  Layers3,
  Moon,
  Settings,
  Sun,
} from "lucide-react";
import { useCallback, useEffect, useState, type ComponentType } from "react";
import { useTranslation } from "react-i18next";
import type { SupportedLocale } from "./i18n";
import { quickJobStatus, quickRecover, type JobView, type PreparedQuickSim, type QuickResultView, type QuickSimRequest } from "./quick";
import { runtimeInstallLatest, runtimeStatus, type RuntimeView } from "./runtime";
import { ImportPage } from "./screens/ImportPage";
import { JobsPage } from "./screens/JobsPage";
import { QuickSimPage } from "./screens/QuickSimPage";
import { ResultsPage } from "./screens/ResultsPage";
import { SettingsPage } from "./screens/SettingsPage";
import { TopGearPage } from "./screens/TopGearPage";
import { topGearSessions, type TopGearSessionView } from "./topGear";

type Page = "home" | "import" | "quickSim" | "topGear" | "jobs" | "results" | "history" | "settings";

type NavItem = {
  id: Page;
  labelKey: `nav.${Page}`;
  icon: ComponentType<{ "aria-hidden"?: boolean; size?: number }>;
};

const navigation: NavItem[] = [
  { id: "home", labelKey: "nav.home", icon: Home },
  { id: "import", labelKey: "nav.import", icon: FileInput },
  { id: "quickSim", labelKey: "nav.quickSim", icon: Gauge },
  { id: "topGear", labelKey: "nav.topGear", icon: Layers3 },
  { id: "jobs", labelKey: "nav.jobs", icon: Activity },
  { id: "results", labelKey: "nav.results", icon: BarChart3 },
  { id: "history", labelKey: "nav.history", icon: History },
  { id: "settings", labelKey: "nav.settings", icon: Settings },
];

const RUNTIME_UPDATE_CHECK_DATE_KEY = "simshredder.runtimeUpdateCheckDate";

function localDateKey(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function App() {
  const { t, i18n } = useTranslation();
  const [page, setPage] = useState<Page>("home");
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  const [quickRequest, setQuickRequest] = useState<QuickSimRequest | null>(null);
  const [preview, setPreview] = useState<PreparedQuickSim | null>(null);
  const [job, setJob] = useState<JobView | null>(null);
  const [result, setResult] = useState<QuickResultView | null>(null);
  const [topGearSession, setTopGearSession] = useState<TopGearSessionView | null>(null);
  const [runtimeUpdate, setRuntimeUpdate] = useState<RuntimeView | null>(null);
  const [runtimeUpdateBusy, setRuntimeUpdateBusy] = useState(false);
  const [runtimeUpdateError, setRuntimeUpdateError] = useState<string | null>(null);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.lang = i18n.resolvedLanguage ?? "en";
  }, [i18n.resolvedLanguage, theme]);

  useEffect(() => {
    void quickRecover().then(async (jobs) => {
      const latest = jobs.at(-1);
      if (latest !== undefined) setJob(await quickJobStatus(latest));
    }).catch(() => undefined);
  }, []);

  useEffect(() => {
    void topGearSessions().then((sessions) => setTopGearSession(sessions.at(-1) ?? null)).catch(() => undefined);
  }, []);

  useEffect(() => {
    const today = localDateKey();
    try {
      if (window.localStorage.getItem(RUNTIME_UPDATE_CHECK_DATE_KEY) === today) return;
      window.localStorage.setItem(RUNTIME_UPDATE_CHECK_DATE_KEY, today);
    } catch {
      // A disabled WebView storage layer must not prevent the update check.
    }
    void runtimeStatus().then((runtime) => {
      if (runtime.state === "ready" && runtime.active && runtime.updateAvailable) {
        setRuntimeUpdate(runtime);
      }
    }).catch(() => undefined);
  }, []);

  const updateJob = useCallback((next: JobView) => setJob(next), []);

  const changeLocale = (locale: SupportedLocale) => {
    void i18n.changeLanguage(locale);
  };

  const installRuntimeUpdate = async () => {
    setRuntimeUpdateBusy(true);
    setRuntimeUpdateError(null);
    try {
      await runtimeInstallLatest();
      setRuntimeUpdate(null);
    } catch (reason) {
      setRuntimeUpdateError(String(reason));
    } finally {
      setRuntimeUpdateBusy(false);
    }
  };

  return (
    <>
    <div className="app-shell" aria-hidden={runtimeUpdate ? true : undefined} inert={Boolean(runtimeUpdate)}>
      <a className="skip-link" href="#main-content">
        {t("app.skipToContent")}
      </a>
      <aside className="sidebar">
        <div className="brand-mark" aria-label={t("app.name")}>
          <span className="brand-glyph" aria-hidden="true">S</span>
          <span>{t("app.name")}</span>
        </div>
        <nav aria-label={t("nav.label")}>
          <ul className="nav-list">
            {navigation.map(({ id, labelKey, icon: Icon }) => (
              <li key={id}>
                <button
                  className="nav-item"
                  aria-label={t(labelKey)}
                  aria-current={page === id ? "page" : undefined}
                  onClick={() => setPage(id)}
                  type="button"
                >
                  <Icon aria-hidden={true} size={19} />
                  <span>{t(labelKey)}</span>
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <div className="sidebar-footer">
          <p>{t("app.privacy")}</p>
        </div>
      </aside>

      <section className="workspace">
        <header className="topbar" role="banner">
          <div className="queue-state" aria-live="polite">
            <span className="status-dot" aria-hidden="true" />
            <span>{t("status.idle")}</span>
            <span className="muted">· {t("status.noActiveJobs")}</span>
          </div>
          <div className="topbar-actions">
            <label className="compact-control">
              <Languages aria-hidden="true" size={17} />
              <span className="sr-only">{t("locale.label")}</span>
              <select
                aria-label={t("locale.label")}
                value={(i18n.resolvedLanguage ?? "en").split("-")[0]}
                onChange={(event) => changeLocale(event.target.value as SupportedLocale)}
              >
                <option value="en">{t("locale.en")}</option>
                <option value="ko">{t("locale.ko")}</option>
              </select>
            </label>
            <button
              className="icon-button"
              type="button"
              aria-label={`${t("theme.label")}: ${t(`theme.${theme === "dark" ? "light" : "dark"}`)}`}
              onClick={() => setTheme((current) => (current === "dark" ? "light" : "dark"))}
            >
              {theme === "dark" ? <Sun aria-hidden="true" size={18} /> : <Moon aria-hidden="true" size={18} />}
            </button>
          </div>
        </header>

        <main id="main-content" tabIndex={-1}>
          {page === "home" ? (
            <HomePage navigate={setPage} />
          ) : page === "import" ? (
            <ImportPage onPrepared={(request, prepared) => { setQuickRequest(request); setPreview(prepared); setPage("quickSim"); }} />
          ) : page === "quickSim" ? (
            <QuickSimPage request={quickRequest} preview={preview} onChange={(request, prepared) => { setQuickRequest(request); setPreview(prepared); }} onStarted={(next) => { setJob(next); setPage("jobs"); }} onImport={() => setPage("import")} />
          ) : page === "topGear" ? (
            <TopGearPage quick={quickRequest} initialSession={topGearSession} onSession={setTopGearSession} onImport={() => setPage("import")} />
          ) : page === "jobs" ? (
            <JobsPage initialJob={job} onJob={updateJob} onResult={(next) => { setResult(next); setPage("results"); }} />
          ) : page === "results" ? (
            <ResultsPage result={result} />
          ) : page === "settings" ? (
            <SettingsPage />
          ) : (
            <PlaceholderPage page={page} />
          )}
        </main>
      </section>
    </div>
    {runtimeUpdate ? (
      <div className="modal-backdrop">
        <dialog className="update-dialog" open aria-labelledby="runtime-update-title" aria-describedby="runtime-update-description">
          <p className="eyebrow">{t("runtime.updatePromptEyebrow")}</p>
          <h2 id="runtime-update-title">{t("runtime.updatePromptTitle")}</h2>
          <p id="runtime-update-description">
            {t("runtime.updatePromptBody", {
              current: runtimeUpdate.active?.build,
              available: runtimeUpdate.availableBuild,
            })}
          </p>
          <p>{t("runtime.updatePromptQuestion")}</p>
          {runtimeUpdateError ? <p className="inline-error" role="alert">{t("runtime.updatePromptError", { error: runtimeUpdateError })}</p> : null}
          <div className="button-row">
            <button className="primary-button" disabled={runtimeUpdateBusy} type="button" onClick={() => void installRuntimeUpdate()}>
              <Download aria-hidden="true" size={18} />
              {runtimeUpdateBusy ? t("runtime.updatePromptInstalling") : t("runtime.updatePromptYes")}
            </button>
            <button className="secondary-button" disabled={runtimeUpdateBusy} type="button" autoFocus onClick={() => setRuntimeUpdate(null)}>
              {t("runtime.updatePromptLater")}
            </button>
          </div>
        </dialog>
      </div>
    ) : null}
    </>
  );
}

function HomePage({ navigate }: { navigate: (page: Page) => void }) {
  const { t } = useTranslation();
  return (
    <div className="page home-page">
      <section className="hero-panel">
        <div>
          <p className="eyebrow">{t("home.eyebrow")}</p>
          <h1>{t("home.title")}</h1>
          <p className="hero-copy">{t("home.description")}</p>
          <div className="button-row">
            <button className="primary-button" type="button" onClick={() => navigate("import")}>
              <FileInput aria-hidden="true" size={18} />
              {t("home.startImport")}
            </button>
            <button className="secondary-button" type="button" onClick={() => navigate("settings")}>
              <Download aria-hidden="true" size={18} />
              {t("home.openSettings")}
            </button>
          </div>
        </div>
        <div className="signal-art" aria-hidden="true">
          <span /><span /><span /><span /><span /><span />
        </div>
      </section>

      <section className="summary-grid" aria-label={t("support.label")}>
        <article className="card runtime-card">
          <div className="card-heading">
            <div className="card-icon"><Gauge aria-hidden="true" size={19} /></div>
            <h2>{t("runtime.title")}</h2>
          </div>
          <p className="status-warning"><span aria-hidden="true" />{t("runtime.missing")}</p>
          <p className="muted">{t("runtime.missingDetail")}</p>
          <button className="text-button" type="button" onClick={() => navigate("settings")}>
            {t("runtime.action")} <span aria-hidden="true">→</span>
          </button>
        </article>
        <article className="card contract-card">
          <div className="card-heading">
            <div className="card-icon"><Activity aria-hidden="true" size={19} /></div>
            <h2>{t("support.label")}</h2>
          </div>
          <p>{t("support.platform")}</p>
          <p className="muted">{t("support.game")}</p>
        </article>
      </section>

      <section className="recent-section">
        <div className="section-heading">
          <h2>{t("home.recentTitle")}</h2>
        </div>
        <div className="empty-state">
          <Clock3 aria-hidden="true" size={25} />
          <div>
            <h3>{t("home.recentEmptyTitle")}</h3>
            <p>{t("home.recentEmptyBody")}</p>
          </div>
        </div>
      </section>
    </div>
  );
}

function PlaceholderPage({ page }: { page: Exclude<Page, "home" | "import" | "quickSim" | "topGear" | "jobs" | "results" | "settings"> }) {
  const { t } = useTranslation();
  const navKey = `nav.${page}` as const;
  return (
    <div className="page placeholder-page">
      <p className="eyebrow">{t(navKey)}</p>
      <h1>{t("placeholder.title")}</h1>
      <p>{t("placeholder.body")}</p>
    </div>
  );
}
