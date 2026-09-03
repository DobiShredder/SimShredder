import { BarChart3, Gauge, Layers3, Search, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { JobView } from "../quick";
import { runKey, type RunReference } from "../runs";
import { buildRunCatalog } from "../runCatalog";
import type { TopGearSessionView } from "../topGear";
import { RunManagementControls } from "../components/RunManagementControls";
import { useModalDialog } from "../components/useModalDialog";

export function HistoryPage({ quickJobs, topGearSessions, runNames, onOpenRun, onOpenResult, onDelete, onRerun, onEditRerun, onRename, canEditRun }: {
  quickJobs: JobView[];
  topGearSessions: TopGearSessionView[];
  runNames: Record<string, string>;
  onOpenRun: (run: RunReference) => void;
  onOpenResult: (run: RunReference) => void;
  onDelete: (run: RunReference) => Promise<void>;
  onRerun: (run: RunReference) => Promise<void>;
  onEditRerun: (run: RunReference) => Promise<void>;
  onRename: (run: RunReference, name: string | null) => void;
  canEditRun: (run: RunReference) => boolean;
}) {
  const { t } = useTranslation();
  const [pendingDelete, setPendingDelete] = useState<RunReference | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");
  const deleteDialog = useModalDialog(Boolean(pendingDelete), () => setPendingDelete(null));
  const entries = useMemo(() => buildRunCatalog(quickJobs, topGearSessions, runNames).history, [quickJobs, runNames, topGearSessions]);
  const visible = entries.filter((entry) => {
    const matchesQuery = `${entry.displayName} ${entry.characterName} ${entry.specialization} ${entry.type}`.toLocaleLowerCase().includes(query.toLocaleLowerCase());
    return matchesQuery && (typeFilter === "all" || entry.type === typeFilter) && (statusFilter === "all" || entry.state === statusFilter);
  });
  const selectedEntry = pendingDelete ? entries.find((entry) => runKey(entry.run) === runKey(pendingDelete)) : null;
  const confirmDelete = async () => {
    if (!pendingDelete) return;
    setBusy(true); setError(null);
    try {
      await onDelete(pendingDelete);
      setPendingDelete(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  return <div className="page history-page">
    <p className="eyebrow">{t("historyPage.eyebrow")}</p><h1>{t("historyPage.title")}</h1><p className="settings-lead">{t("historyPage.body")}</p>
    <section className="history-filters" aria-label={t("historyPage.filters")}><label><span><Search aria-hidden="true" size={16} />{t("historyPage.search")}</span><input value={query} onChange={(event) => setQuery(event.target.value)} /></label><label>{t("historyPage.type")}<select value={typeFilter} onChange={(event) => setTypeFilter(event.target.value)}><option value="all">{t("historyPage.all")}</option><option value="characterAnalysis">{t("historyPage.characterAnalysis")}</option><option value="gearOptimizer">{t("historyPage.gearOptimizer")}</option></select></label><label>{t("historyPage.status")}<select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}><option value="all">{t("historyPage.all")}</option>{["succeeded", "failed", "canceled", "interrupted"].map((state) => <option key={state} value={state}>{t(`jobsPage.${state}`)}</option>)}</select></label></section>
    {visible.length ? <ul className="run-history-list">{visible.map((entry) => { const Icon = entry.type === "gearOptimizer" ? Layers3 : Gauge; const complete = entry.state === "succeeded"; const recordId = entry.run.kind === "topGear" ? t("historyPage.session", { id: entry.run.sessionId }) : t("jobsPage.job", { id: entry.run.jobId }); return <li key={entry.key}><div className="card-icon"><Icon aria-hidden="true" size={19} /></div><div className="history-entry-summary"><strong>{entry.displayName}</strong><span>{entry.characterName} · {entry.specialization} · {t(`historyPage.${entry.type}`)} · {recordId} · {new Date(entry.createdUnixMillis).toLocaleString()} · {entry.settings.fightStyle}, {entry.settings.desiredTargets}</span></div><span className="run-state"><span className="status-dot" aria-hidden="true" />{t(`jobsPage.${entry.state}`)}</span><div className="history-actions"><button className="secondary-button" type="button" onClick={() => complete ? onOpenResult(entry.run) : onOpenRun(entry.run)}>{complete ? <BarChart3 aria-hidden="true" size={17} /> : null}{t(complete ? "historyPage.openResult" : "historyPage.openRun")}</button><button className="icon-button history-delete" aria-label={t("historyPage.deleteLabel", { name: `${t(`historyPage.${entry.type}`)} ${recordId}` })} type="button" onClick={() => { setError(null); setPendingDelete(entry.run); }}><Trash2 aria-hidden="true" size={17} /></button></div><RunManagementControls run={entry.run} displayName={entry.displayName} defaultName={`${entry.characterName} · ${entry.specialization}`} canEdit={canEditRun(entry.run)} onRerun={onRerun} onEditRerun={onEditRerun} onRename={onRename} /></li>; })}</ul> : <p className="profile-empty">{entries.length ? t("historyPage.noMatches") : t("historyPage.empty")}</p>}
    {error && !pendingDelete ? <div className="inline-error" role="alert"><code>{error}</code></div> : null}
    {pendingDelete && selectedEntry ? <div className="modal-backdrop"><dialog ref={deleteDialog} className="update-dialog" open aria-labelledby="history-delete-title" aria-describedby="history-delete-description"><p className="eyebrow">{t("historyPage.deleteEyebrow")}</p><h2 id="history-delete-title">{t("historyPage.deleteTitle", { name: selectedEntry.displayName })}</h2><p id="history-delete-description">{t("historyPage.deleteBody")}</p>{error ? <div className="inline-error" role="alert"><code>{error}</code></div> : null}<div className="button-row"><button className="danger-button" disabled={busy} type="button" onClick={() => void confirmDelete()}><Trash2 aria-hidden="true" size={17} />{busy ? t("historyPage.deleting") : t("historyPage.confirmDelete")}</button><button className="secondary-button" disabled={busy} autoFocus data-modal-initial-focus type="button" onClick={() => setPendingDelete(null)}>{t("historyPage.cancel")}</button></div></dialog></div> : null}
  </div>;
}
