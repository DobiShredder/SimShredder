import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Bell, CheckCircle2, Download, FolderCog, FolderOpen, Image, MonitorCog, MoonStar, RotateCcw, Save, ShieldCheck, Trash2 } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  formatRuntimeDataDate,
  formatRuntimeError,
  runtimeCheckUpdates,
  runtimeInstallLatest,
  runtimeRollback,
  runtimeStatus,
  type RuntimeView,
} from "../runtime";
import { iconCacheClear, iconCacheStatus, type IconCacheStatus } from "../icons";
import { storagePathsGet, storagePathsReset, storagePathsSave, type StoragePaths } from "../storage";
import { resetWindowState } from "../windowState";

type Operation = "checking" | "installing" | "rollingBack" | null;

export function SettingsPage({ initialRuntime, onRuntimeChange, preventSleep, notificationsEnabled, onPreventSleepChange, onNotificationsChange }: {
  initialRuntime: RuntimeView | null;
  onRuntimeChange: (runtime: RuntimeView) => void;
  preventSleep: boolean;
  notificationsEnabled: boolean;
  onPreventSleepChange: (enabled: boolean) => void;
  onNotificationsChange: (enabled: boolean) => void;
}) {
  const { t, i18n } = useTranslation();
  const [runtime, setRuntime] = useState<RuntimeView | null>(initialRuntime);
  const [operation, setOperation] = useState<Operation>(null);
  const [error, setError] = useState<string | null>(null);
  const [icons, setIcons] = useState<IconCacheStatus | null>(null);
  const [iconBusy, setIconBusy] = useState(false);
  const [iconError, setIconError] = useState<string | null>(null);
  const [storage, setStorage] = useState<StoragePaths | null>(null);
  const [storageDraft, setStorageDraft] = useState<Pick<StoragePaths, "workspace" | "simulationcraft" | "icons" | "exports"> | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [storageSaved, setStorageSaved] = useState(false);
  const [windowResetting, setWindowResetting] = useState(false);
  const [windowReset, setWindowReset] = useState(false);
  const [windowError, setWindowError] = useState<string | null>(null);

  const publishRuntime = useCallback((next: RuntimeView) => {
    setRuntime(next);
    onRuntimeChange(next);
  }, [onRuntimeChange]);

  const refresh = useCallback(async () => {
    setOperation("checking");
    setError(null);
    try {
      publishRuntime(await runtimeCheckUpdates());
    } catch (reason) {
      setError(formatRuntimeError(reason, t));
    } finally {
      setOperation(null);
    }
  }, [publishRuntime, t]);

  useEffect(() => setRuntime(initialRuntime), [initialRuntime]);

  useEffect(() => {
    void iconCacheStatus().then(setIcons).catch((reason) => setIconError(String(reason)));
    void storagePathsGet().then((paths) => {
      setStorage(paths);
      setStorageDraft(paths);
    }).catch((reason) => setStorageError(String(reason)));
  }, []);

  const updateStorageDraft = (key: "workspace" | "simulationcraft" | "icons" | "exports", value: string) => {
    setStorageSaved(false);
    setStorageDraft((current) => current ? { ...current, [key]: value } : current);
  };

  const browseStorage = async (key: "workspace" | "simulationcraft" | "icons" | "exports") => {
    if (!storageDraft) return;
    setStorageError(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: storageDraft[key],
        title: t("storage.browseTitle", { name: t(`storage.${key}`) }),
      });
      if (typeof selected === "string") updateStorageDraft(key, selected);
    } catch (reason) {
      setStorageError(String(reason));
    }
  };

  const saveStorage = async (event: FormEvent) => {
    event.preventDefault();
    if (!storageDraft) return;
    setStorageBusy(true);
    setStorageError(null);
    setStorageSaved(false);
    try {
      const next = await storagePathsSave(storageDraft);
      setStorage(next);
      setStorageDraft(next);
      setStorageSaved(true);
      try {
        setIcons(await iconCacheStatus());
      } catch (reason) {
        setIconError(String(reason));
      }
      try {
        publishRuntime(await runtimeStatus());
      } catch (reason) {
        setError(formatRuntimeError(reason, t));
      }
    } catch (reason) {
      setStorageError(String(reason));
    } finally {
      setStorageBusy(false);
    }
  };

  const resetStorage = async () => {
    setStorageBusy(true);
    setStorageError(null);
    setStorageSaved(false);
    try {
      const next = await storagePathsReset();
      setStorage(next);
      setStorageDraft(next);
      setStorageSaved(true);
      try {
        setIcons(await iconCacheStatus());
      } catch (reason) {
        setIconError(String(reason));
      }
      try {
        publishRuntime(await runtimeStatus());
      } catch (reason) {
        setError(formatRuntimeError(reason, t));
      }
    } catch (reason) {
      setStorageError(String(reason));
    } finally {
      setStorageBusy(false);
    }
  };

  const perform = async (nextOperation: Exclude<Operation, "checking" | null>) => {
    setOperation(nextOperation);
    setError(null);
    try {
      const next = nextOperation === "installing" ? await runtimeInstallLatest() : await runtimeRollback();
      publishRuntime(next);
    } catch (reason) {
      setError(formatRuntimeError(reason, t));
    } finally {
      setOperation(null);
    }
  };

  const isBusy = operation !== null;
  const storageLocked = storageBusy || isBusy || iconBusy;
  const activeId = runtime?.active?.id;
  const canRollback = Boolean(runtime && runtime.installed.some((record) => record.id !== activeId));
  const stateLabel = runtime?.state === "ready"
    ? t("runtime.ready")
    : runtime?.state === "damaged"
      ? t("runtime.damaged")
      : t("runtime.missing");
  const activeDataDate = formatRuntimeDataDate(runtime?.activeDataDate ?? null, i18n.resolvedLanguage ?? "en");
  const runtimeDiagnostic = runtime?.diagnostic ? formatRuntimeError(runtime.diagnostic, t) : null;
  const availableBuildLabel = runtime?.availableConfirmed
    ? `${runtime.availableVersion} · ${runtime.availableBuild}`
    : runtime?.state === "ready"
      ? t("runtime.usingInstalledUnconfirmed")
      : t("runtime.availabilityUnconfirmed");
  const formatBytes = (bytes: number) => `${(bytes / (1024 * 1024)).toLocaleString(undefined, { maximumFractionDigits: 1 })} MiB`;

  return (
    <div className="page settings-page">
      <p className="eyebrow">{t("nav.settings")}</p>
      <h1>{t("runtime.installTitle")}</h1>
      <p className="settings-lead">{t("runtime.installBody")}</p>

      <section className="setup-card" aria-labelledby="run-behavior-title">
        <div className="setup-card-header"><div><h2 id="run-behavior-title">{t("runBehavior.title")}</h2><p className="settings-lead">{t("runBehavior.body")}</p></div><Bell aria-hidden="true" size={34} className="setup-shield" /></div>
        <div className="quick-toggle-grid"><label className="checkbox-line"><input checked={notificationsEnabled} type="checkbox" onChange={(event) => onNotificationsChange(event.target.checked)} /><span><strong>{t("runBehavior.notifications")}</strong><small>{t("runBehavior.notificationsHelp")}</small></span></label><label className="checkbox-line"><input checked={preventSleep} type="checkbox" onChange={(event) => onPreventSleepChange(event.target.checked)} /><MoonStar aria-hidden="true" size={18} /><span><strong>{t("runBehavior.preventSleep")}</strong><small>{t("runBehavior.preventSleepHelp")}</small></span></label></div>
      </section>

      <section className="setup-card" aria-labelledby="window-state-title">
        <div className="setup-card-header">
          <div>
            <h2 id="window-state-title">{t("windowState.title")}</h2>
            <p className="settings-lead">{t("windowState.body")}</p>
          </div>
          <MonitorCog aria-hidden="true" size={34} className="setup-shield" />
        </div>
        {windowError ? <div className="inline-error" role="alert"><strong>{t("windowState.errorTitle")}</strong><code>{windowError}</code></div> : null}
        {windowReset ? <p className="safe-note" role="status"><CheckCircle2 aria-hidden="true" size={17} />{t("windowState.resetDone")}</p> : null}
        <div className="button-row">
          <button className="secondary-button" disabled={windowResetting} type="button" onClick={() => {
            setWindowResetting(true);
            setWindowReset(false);
            setWindowError(null);
            void resetWindowState()
              .then(() => setWindowReset(true))
              .catch((reason) => setWindowError(String(reason)))
              .finally(() => setWindowResetting(false));
          }}><RotateCcw aria-hidden="true" size={18} />{windowResetting ? t("windowState.resetting") : t("windowState.reset")}</button>
        </div>
      </section>

      <section className="setup-card" aria-labelledby="runtime-card-title">
        <div className="setup-card-header">
          <div>
            <h2 id="runtime-card-title">{t("runtime.title")}</h2>
            <p className={`runtime-pill runtime-${runtime?.state ?? "missing"}`}>
              {runtime?.state === "ready" ? (
                <CheckCircle2 aria-hidden="true" size={16} />
              ) : (
                <AlertTriangle aria-hidden="true" size={16} />
              )}
              <span>{stateLabel}</span>
            </p>
          </div>
          <ShieldCheck aria-hidden="true" size={34} className="setup-shield" />
        </div>

        {operation === "checking" && !runtime ? (
          <p role="status" className="runtime-progress">{t("runtime.checking")}</p>
        ) : null}

        <dl className="runtime-details">
          <div>
            <dt>{runtime?.state === "ready" && !runtime.availableConfirmed ? t("runtime.updateStatus") : t("runtime.available")}</dt>
            <dd>{availableBuildLabel}</dd>
          </div>
          <div>
            <dt>{t("runtime.active")}</dt>
            <dd>{activeId ?? t("runtime.none")}</dd>
          </div>
          <div>
            <dt>{t("runtime.dataDate")}</dt>
            <dd>{activeDataDate ?? t("runtime.unknownDate")}</dd>
          </div>
        </dl>

        <p className="safe-note"><ShieldCheck aria-hidden="true" size={17} />{t("runtime.noAdmin")}</p>

        {error ? (
          <div className="inline-error" role="alert">
            <strong>{t("runtime.errorTitle")}</strong>
            <code>{error}</code>
          </div>
        ) : null}

        {!error && runtimeDiagnostic && runtime?.state !== "ready" ? (
          <div className="inline-notice" role="status">
            <strong>{t("runtime.sourceWarningTitle")}</strong>
            <p>{runtimeDiagnostic}</p>
          </div>
        ) : null}

        {operation === "installing" ? (
          <div className="indeterminate" role="status" aria-label={t("runtime.installing")}>
            <span />
            <p>{t("runtime.installing")}</p>
          </div>
        ) : null}

        <div className="button-row">
          {runtime?.availableConfirmed && (runtime.state !== "ready" || runtime.updateAvailable) ? (
            <button className="primary-button" disabled={isBusy} type="button" onClick={() => void perform("installing")}>
              <Download aria-hidden="true" size={18} />
              {runtime?.updateAvailable ? t("runtime.update") : t("runtime.install")}
            </button>
          ) : null}
          <button className="secondary-button" disabled={isBusy || !canRollback} type="button" onClick={() => void perform("rollingBack")}>
            <RotateCcw aria-hidden="true" size={18} />
            {t("runtime.rollback")}
          </button>
          <button className="text-button" disabled={isBusy} type="button" onClick={() => void refresh()}>
            {t("runtime.retry")}
          </button>
        </div>
      </section>

      <section className="setup-card" aria-labelledby="storage-title">
        <div className="setup-card-header">
          <div>
            <h2 id="storage-title">{t("storage.title")}</h2>
            <p className="settings-lead">{t("storage.body")}</p>
          </div>
          <FolderCog aria-hidden="true" size={34} className="setup-shield" />
        </div>
        <p className="safe-note"><ShieldCheck aria-hidden="true" size={17} />{t("storage.configRoot", { path: storage?.configRoot ?? "—" })}</p>
        <form className="settings-form storage-form" onSubmit={(event) => void saveStorage(event)}>
          {(["workspace", "simulationcraft", "icons", "exports"] as const).map((key) => (
            <div className="storage-path-field" key={key}>
              <label htmlFor={`storage-${key}`}>{t(`storage.${key}`)}</label>
              <span className="storage-path-control">
                <input
                  id={`storage-${key}`}
                  type="text"
                  value={storageDraft?.[key] ?? ""}
                  disabled={storageLocked || !storageDraft}
                  spellCheck={false}
                  autoComplete="off"
                  onChange={(event) => updateStorageDraft(key, event.target.value)}
                />
                <button
                  className="secondary-button storage-browse-button"
                  disabled={storageLocked || !storageDraft}
                  type="button"
                  aria-label={t("storage.browseLabel", { name: t(`storage.${key}`) })}
                  onClick={() => void browseStorage(key)}
                >
                  <FolderOpen aria-hidden="true" size={17} />
                  {t("storage.browse")}
                </button>
              </span>
              <small>{t("storage.defaultPath", { path: storage?.[`default${key[0].toUpperCase()}${key.slice(1)}` as keyof StoragePaths] ?? "—" })}</small>
            </div>
          ))}
          <p className="storage-warning">{t("storage.moveWarning")}</p>
          {storageError ? <div className="inline-error" role="alert"><strong>{t("storage.errorTitle")}</strong><code>{storageError}</code></div> : null}
          {storageSaved ? <p className="safe-note" role="status"><CheckCircle2 aria-hidden="true" size={17} />{t("storage.saved")}</p> : null}
          <div className="button-row">
            <button className="primary-button" disabled={storageLocked || !storageDraft} type="submit"><Save aria-hidden="true" size={18} />{storageBusy ? t("storage.saving") : t("storage.save")}</button>
            <button className="secondary-button" disabled={storageLocked || !storage} type="button" onClick={() => void resetStorage()}><RotateCcw aria-hidden="true" size={18} />{t("storage.reset")}</button>
          </div>
        </form>
      </section>

      <section className="setup-card" aria-labelledby="icon-cache-title">
        <div className="setup-card-header">
          <div>
            <h2 id="icon-cache-title">{t("icons.title")}</h2>
            <p className="settings-lead">{t("icons.body")}</p>
          </div>
          <Image aria-hidden="true" size={34} className="setup-shield" />
        </div>
        <dl className="runtime-details">
          <div>
            <dt>{t("icons.cache")}</dt>
            <dd>{icons ? t("icons.usage", { used: formatBytes(icons.usedBytes), budget: formatBytes(icons.budgetBytes), count: icons.iconCount }) : "—"}</dd>
          </div>
          <div>
            <dt>{t("icons.provider")}</dt>
            <dd>{t("icons.providerOff")}</dd>
          </div>
        </dl>
        {iconError ? <div className="inline-error" role="alert"><strong>{t("icons.errorTitle")}</strong><code>{iconError}</code></div> : null}
        <div className="button-row">
          <button className="secondary-button" disabled={iconBusy || !icons?.iconCount} type="button" onClick={() => {
            setIconBusy(true);
            setIconError(null);
            void iconCacheClear().then(setIcons).catch((reason) => setIconError(String(reason))).finally(() => setIconBusy(false));
          }}><Trash2 aria-hidden="true" size={18} />{iconBusy ? t("icons.clearing") : t("icons.clear")}</button>
        </div>
      </section>

      <section className="installed-list" aria-labelledby="installed-heading">
        <h2 id="installed-heading">{t("runtime.installedVersions")}</h2>
        {runtime?.installed.length ? (
          <ul>
            {runtime.installed.map((record) => (
              <li key={record.id}>
                <span><strong>{record.simcVersion}</strong><small>{record.build} · WoW {record.gameVersion}</small></span>
                {record.id === activeId ? <span className="current-label">{t("runtime.current")}</span> : null}
              </li>
            ))}
          </ul>
        ) : (
          <p className="muted">{t("runtime.noInstalledVersions")}</p>
        )}
      </section>
      <p className="source-note">{t("runtime.sourceNote")}</p>
    </div>
  );
}
