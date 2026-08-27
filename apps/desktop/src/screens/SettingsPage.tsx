import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, CheckCircle2, Download, FolderCog, FolderOpen, Image, RotateCcw, Save, ShieldCheck, Trash2 } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  runtimeInstallLatest,
  runtimeRollback,
  runtimeStatus,
  type RuntimeView,
} from "../runtime";
import { iconCacheClear, iconCacheStatus, type IconCacheStatus } from "../icons";
import { storagePathsGet, storagePathsReset, storagePathsSave, type StoragePaths } from "../storage";

type Operation = "checking" | "installing" | "rollingBack" | null;

export function SettingsPage() {
  const { t } = useTranslation();
  const [runtime, setRuntime] = useState<RuntimeView | null>(null);
  const [operation, setOperation] = useState<Operation>("checking");
  const [error, setError] = useState<string | null>(null);
  const [icons, setIcons] = useState<IconCacheStatus | null>(null);
  const [iconBusy, setIconBusy] = useState(false);
  const [iconError, setIconError] = useState<string | null>(null);
  const [storage, setStorage] = useState<StoragePaths | null>(null);
  const [storageDraft, setStorageDraft] = useState<Pick<StoragePaths, "workspace" | "simulationcraft" | "icons" | "exports"> | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [storageSaved, setStorageSaved] = useState(false);

  const refresh = useCallback(async () => {
    setOperation("checking");
    setError(null);
    try {
      setRuntime(await runtimeStatus());
    } catch (reason) {
      setError(String(reason));
    } finally {
      setOperation(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
    void iconCacheStatus().then(setIcons).catch((reason) => setIconError(String(reason)));
    void storagePathsGet().then((paths) => {
      setStorage(paths);
      setStorageDraft(paths);
    }).catch((reason) => setStorageError(String(reason)));
  }, [refresh]);

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
        setRuntime(await runtimeStatus());
      } catch (reason) {
        setError(String(reason));
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
        setRuntime(await runtimeStatus());
      } catch (reason) {
        setError(String(reason));
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
      setRuntime(next);
    } catch (reason) {
      setError(String(reason));
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
  const formatBytes = (bytes: number) => `${(bytes / (1024 * 1024)).toLocaleString(undefined, { maximumFractionDigits: 1 })} MiB`;

  return (
    <div className="page settings-page">
      <p className="eyebrow">{t("nav.settings")}</p>
      <h1>{t("runtime.installTitle")}</h1>
      <p className="settings-lead">{t("runtime.installBody")}</p>

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
            <dt>{t("runtime.available")}</dt>
            <dd>{runtime ? `${runtime.availableVersion} · ${runtime.availableBuild}` : "—"}</dd>
          </div>
          <div>
            <dt>{t("runtime.active")}</dt>
            <dd>{activeId ?? t("runtime.none")}</dd>
          </div>
        </dl>

        <p className="safe-note"><ShieldCheck aria-hidden="true" size={17} />{t("runtime.noAdmin")}</p>

        {error || runtime?.diagnostic ? (
          <div className="inline-error" role="alert">
            <strong>{t("runtime.errorTitle")}</strong>
            <code>{error ?? runtime?.diagnostic}</code>
          </div>
        ) : null}

        {operation === "installing" ? (
          <div className="indeterminate" role="status" aria-label={t("runtime.installing")}>
            <span />
            <p>{t("runtime.installing")}</p>
          </div>
        ) : null}

        <div className="button-row">
          {runtime?.state !== "ready" || runtime.updateAvailable ? (
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
