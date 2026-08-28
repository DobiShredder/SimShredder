import { BarChart3, Gauge, Layers3, Trash2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { JobView } from "../quick";
import { runKey, type RunReference } from "../runs";
import type { TopGearSessionView } from "../topGear";

export function HistoryPage({ quickJobs, topGearSessions, onOpenRun, onOpenResult, onDelete }: {
  quickJobs: JobView[];
  topGearSessions: TopGearSessionView[];
  onOpenRun: (run: RunReference) => void;
  onOpenResult: (run: RunReference) => void;
  onDelete: (run: RunReference) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [pendingDelete, setPendingDelete] = useState<RunReference | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const entries = [
    ...topGearSessions.map((session) => ({ key: `top-gear-${session.id}`, run: { kind: "topGear", sessionId: session.id } as RunReference, title: t("historyPage.gearOptimizer"), identity: t("historyPage.session", { id: session.id }), state: session.stage === "complete" ? "succeeded" : session.currentJob.state, complete: session.stage === "complete", active: ["queued", "running"].includes(session.currentJob.state), icon: Layers3 })),
    ...quickJobs.map((job) => ({ key: `quick-${job.id}`, run: { kind: "quick", jobId: job.id } as RunReference, title: t("historyPage.characterAnalysis"), identity: t("jobsPage.job", { id: job.id }), state: job.state, complete: job.state === "succeeded", active: ["queued", "running"].includes(job.state), icon: Gauge })),
  ];
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
    {entries.length ? <ul className="run-history-list">{entries.map((entry) => { const Icon = entry.icon; return <li key={entry.key}><div className="card-icon"><Icon aria-hidden="true" size={19} /></div><div><strong>{entry.title}</strong><span>{entry.identity}</span></div><span className="run-state"><span className="status-dot" aria-hidden="true" />{t(`jobsPage.${entry.state}`)}</span><div className="history-actions"><button className="secondary-button" type="button" onClick={() => entry.complete ? onOpenResult(entry.run) : onOpenRun(entry.run)}>{entry.complete ? <BarChart3 aria-hidden="true" size={17} /> : null}{t(entry.complete ? "historyPage.openResult" : "historyPage.openRun")}</button><button className="icon-button history-delete" aria-label={t("historyPage.deleteLabel", { name: `${entry.title} ${entry.identity}` })} disabled={entry.active} title={entry.active ? t("historyPage.deleteActive") : t("historyPage.delete")} type="button" onClick={() => { setError(null); setPendingDelete(entry.run); }}><Trash2 aria-hidden="true" size={17} /></button></div></li>; })}</ul> : <p className="profile-empty">{t("historyPage.empty")}</p>}
    {pendingDelete && selectedEntry ? <div className="modal-backdrop"><dialog className="update-dialog" open aria-labelledby="history-delete-title" aria-describedby="history-delete-description"><p className="eyebrow">{t("historyPage.deleteEyebrow")}</p><h2 id="history-delete-title">{t("historyPage.deleteTitle", { name: selectedEntry.identity })}</h2><p id="history-delete-description">{t("historyPage.deleteBody")}</p>{error ? <div className="inline-error" role="alert"><code>{error}</code></div> : null}<div className="button-row"><button className="danger-button" disabled={busy} type="button" onClick={() => void confirmDelete()}><Trash2 aria-hidden="true" size={17} />{busy ? t("historyPage.deleting") : t("historyPage.confirmDelete")}</button><button className="secondary-button" disabled={busy} autoFocus type="button" onClick={() => setPendingDelete(null)}>{t("historyPage.cancel")}</button></div></dialog></div> : null}
  </div>;
}
