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
import { quickDelete, quickJobs as loadQuickJobs, quickRecover, type JobView, type PreparedQuickSim, type QuickSimRequest } from "./quick";
import { sameRun, type RunReference } from "./runs";
import { formatRuntimeDataDate, runtimeCheckUpdates, runtimeInstallLatest, runtimeStatus, type RuntimeView } from "./runtime";
import { ImportPage } from "./screens/ImportPage";
import { HistoryPage } from "./screens/HistoryPage";
import { JobsPage } from "./screens/JobsPage";
import { QuickSimPage } from "./screens/QuickSimPage";
import { ResultsPage } from "./screens/ResultsPage";
import { SettingsPage } from "./screens/SettingsPage";
import { TopGearPage } from "./screens/TopGearPage";
import { topGearDelete, topGearSessions, type TopGearSessionView } from "./topGear";

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
  const [jobs, setJobs] = useState<JobView[]>([]);
  const [topGearSessionsState, setTopGearSessionsState] = useState<TopGearSessionView[]>([]);
  const [selectedRun, setSelectedRun] = useState<RunReference | null>(null);
  const [selectedResult, setSelectedResult] = useState<RunReference | null>(null);
  const [runtime, setRuntime] = useState<RuntimeView | null>(null);
  const [runtimeUpdate, setRuntimeUpdate] = useState<RuntimeView | null>(null);
  const [runtimeUpdateBusy, setRuntimeUpdateBusy] = useState(false);
  const [runtimeUpdateError, setRuntimeUpdateError] = useState<string | null>(null);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.lang = i18n.resolvedLanguage ?? "en";
  }, [i18n.resolvedLanguage, theme]);

  useEffect(() => {
    void quickRecover().then(loadQuickJobs).then(setJobs).catch(() => undefined);
  }, []);

  useEffect(() => {
    void topGearSessions().then(setTopGearSessionsState).catch(() => undefined);
  }, []);

  useEffect(() => {
    void runtimeStatus().then(setRuntime).catch(() => undefined);
  }, []);

  useEffect(() => {
    const today = localDateKey();
    try {
      if (window.localStorage.getItem(RUNTIME_UPDATE_CHECK_DATE_KEY) === today) return;
      window.localStorage.setItem(RUNTIME_UPDATE_CHECK_DATE_KEY, today);
    } catch {
      // A disabled WebView storage layer must not prevent the update check.
    }
    void runtimeCheckUpdates().then((nextRuntime) => {
      setRuntime(nextRuntime);
      if (nextRuntime.state === "ready" && nextRuntime.active && nextRuntime.updateAvailable) {
        setRuntimeUpdate(nextRuntime);
      }
    }).catch(() => undefined);
  }, []);

  const updateJob = useCallback((next: JobView) => setJobs((current) => [next, ...current.filter((job) => job.id !== next.id)]), []);
  const updateTopGearSession = useCallback((next: TopGearSessionView) => setTopGearSessionsState((current) => [next, ...current.filter((session) => session.id !== next.id)]), []);
  const openRun = useCallback((run: RunReference) => { setSelectedRun(run); setPage("jobs"); }, []);
  const openResult = useCallback((run: RunReference) => { setSelectedResult(run); setPage("results"); }, []);
  const deleteRun = useCallback(async (run: RunReference) => {
    if (run.kind === "quick") {
      await quickDelete(run.jobId);
      setJobs((current) => current.filter((job) => job.id !== run.jobId));
    } else {
      const session = topGearSessionsState.find((candidate) => candidate.id === run.sessionId);
      await topGearDelete(run.sessionId);
      const jobIds = new Set(session ? [session.lowJobId, session.highJobId, session.actionJobId].filter((id): id is number => id !== null) : []);
      setTopGearSessionsState((current) => current.filter((candidate) => candidate.id !== run.sessionId));
      setJobs((current) => current.filter((job) => !jobIds.has(job.id)));
    }
    setSelectedRun((current) => sameRun(current, run) ? null : current);
    setSelectedResult((current) => sameRun(current, run) ? null : current);
  }, [topGearSessionsState]);

  const changeLocale = (locale: SupportedLocale) => {
    void i18n.changeLanguage(locale);
  };

  const installRuntimeUpdate = async () => {
    setRuntimeUpdateBusy(true);
    setRuntimeUpdateError(null);
    try {
      setRuntime(await runtimeInstallLatest());
      setRuntimeUpdate(null);
    } catch (reason) {
      setRuntimeUpdateError(String(reason));
    } finally {
      setRuntimeUpdateBusy(false);
    }
  };
  const topGearJobIds = new Set(topGearSessionsState.flatMap((session) => [session.lowJobId, session.mediumJobId, session.highJobId, session.actionJobId].filter((id): id is number => id !== null)));
  const quickRuns = jobs.filter((job) => !topGearJobIds.has(job.id));
  const trackedSession = topGearSessionsState.find((session) => session.stage !== "complete");
  const trackedJob = quickRuns.find((candidate) => ["queued", "running"].includes(candidate.state));
  const trackedTopGear = trackedSession
    ? { page: "jobs" as const, label: t("status.gearOptimizer"), detail: `${t(`topGear.stage_${trackedSession.stage}`)} · ${t(`jobsPage.${trackedSession.currentJob.state}`)}`, active: ["queued", "running"].includes(trackedSession.currentJob.state) }
    : null;
  const trackedQuick = trackedJob
    ? { page: "jobs" as const, label: t("status.characterAnalysis"), detail: t(`jobsPage.${trackedJob.state}`), active: true }
    : null;
  const trackedWork = trackedTopGear ?? trackedQuick;

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
          {trackedWork ? <button className="queue-state queue-state-button" type="button" aria-live="polite" onClick={() => setPage(trackedWork.page)}>
            <span className={`status-dot ${trackedWork.active ? "status-dot-active" : ""}`} aria-hidden="true" />
            <span>{trackedWork.label}</span>
            <span className="muted">· {trackedWork.detail}</span>
          </button> : <div className="queue-state" aria-live="polite">
            <span className="status-dot" aria-hidden="true" />
            <span>{t("status.idle")}</span>
            <span className="muted">· {t("status.noActiveJobs")}</span>
          </div>}
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
            <HomePage navigate={setPage} runtime={runtime} />
          ) : page === "import" ? (
            <ImportPage onPrepared={(request, prepared) => { setQuickRequest(request); setPreview(prepared); setPage("quickSim"); }} />
          ) : page === "quickSim" ? (
            <QuickSimPage request={quickRequest} preview={preview} onChange={(request, prepared) => { setQuickRequest(request); setPreview(prepared); }} onStarted={(next) => { updateJob(next); openRun({ kind: "quick", jobId: next.id }); }} onImport={() => setPage("import")} />
          ) : page === "topGear" ? (
            <TopGearPage quick={quickRequest} onStarted={(next) => { updateTopGearSession(next); openRun({ kind: "topGear", sessionId: next.id }); }} onImport={() => setPage("import")} />
          ) : page === "jobs" ? (
            <JobsPage quickJobs={quickRuns} topGearSessions={topGearSessionsState} selected={selectedRun} onSelect={setSelectedRun} onQuickJob={updateJob} onTopGearSession={updateTopGearSession} onResult={openResult} />
          ) : page === "results" ? (
            <ResultsPage quickJobs={quickRuns} topGearSessions={topGearSessionsState} selected={selectedResult} onSelect={setSelectedResult} />
          ) : page === "history" ? (
            <HistoryPage quickJobs={quickRuns} topGearSessions={topGearSessionsState} onOpenRun={openRun} onOpenResult={openResult} onDelete={deleteRun} />
          ) : page === "settings" ? (
            <SettingsPage initialRuntime={runtime} onRuntimeChange={setRuntime} />
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

function HomePage({ navigate, runtime }: { navigate: (page: Page) => void; runtime: RuntimeView | null }) {
  const { t, i18n } = useTranslation();
  const dataDate = formatRuntimeDataDate(runtime?.activeDataDate ?? null, i18n.resolvedLanguage ?? "en");
  const runtimeLabel = runtime === null
    ? t("runtime.checking")
    : runtime.state === "ready"
      ? t("runtime.ready")
      : runtime.state === "damaged"
        ? t("runtime.damaged")
        : t("runtime.missing");
  const runtimeDetail = runtime?.state === "ready" && runtime.active
    ? t("runtime.readyDetail", {
        version: runtime.active.simcVersion,
        build: runtime.active.build,
        date: dataDate ?? t("runtime.unknownDate"),
      })
    : runtime?.state === "damaged"
      ? t("runtime.damagedDetail")
      : runtime === null
        ? t("runtime.checkingDetail")
        : t("runtime.missingDetail");
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
          <p className={`runtime-home-status runtime-home-${runtime?.state ?? "checking"}`}><span aria-hidden="true" />{runtimeLabel}</p>
          <p className="muted">{runtimeDetail}</p>
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
