import { Cpu, Play, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { quickPrepare, quickStart, type JobView, type PreparedQuickSim, type QuickSimRequest } from "../quick";
import { EntityTooltip, type TooltipModel } from "../tooltips";

export function QuickSimPage({ request, preview, onChange, onStarted, onImport }: {
  request: QuickSimRequest | null;
  preview: PreparedQuickSim | null;
  onChange: (request: QuickSimRequest, preview: PreparedQuickSim) => void;
  onStarted: (job: JobView) => void;
  onImport: () => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(request);
  const [operation, setOperation] = useState<"preview" | "start" | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setDraft(request), [request]);

  if (!draft || !preview) {
    return <div className="page placeholder-page"><p className="eyebrow">{t("quick.eyebrow")}</p><h1>{t("quick.importFirst")}</h1><button className="primary-button" type="button" onClick={onImport}>{t("quick.goImport")}</button></div>;
  }
  const update = <Key extends keyof QuickSimRequest>(key: Key, value: QuickSimRequest[Key]) => setDraft({ ...draft, [key]: value });
  const talentLabels: Record<string, string> = {
    talents: t("quick.talentLoadout"),
    class_talents: t("quick.classTalents"),
    spec_talents: t("quick.specTalents"),
    hero_talents: t("quick.heroTalents"),
  };
  const talentTooltip: TooltipModel = {
    kind: "talent",
    id: null,
    title: t("quick.talentConfiguration"),
    category: t("tooltip.categories.talent"),
    details: preview.profile.talents.map((talent) => ({
      label: talentLabels[talent.name] ?? talent.name,
      value: talent.value,
    })),
  };
  const refresh = async () => {
    setOperation("preview");
    setError(null);
    try { onChange(draft, await quickPrepare(draft)); } catch (reason) { setError(String(reason)); } finally { setOperation(null); }
  };
  const start = async () => {
    setOperation("start");
    setError(null);
    try {
      const refreshed = await quickPrepare(draft);
      onChange(draft, refreshed);
      onStarted(await quickStart(draft));
    } catch (reason) { setError(String(reason)); } finally { setOperation(null); }
  };

  return (
    <div className="page quick-page">
      <p className="eyebrow">{t("quick.eyebrow")}</p>
      <h1>{t("quick.title")}</h1>
      <div className="profile-strip">
        <div><span>{t("quick.profile")}</span><div className="profile-title-with-tooltip"><strong>{preview.profile.name}</strong>{preview.profile.talents.length ? <EntityTooltip model={talentTooltip} /> : null}</div><small>{preview.profile.class} · {preview.profile.specialization} · {preview.profile.race}</small></div>
        <div><span>{t("quick.equipment")}</span><strong>{t("quick.equipped", { count: preview.profile.equippedItems })}</strong><small>{t("quick.bags", { count: preview.profile.bagItems })}</small></div>
        <div><span><Cpu aria-hidden="true" size={15} />{t("quick.cpu")}</span><strong>{t("quick." + draft.cpuPreset)}</strong><small>{t("quick.threads", { threads: preview.threads, workers: preview.profilesetWorkThreads })}</small></div>
      </div>
      <div className="quick-grid">
        <section className="settings-form" aria-labelledby="quick-settings-heading">
          <h2 id="quick-settings-heading">{t("quick.settings")}</h2>
          <label>{t("quick.iterations")}<input min={100} max={10_000_000} type="number" value={draft.iterations} onChange={(event) => update("iterations", Number(event.target.value))} /></label>
          <label>{t("quick.duration")}<span className="input-with-unit"><input min={30} max={3600} type="number" value={draft.maxTimeSeconds} onChange={(event) => update("maxTimeSeconds", Number(event.target.value))} /><span>{t("quick.seconds")}</span></span></label>
          <label>{t("quick.targets")}<input min={1} max={20} type="number" value={draft.desiredTargets} onChange={(event) => update("desiredTargets", Number(event.target.value))} /></label>
          <label>{t("quick.variance")}<input min={0} max={0.5} step={0.05} type="number" value={draft.varyCombatLength} onChange={(event) => update("varyCombatLength", Number(event.target.value))} /></label>
          <label>{t("quick.fightStyle")}<select value={draft.fightStyle} onChange={(event) => update("fightStyle", event.target.value as QuickSimRequest["fightStyle"])}><option value="Patchwerk">{t("quick.patchwerk")}</option><option value="DungeonSlice">{t("quick.dungeonSlice")}</option><option value="HecticAddCleave">{t("quick.hecticAddCleave")}</option><option value="LightMovement">{t("quick.lightMovement")}</option></select></label>
          <fieldset><legend>{t("quick.cpu")}</legend>{(["efficient", "balanced", "maximum"] as const).map((value) => <label className="radio-line" key={value}><input checked={draft.cpuPreset === value} name="cpu-preset" type="radio" onChange={() => update("cpuPreset", value)} />{t("quick." + value)}</label>)}</fieldset>
        </section>
        <section className="generated-panel" aria-labelledby="generated-heading"><h2 id="generated-heading">{t("quick.generated")}</h2><p>{t("quick.generatedHelp")}</p><pre tabIndex={0}>{preview.generatedInput}</pre></section>
      </div>
      {error ? <div className="inline-error" role="alert"><strong>{t("quick.errorTitle")}</strong><code>{error}</code></div> : null}
      <div className="button-row quick-actions">
        <button className="secondary-button" disabled={operation !== null} type="button" onClick={() => void refresh()}><RefreshCw aria-hidden="true" size={18} />{operation === "preview" ? t("quick.refreshing") : t("quick.refresh")}</button>
        <button className="primary-button" disabled={operation !== null} type="button" onClick={() => void start()}><Play aria-hidden="true" size={18} />{operation === "start" ? t("quick.starting") : t("quick.run")}</button>
      </div>
    </div>
  );
}
