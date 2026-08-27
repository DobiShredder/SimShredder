import { CheckCircle2, CircleStop, RotateCcw } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { quickCancel, quickJobStatus, quickResult, quickRetry, type JobView, type QuickResultView } from "../quick";

const terminal = new Set(["succeeded", "failed", "canceled"]);

export function JobsPage({ initialJob, onJob, onResult }: {
  initialJob: JobView | null;
  onJob: (job: JobView) => void;
  onResult: (result: QuickResultView) => void;
}) {
  const { t } = useTranslation();
  const [job, setJob] = useState(initialJob);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => setJob(initialJob), [initialJob]);
  useEffect(() => {
    if (!job || terminal.has(job.state)) return;
    const timer = window.setInterval(() => {
      void quickJobStatus(job.id).then((next) => { setJob(next); onJob(next); }).catch((reason) => setError(String(reason)));
    }, 500);
    return () => window.clearInterval(timer);
  }, [job, onJob]);

  if (!job) return <div className="page placeholder-page"><p className="eyebrow">{t("jobsPage.eyebrow")}</p><h1>{t("jobsPage.noJob")}</h1></div>;
  const total = job.succeededBatches + job.pendingBatches;
  const action = async (kind: "cancel" | "retry") => {
    setBusy(true); setError(null);
    try {
      const next = kind === "cancel" ? await quickCancel(job.id) : await quickRetry(job.id);
      setJob(next); onJob(next);
    } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const showResult = async () => {
    setBusy(true);
    try { onResult(await quickResult(job.id)); } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  return (
    <div className="page jobs-page">
      <p className="eyebrow">{t("jobsPage.eyebrow")}</p><h1>{t("jobsPage.title")}</h1>
      <section className="job-card" aria-live="polite">
        <div className="job-title"><div><span>{t("jobsPage.job", { id: job.id })}</span><strong>{t("jobsPage." + job.state)}</strong></div><CheckCircle2 aria-hidden="true" size={28} /></div>
        <div className="progress-track" role="progressbar" aria-label={t("jobsPage.progress", { done: job.succeededBatches, total })} aria-valuemin={0} aria-valuemax={total} aria-valuenow={job.succeededBatches}><span style={{ width: String(total ? (job.succeededBatches / total) * 100 : 0) + "%" }} /></div>
        <p className="muted">{t("jobsPage.progress", { done: job.succeededBatches, total })}</p>
        {job.failure || error ? <div className="inline-error" role="alert"><strong>{t("jobsPage.diagnostic")}</strong><code>{error ?? job.failure}</code></div> : null}
        <ul className="attempt-list">{job.attempts.map((attempt) => <li key={attempt.id}><span>{t("jobsPage.attempt", { sequence: attempt.sequence })}</span><strong>{t("jobsPage." + attempt.state)}</strong>{attempt.cacheHit ? <small>{t("jobsPage.cacheHit")}</small> : null}{attempt.failure ? <code>{attempt.failure}</code> : null}</li>)}</ul>
        <div className="button-row">
          {["queued", "running"].includes(job.state) ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void action("cancel")}><CircleStop aria-hidden="true" size={18} />{t("jobsPage.cancel")}</button> : null}
          {["failed", "canceled", "interrupted"].includes(job.state) ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void action("retry")}><RotateCcw aria-hidden="true" size={18} />{t("jobsPage.retry")}</button> : null}
          {job.state === "succeeded" ? <button className="primary-button" disabled={busy} type="button" onClick={() => void showResult()}>{t("jobsPage.viewResult")}</button> : null}
        </div>
      </section>
    </div>
  );
}
