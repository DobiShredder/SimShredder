import { Cpu, Play, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { quickPrepare, quickStart, type JobView, type PreparedQuickSim, type QuickSimRequest } from "../quick";
import { EntityTooltip, type TooltipModel } from "../tooltips";
import { PresetControls } from "../components/PresetControls";
import { saveLastRequest } from "../workflowPreferences";

export function QuickSimPage({ profileId, request, preview, onChange, onStarted, onImport }: {
  profileId: string | null;
  request: QuickSimRequest | null;
  preview: PreparedQuickSim | null;
  onChange: (request: QuickSimRequest, preview: PreparedQuickSim) => void;
  onStarted: (job: JobView, request: QuickSimRequest) => void;
  onImport: () => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(request);
  const [operation, setOperation] = useState<"preview" | "start" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  useEffect(() => setDraft(request), [request]);
  useEffect(() => {
    if (profileId && draft) {
      try { saveLastRequest(profileId, { kind: "quick", profileId, request: draft }); } catch { /* Keep the in-memory draft usable. */ }
    }
  }, [draft, profileId]);
  useEffect(() => { if (error) errorRef.current?.focus(); }, [error]);

  if (!draft || !preview) {
    return <div className="page placeholder-page"><p className="eyebrow">{t("quick.eyebrow")}</p><h1>{t("quick.title")}</h1><p className="placeholder-description">{t("quick.importFirst")}</p><button className="primary-button" type="button" onClick={onImport}>{t("quick.goImport")}</button></div>;
  }
  const update = <Key extends keyof QuickSimRequest>(key: Key, value: QuickSimRequest[Key]) => setDraft({ ...draft, [key]: value });
  const updateAnalysis = <Key extends keyof QuickSimRequest["analysis"]>(key: Key, value: QuickSimRequest["analysis"][Key]) => setDraft({ ...draft, analysis: { ...draft.analysis, [key]: value } });
  const updateRaidBuff = <Key extends keyof QuickSimRequest["analysis"]["raidBuffs"]>(key: Key, value: boolean) => setDraft({ ...draft, analysis: { ...draft.analysis, raidBuffs: { ...draft.analysis.raidBuffs, [key]: value } } });
  const updateConsumable = <Key extends keyof QuickSimRequest["analysis"]["consumableOptions"]>(key: Key, value: boolean) => setDraft({ ...draft, analysis: { ...draft.analysis, consumableOptions: { ...draft.analysis.consumableOptions, [key]: value } } });
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
      onStarted(await quickStart(draft), draft);
    } catch (reason) { setError(String(reason)); } finally { setOperation(null); }
  };
  const builtIns = [
    { id: "builtin-single-target", name: t("presets.singleTarget"), summary: t("presets.singleTargetSummary"), request: { ...draft, desiredTargets: 1, fightStyle: "Patchwerk" as const } },
    { id: "builtin-aoe", name: t("presets.aoe"), summary: t("presets.aoeSummary"), request: { ...draft, desiredTargets: 5, fightStyle: "HecticAddCleave" as const } },
  ];

  return (
    <div className="page quick-page" aria-describedby={error ? "quick-validation-error" : undefined}>
      <p className="eyebrow">{t("quick.eyebrow")}</p>
      <h1>{t("quick.title")}</h1>
      {profileId ? <PresetControls profileId={profileId} kind="quick" request={draft} builtIns={builtIns} onApply={(request) => setDraft(request as QuickSimRequest)} /> : null}
      <div className="profile-strip">
        <div><span>{t("quick.profile")}</span><div className="profile-title-with-tooltip"><strong>{preview.profile.name}</strong>{preview.profile.talents.length ? <EntityTooltip model={talentTooltip} /> : null}</div><small>{preview.profile.class} · {preview.profile.specialization} · {preview.profile.race}</small></div>
        <div><span>{t("quick.equipment")}</span><strong>{t("quick.equipped", { count: preview.profile.equippedItems })}</strong><small>{t("quick.bags", { count: preview.profile.bagItems })}</small></div>
        <div><span><Cpu aria-hidden="true" size={15} />{t("quick.cpu")}</span><strong>{t("quick." + draft.cpuPreset)}</strong><small>{t("quick.threads", { threads: preview.threads, workers: preview.profilesetWorkThreads })}</small></div>
      </div>
      <details className="input-compatibility">
        <summary>{t("quick.inputCompatibility.title")}</summary>
        <p>{t("quick.inputCompatibility.body")}</p>
        <ul>
          <li>{t("quick.inputCompatibility.supported", { count: preview.profile.inputCompatibility.supportedEditable })}</li>
          <li>{t("quick.inputCompatibility.preserved", { count: preview.profile.inputCompatibility.preservedNotEditable })}</li>
          <li>{t("quick.inputCompatibility.blocked", { count: preview.profile.inputCompatibility.executionBlocked })}</li>
        </ul>
        {preview.profile.inputCompatibility.diagnostics.length ? <ul className="compatibility-diagnostics">{preview.profile.inputCompatibility.diagnostics.map((diagnostic) => <li key={`${diagnostic.line}-${diagnostic.key ?? "line"}`}><code>{t("quick.inputCompatibility.line", { line: diagnostic.line })}{diagnostic.key ? ` · ${diagnostic.key}` : ""}</code> — {t(`quick.inputCompatibility.reasons.${diagnostic.category}`)}</li>)}</ul> : null}
      </details>
      <div className="quick-grid">
        <section className="settings-form" aria-labelledby="quick-settings-heading">
          <h2 id="quick-settings-heading">{t("quick.settings")}</h2>
          <fieldset><legend>{t("quick.precision")}</legend>{(["fixed", "smart"] as const).map((value) => <label className="radio-line" key={value}><input checked={draft.analysis.precision === value} name="precision-mode" type="radio" onChange={() => updateAnalysis("precision", value)} />{t(`quick.precision${value === "fixed" ? "Fixed" : "Smart"}`)}</label>)}</fieldset>
          <label>{t(draft.analysis.precision === "smart" ? "quick.maxIterations" : "quick.iterations")}<input min={100} max={10_000_000} type="number" value={draft.iterations} onChange={(event) => update("iterations", Number(event.target.value))} /></label>
          {draft.analysis.precision === "smart" ? <label>{t("quick.targetError")}<span className="input-with-unit"><input min={0.01} max={5} step={0.01} type="number" value={draft.analysis.targetError} onChange={(event) => updateAnalysis("targetError", Number(event.target.value))} /><span>%</span></span></label> : null}
          <label>{t("quick.duration")}<span className="input-with-unit"><input min={30} max={3600} type="number" value={draft.maxTimeSeconds} onChange={(event) => update("maxTimeSeconds", Number(event.target.value))} /><span>{t("quick.seconds")}</span></span></label>
          <label>{t("quick.targets")}<input min={1} max={20} type="number" value={draft.desiredTargets} onChange={(event) => update("desiredTargets", Number(event.target.value))} /></label>
          <label>{t("quick.fightStyle")}<select value={draft.fightStyle} onChange={(event) => update("fightStyle", event.target.value as QuickSimRequest["fightStyle"])}><option value="Patchwerk">{t("quick.patchwerk")}</option><option value="CastingPatchwerk">{t("quick.castingPatchwerk")}</option><option value="DungeonSlice">{t("quick.dungeonSlice")}</option><option value="HecticAddCleave">{t("quick.hecticAddCleave")}</option><option value="LightMovement">{t("quick.lightMovement")}</option><option value="HeavyMovement">{t("quick.heavyMovement")}</option><option value="HelterSkelter">{t("quick.helterSkelter")}</option><option value="CleaveAdd">{t("quick.cleaveAdd")}</option><option value="Beastlord">{t("quick.beastlord")}</option></select></label>
          <div className="quick-toggle-grid">
            {(["optimalRaid", "bloodlust", "consumables", "reportDetails"] as const).map((key) => <label className="checkbox-line" key={key}><input checked={draft.analysis[key]} type="checkbox" onChange={(event) => updateAnalysis(key, event.target.checked)} /><span><strong>{t(`quick.${key}`)}</strong><small>{t(`quick.${key}Help`)}</small></span></label>)}
          </div>
          <details className="advanced-options">
            <summary>{t("quick.advanced")}</summary>
            <div className="advanced-options-body">
              <label>{t("quick.variance")}<input min={0} max={0.5} step={0.05} type="number" value={draft.varyCombatLength} onChange={(event) => update("varyCombatLength", Number(event.target.value))} /></label>
              <label className="checkbox-line"><input checked={draft.fixedTime} type="checkbox" onChange={(event) => update("fixedTime", event.target.checked)} /><span><strong>{t("quick.fixedTime")}</strong><small>{t("quick.fixedTimeHelp")}</small></span></label>
              <label>{t("quick.targetLevel")}<input min={1} max={100} type="number" value={draft.analysis.targetLevel} onChange={(event) => updateAnalysis("targetLevel", Number(event.target.value))} /></label>
              <label>{t("quick.targetRace")}<select value={draft.analysis.targetRace} onChange={(event) => updateAnalysis("targetRace", event.target.value as QuickSimRequest["analysis"]["targetRace"])}>{["humanoid", "aberration", "beast", "demon", "dragonkin", "elemental", "giant", "mechanical", "undead", "not_specified"].map((value) => <option key={value} value={value}>{t(`quick.targetRaces.${value}`)}</option>)}</select></label>
              <label>{t("quick.playerSkill")}<input min={0.1} max={1} step={0.01} type="number" value={draft.analysis.playerSkill} onChange={(event) => updateAnalysis("playerSkill", Number(event.target.value))} /></label>
              <label>{t("quick.worldLag")}<span className="input-with-unit"><input min={0} max={2000} type="number" value={draft.analysis.worldLagMs} onChange={(event) => updateAnalysis("worldLagMs", Number(event.target.value))} /><span>ms</span></span></label>
              <label>{t("quick.worldLagStddev")}<span className="input-with-unit"><input min={0} max={1000} type="number" value={draft.analysis.worldLagStddevMs} onChange={(event) => updateAnalysis("worldLagStddevMs", Number(event.target.value))} /><span>ms</span></span></label>
              <label>{t("quick.seed")}<input min={1} max={Number.MAX_SAFE_INTEGER} type="number" value={draft.analysis.seed} onChange={(event) => updateAnalysis("seed", Number(event.target.value))} /></label>
              <label>{t("quick.bloodlustTime")}<span className="input-with-unit"><input min={-3600} max={3600} type="number" value={draft.analysis.bloodlustTime} onChange={(event) => updateAnalysis("bloodlustTime", Number(event.target.value))} /><span>{t("quick.seconds")}</span></span></label>
              <label>{t("quick.bloodlustPercent")}<span className="input-with-unit"><input min={0} max={100} type="number" value={draft.analysis.bloodlustPercent} onChange={(event) => updateAnalysis("bloodlustPercent", Number(event.target.value))} /><span>%</span></span></label>
              <fieldset className="advanced-check-grid"><legend>{t("quick.raidBuffOverrides")}</legend>{(Object.keys(draft.analysis.raidBuffs) as Array<keyof QuickSimRequest["analysis"]["raidBuffs"]>).map((key) => <label className="checkbox-line" key={key}><input checked={draft.analysis.raidBuffs[key]} type="checkbox" onChange={(event) => updateRaidBuff(key, event.target.checked)} /><span><strong>{t(`quick.raidBuffs.${key}`)}</strong></span></label>)}</fieldset>
              <fieldset className="advanced-check-grid"><legend>{t("quick.consumableOverrides")}</legend>{(Object.keys(draft.analysis.consumableOptions) as Array<keyof QuickSimRequest["analysis"]["consumableOptions"]>).map((key) => <label className="checkbox-line" key={key}><input checked={draft.analysis.consumableOptions[key]} disabled={!draft.analysis.consumables} type="checkbox" onChange={(event) => updateConsumable(key, event.target.checked)} /><span><strong>{t(`quick.consumablesList.${key}`)}</strong></span></label>)}</fieldset>
              <label className="checkbox-line"><input checked={draft.analysis.reportPetsSeparately} type="checkbox" onChange={(event) => updateAnalysis("reportPetsSeparately", event.target.checked)} /><span><strong>{t("quick.reportPetsSeparately")}</strong><small>{t("quick.reportPetsSeparatelyHelp")}</small></span></label>
              <fieldset><legend>{t("quick.cpu")}</legend>{(["efficient", "balanced", "maximum"] as const).map((value) => <label className="radio-line" key={value}><input checked={draft.cpuPreset === value} name="cpu-preset" type="radio" onChange={() => update("cpuPreset", value)} />{t("quick." + value)}</label>)}</fieldset>
              <label>{t("quick.customApl")}<textarea value={draft.analysis.customApl} onChange={(event) => updateAnalysis("customApl", event.target.value)} placeholder={t("quick.customAplPlaceholder")} spellCheck={false} /><small>{t("quick.customAplHelp")}</small></label>
              <label>{t("quick.customOptions")}<textarea value={draft.analysis.customOptions} onChange={(event) => updateAnalysis("customOptions", event.target.value)} placeholder={t("quick.customOptionsPlaceholder")} spellCheck={false} /><small>{t("quick.customOptionsHelp")}</small></label>
            </div>
          </details>
        </section>
        <section className="generated-panel" aria-labelledby="generated-heading"><h2 id="generated-heading">{t("quick.generated")}</h2><p>{t("quick.generatedHelp")}</p><pre tabIndex={0}>{preview.generatedInput}</pre></section>
      </div>
      {error ? <div ref={errorRef} id="quick-validation-error" className="inline-error" role="alert" tabIndex={-1}><strong>{t("quick.errorTitle")}</strong><code>{error}</code></div> : null}
      <div className="button-row quick-actions">
        <button className="secondary-button" disabled={operation !== null} type="button" onClick={() => void refresh()}><RefreshCw aria-hidden="true" size={18} />{operation === "preview" ? t("quick.refreshing") : t("quick.refresh")}</button>
        <button className="primary-button" disabled={operation !== null} type="button" onClick={() => void start()}><Play aria-hidden="true" size={18} />{operation === "start" ? t("quick.starting") : t("quick.run")}</button>
      </div>
    </div>
  );
}
