import { BarChart3, CheckCircle2, CircleStop, Gauge, Layers3, RotateCcw } from "lucide-react";
import type { TFunction } from "i18next";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { quickCancel, quickJobStatus, quickRetry, type JobView } from "../quick";
import { runKey, sameRun, type RunReference } from "../runs";
import { topGearCancel, topGearRetry, topGearStatus, type TopGearSessionView } from "../topGear";

const active = (state: string) => state === "queued" || state === "running";

export function JobsPage({ quickJobs, topGearSessions, selected, onSelect, onQuickJob, onTopGearSession, onResult }: {
  quickJobs: JobView[];
  topGearSessions: TopGearSessionView[];
  selected: RunReference | null;
  onSelect: (run: RunReference) => void;
  onQuickJob: (job: JobView) => void;
  onTopGearSession: (session: TopGearSessionView) => void;
  onResult: (run: RunReference) => void;
}) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const entries = useMemo(() => [
    ...topGearSessions.map((session) => ({ run: { kind: "topGear", sessionId: session.id } as RunReference, state: session.stage === "complete" ? "succeeded" : session.currentJob.state })),
    ...quickJobs.map((job) => ({ run: { kind: "quick", jobId: job.id } as RunReference, state: job.state })),
  ], [quickJobs, topGearSessions]);

  useEffect(() => {
    if ((!selected || !entries.some((entry) => sameRun(selected, entry.run))) && entries[0]) onSelect(entries[0].run);
  }, [entries, onSelect, selected]);
  const quick = selected?.kind === "quick" ? quickJobs.find((job) => job.id === selected.jobId) ?? null : null;
  const gear = selected?.kind === "topGear" ? topGearSessions.find((session) => session.id === selected.sessionId) ?? null : null;

  useEffect(() => {
    if (!quick || !active(quick.state)) return;
    const timer = window.setInterval(() => void quickJobStatus(quick.id).then(onQuickJob).catch((reason) => setError(String(reason))), 500);
    return () => window.clearInterval(timer);
  }, [onQuickJob, quick]);
  useEffect(() => {
    if (!gear || gear.stage === "complete") return;
    const timer = window.setInterval(() => void topGearStatus(gear.id).then(onTopGearSession).catch((reason) => setError(String(reason))), 500);
    return () => window.clearInterval(timer);
  }, [gear, onTopGearSession]);
  useEffect(() => {
    if (gear?.stage === "complete") onResult({ kind: "topGear", sessionId: gear.id });
  }, [gear?.id, gear?.stage, onResult]);

  const quickAction = async (kind: "cancel" | "retry") => {
    if (!quick) return;
    setBusy(true); setError(null);
    try { onQuickJob(kind === "cancel" ? await quickCancel(quick.id) : await quickRetry(quick.id)); }
    catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const gearAction = async (kind: "cancel" | "retry") => {
    if (!gear) return;
    setBusy(true); setError(null);
    try {
      const next = kind === "cancel" ? await topGearCancel(gear.id) : await topGearRetry(gear.id);
      onTopGearSession(next);
    }
    catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };

  return <div className="page jobs-page">
    <p className="eyebrow">{t("jobsPage.eyebrow")}</p><h1>{t("jobsPage.title")}</h1><p className="settings-lead">{t("jobsPage.body")}</p>
    {entries.length ? <div className="run-workspace"><aside className="run-selector" aria-label={t("jobsPage.runList")}>{entries.map((entry) => {
      const isGear = entry.run.kind === "topGear";
      const identity = isGear ? (entry.run as Extract<RunReference, { kind: "topGear" }>).sessionId : t("jobsPage.job", { id: (entry.run as Extract<RunReference, { kind: "quick" }>).jobId });
      return <button aria-current={sameRun(selected, entry.run) ? "true" : undefined} key={runKey(entry.run)} type="button" onClick={() => onSelect(entry.run)}>{isGear ? <Layers3 aria-hidden="true" size={17} /> : <Gauge aria-hidden="true" size={17} />}<span><strong>{t(isGear ? "historyPage.gearOptimizer" : "historyPage.characterAnalysis")}</strong><small>{identity}</small></span><em>{t(`jobsPage.${entry.state}`)}</em></button>;
    })}</aside><div className="run-detail">
      {quick ? <QuickJobDetail job={quick} busy={busy} error={error} onAction={quickAction} onResult={() => onResult({ kind: "quick", jobId: quick.id })} t={t} /> : null}
      {gear ? <GearJobDetail session={gear} busy={busy} error={error} onAction={gearAction} onResult={() => onResult({ kind: "topGear", sessionId: gear.id })} t={t} /> : null}
    </div></div> : <p className="profile-empty">{t("jobsPage.noJob")}</p>}
  </div>;
}

function QuickJobDetail({ job, busy, error, onAction, onResult, t }: { job: JobView; busy: boolean; error: string | null; onAction: (kind: "cancel" | "retry") => Promise<void>; onResult: () => void; t: TFunction }) {
  const total = job.succeededBatches + job.pendingBatches;
  return <section className="job-card run-detail-card" aria-live="polite"><div className="job-title"><div><span>{t("jobsPage.job", { id: job.id })}</span><strong>{t(`jobsPage.${job.state}`)}</strong></div><CheckCircle2 aria-hidden="true" size={28} /></div><div className="progress-track" role="progressbar" aria-label={t("jobsPage.progress", { done: job.succeededBatches, total })} aria-valuemin={0} aria-valuemax={total} aria-valuenow={job.succeededBatches}><span style={{ width: `${total ? job.succeededBatches / total * 100 : 0}%` }} /></div><p className="muted">{t("jobsPage.progress", { done: job.succeededBatches, total })}</p>{job.failure || error ? <div className="inline-error" role="alert"><strong>{t("jobsPage.diagnostic")}</strong><code>{error ?? job.failure}</code></div> : null}<ul className="attempt-list">{job.attempts.map((attempt) => <li key={attempt.id}><span>{t("jobsPage.attempt", { sequence: attempt.sequence })}</span><strong>{t(`jobsPage.${attempt.state}`)}</strong>{attempt.cacheHit ? <small>{t("jobsPage.cacheHit")}</small> : null}{attempt.failure ? <code>{attempt.failure}</code> : null}</li>)}</ul><RunActions state={job.state} busy={busy} onCancel={() => void onAction("cancel")} onRetry={() => void onAction("retry")} onResult={onResult} t={t} /></section>;
}

function GearJobDetail({ session, busy, error, onAction, onResult, t }: { session: TopGearSessionView; busy: boolean; error: string | null; onAction: (kind: "cancel" | "retry") => Promise<void>; onResult: () => void; t: TFunction }) {
  const stages = ["low_precision", "medium_precision", "high_precision"] as const;
  const current = session.stage === "complete" ? stages.length : session.stage === "action_plan" ? 2 : stages.indexOf(session.stage);
  const state = session.stage === "complete" ? "succeeded" : session.currentJob.state;
  const messageKey = `topGear.job_${state}`;
  const diagnostic = error ?? session.pipelineFailure ?? session.currentJob.failure;
  return <section className="job-card run-detail-card" aria-live="polite"><div className="job-title"><div><span>{t("topGear.session", { id: session.id })}</span><h2>{t(`topGear.stage_${session.stage}`)}</h2></div><Layers3 aria-hidden="true" size={28} /></div><div className="optimizer-current-state"><span className={`status-dot ${active(state) ? "status-dot-active" : ""}`} aria-hidden="true" /><strong>{t(`jobsPage.${state}`)}</strong><span>{t(messageKey)}</span></div><ol className="optimizer-stages" aria-label={t("topGear.stageProgress")}>{stages.map((stage, index) => { const step = index < current || session.stage === "complete" ? "complete" : index === current ? "current" : "upcoming"; return <li data-state={step} aria-current={step === "current" ? "step" : undefined} key={stage}><span>{index + 1}</span><div><strong>{t(`topGear.stage_${stage}`)}</strong><small>{t(`topGear.step_${step}`)}</small></div></li>; })}</ol><div className={`progress-track ${active(state) ? "progress-track-indeterminate" : ""}`} role="progressbar" aria-label={t("topGear.progress", { done: session.completedExecutions, total: session.totalExecutions })} aria-valuemin={0} aria-valuemax={session.totalExecutions} aria-valuenow={active(state) ? undefined : session.completedExecutions}><span style={active(state) ? undefined : { width: `${session.totalExecutions ? session.completedExecutions / session.totalExecutions * 100 : 0}%` }} /></div><p className="muted">{t("topGear.progress", { done: session.completedExecutions, total: session.totalExecutions })}</p>{diagnostic ? <div className="inline-error" role="alert"><strong>{t("jobsPage.diagnostic")}</strong><code>{diagnostic}</code></div> : null}<div className="button-row">{active(state) ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void onAction("cancel")}><CircleStop aria-hidden="true" size={18} />{t("jobsPage.cancel")}</button> : null}{["failed", "canceled", "interrupted"].includes(state) || session.pipelineFailure ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void onAction("retry")}><RotateCcw aria-hidden="true" size={18} />{t("jobsPage.retry")}</button> : null}{session.stage === "complete" ? <button className="primary-button" type="button" onClick={onResult}><BarChart3 aria-hidden="true" size={18} />{t("jobsPage.viewResult")}</button> : null}</div></section>;
}

function RunActions({ state, busy, onCancel, onRetry, onResult, t }: { state: string; busy: boolean; onCancel: () => void; onRetry: () => void; onResult: () => void; t: TFunction }) {
  return <div className="button-row">{active(state) ? <button className="secondary-button" disabled={busy} type="button" onClick={onCancel}><CircleStop aria-hidden="true" size={18} />{t("jobsPage.cancel")}</button> : null}{["failed", "canceled", "interrupted"].includes(state) ? <button className="secondary-button" disabled={busy} type="button" onClick={onRetry}><RotateCcw aria-hidden="true" size={18} />{t("jobsPage.retry")}</button> : null}{state === "succeeded" ? <button className="primary-button" disabled={busy} type="button" onClick={onResult}><BarChart3 aria-hidden="true" size={18} />{t("jobsPage.viewResult")}</button> : null}</div>;
}
