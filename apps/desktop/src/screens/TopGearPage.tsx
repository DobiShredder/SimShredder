import { CircleStop, Download, Gem, Hammer, Play, RefreshCw, RotateCcw, ShieldCheck, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { QuickSimRequest } from "../quick";
import { EntityTooltip, itemTooltipModel } from "../tooltips";
import {
  defaultTopGearRequest,
  topGearAdvance,
  topGearCancel,
  topGearExport,
  topGearPrepare,
  topGearResult,
  topGearRetry,
  topGearStart,
  topGearStatus,
  type GearSlot,
  type ItemVariant,
  type PreparedTopGear,
  type TopGearRequest,
  type TopGearResultView,
  type TopGearSessionView,
} from "../topGear";

const terminal = new Set(["failed", "canceled"]);
type VariantDraft = {
  baseKey: string;
  gemIds: string;
  enchantId: string;
  rank: number;
  itemLevel: string;
  crest: number;
  valor: number;
  weaponKind: ItemVariant["weaponKind"];
  uniqueGroup: string;
  embellishment: boolean;
};

const emptyVariant: VariantDraft = {
  baseKey: "",
  gemIds: "",
  enchantId: "",
  rank: 0,
  itemLevel: "",
  crest: 0,
  valor: 0,
  weaponKind: "none",
  uniqueGroup: "",
  embellishment: false,
};

export function TopGearPage({ quick, initialSession, onSession, onImport }: { quick: QuickSimRequest | null; initialSession: TopGearSessionView | null; onSession: (session: TopGearSessionView) => void; onImport: () => void }) {
  const { t } = useTranslation();
  const [request, setRequest] = useState<TopGearRequest | null>(() => quick ? defaultTopGearRequest(quick) : null);
  const [preview, setPreview] = useState<PreparedTopGear | null>(null);
  const [session, setSession] = useState<TopGearSessionView | null>(initialSession);
  const [result, setResult] = useState<TopGearResultView | null>(null);
  const [draft, setDraft] = useState<VariantDraft>(emptyVariant);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exported, setExported] = useState<string | null>(null);
  const [resultFilter, setResultFilter] = useState<"all" | "pareto" | "changed">("all");
  const [resultSort, setResultSort] = useState<"rank" | "delta" | "cost">("rank");
  const [compareKeys, setCompareKeys] = useState<string[]>([]);

  const visibleRanked = useMemo(() => {
    const totalCost = (cost: Record<string, number>) => Object.values(cost).reduce((sum, amount) => sum + amount, 0);
    return [...(result?.ranked ?? [])]
      .filter((entry) => resultFilter === "all" || (resultFilter === "pareto" ? entry.paretoOptimal : entry.loadout.changedSlots > 0))
      .sort((left, right) => resultSort === "rank" ? left.rank - right.rank : resultSort === "delta" ? right.delta - left.delta : totalCost(left.loadout.cost) - totalCost(right.loadout.cost))
      .slice(0, 256);
  }, [result, resultFilter, resultSort]);
  const compared = useMemo(() => (result?.ranked ?? []).filter((entry) => compareKeys.includes(entry.loadout.key)), [compareKeys, result]);
  const itemTooltip = (variant: ItemVariant) => itemTooltipModel({
    id: variant.sourceItemId,
    slot: variant.slot,
    itemLevel: variant.simcOptions.ilevel,
    rank: variant.rank,
    gemIds: variant.gemIds,
    enchantId: variant.enchantId,
    changed: variant.changed,
  }, {
    title: t("tooltip.itemTitle", { id: variant.sourceItemId }),
    category: t("tooltip.itemCategory", { slot: t(`topGear.slot_${variant.slot}`) }),
    itemLevel: t("tooltip.itemLevel"),
    rank: t("tooltip.rank"),
    gems: t("tooltip.gems"),
    enchant: t("tooltip.enchant"),
    state: t("tooltip.state"),
    candidate: t("topGear.candidate"),
    worn: t("topGear.worn"),
    none: t("tooltip.none"),
  });

  useEffect(() => {
    if (!quick) return;
    setRequest((current) => current?.quick.source === quick.source ? current : defaultTopGearRequest(quick));
  }, [quick]);

  useEffect(() => setSession(initialSession), [initialSession]);

  const recordSession = (next: TopGearSessionView) => {
    setSession(next);
    onSession(next);
  };

  useEffect(() => {
    if (!request || preview || busy) return;
    setBusy(true);
    void topGearPrepare(request)
      .then((next) => {
        setPreview(next);
        setRequest((current) => current ? { ...current, variants: next.variants } : current);
        const base = next.variants[0];
        setDraft((current) => ({ ...current, baseKey: base?.key ?? "", rank: base?.rank ?? 0, gemIds: base?.gemIds.join("/") ?? "", enchantId: base?.enchantId ? String(base.enchantId) : "" }));
      })
      .catch((reason) => setError(String(reason)))
      .finally(() => setBusy(false));
  }, [busy, preview, request]);

  useEffect(() => {
    if (!session || session.stage === "complete" || terminal.has(session.currentJob.state)) return;
    const timer = window.setInterval(() => {
      void topGearStatus(session.id).then(recordSession).catch((reason) => setError(String(reason)));
    }, 600);
    return () => window.clearInterval(timer);
  }, [session]);

  useEffect(() => {
    if (session?.stage !== "complete" || result) return;
    void topGearResult(session.id).then(setResult).catch((reason) => setError(String(reason)));
  }, [result, session]);

  if (!quick || !request) {
    return <div className="page placeholder-page"><p className="eyebrow">{t("topGear.eyebrow")}</p><h1>{t("topGear.importFirst")}</h1><button className="primary-button" type="button" onClick={onImport}>{t("quick.goImport")}</button></div>;
  }

  const updateNumber = (field: "combinationLimit" | "lowIterations" | "highIterations" | "finalistCount", value: number) =>
    setRequest({ ...request, [field]: value });
  const updateCurrency = (kind: "balances" | "reserves", currency: string, value: number) =>
    setRequest({ ...request, [kind]: { ...request[kind], [currency]: Math.max(0, value) }, currencyConfirmedAtUnixSeconds: Math.floor(Date.now() / 1000) });

  const refresh = async () => {
    setBusy(true); setError(null); setResult(null);
    try { setPreview(await topGearPrepare(request)); } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const start = async () => {
    setBusy(true); setError(null); setResult(null);
    try {
      const next = await topGearPrepare(request);
      setPreview(next);
      recordSession(await topGearStart(request));
    } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const addVariant = () => {
    const base = request.variants.find((variant) => variant.key === draft.baseKey);
    if (!base) return;
    const gemIds = draft.gemIds.split(/[\s/,]+/).filter(Boolean).map(Number).filter(Number.isSafeInteger);
    const enchantId = draft.enchantId ? Number(draft.enchantId) : null;
    const simcOptions = { ...base.simcOptions };
    if (gemIds.length) simcOptions.gem_id = gemIds.join("/"); else if (base.gemIds.length) simcOptions.gem_id = "0"; else delete simcOptions.gem_id;
    if (enchantId) simcOptions.enchant_id = String(enchantId); else if (base.enchantId) simcOptions.enchant_id = "0"; else delete simcOptions.enchant_id;
    if (draft.itemLevel) simcOptions.ilevel = draft.itemLevel; else delete simcOptions.ilevel;
    const additionalCost = { crest: draft.crest, valor: draft.valor };
    const cost = { crest: (base.cost.crest ?? 0) + draft.crest, valor: (base.cost.valor ?? 0) + draft.valor };
    const variantStamp = Date.now();
    const actions: ItemVariant["actions"] = [...base.actions];
    const newActions: ItemVariant["actions"] = [];
    if (gemIds.join("/") !== base.gemIds.join("/")) newActions.push({ id: `gem-${variantStamp}`, label: t("topGear.gemAction"), kind: "gem", cost: {}, dependsOn: [], fromRank: null, toRank: null, slot: base.slot, sourceItemId: base.sourceItemId, simcOptionsPatch: { gem_id: gemIds.length ? gemIds.join("/") : "0" } });
    if (enchantId !== base.enchantId) newActions.push({ id: `enchant-${variantStamp}`, label: t("topGear.enchantAction"), kind: "enchant", cost: {}, dependsOn: [], fromRank: null, toRank: null, slot: base.slot, sourceItemId: base.sourceItemId, simcOptionsPatch: { enchant_id: enchantId ? String(enchantId) : "0" } });
    if (draft.rank !== base.rank) {
      if (draft.rank !== base.rank + 1 || !draft.itemLevel) {
        setError(t("topGear.rankStepError"));
        return;
      }
      const priorUpgrade = [...actions].reverse().find((action) => action.kind === "upgrade");
      newActions.push({ id: `upgrade-${base.key}-${base.rank}-${draft.rank}-${variantStamp}`, label: t("topGear.upgradeAction", { rank: draft.rank }), kind: "upgrade", cost: {}, dependsOn: priorUpgrade ? [priorUpgrade.id] : [], fromRank: base.rank, toRank: draft.rank, slot: base.slot, sourceItemId: base.sourceItemId, simcOptionsPatch: { ilevel: draft.itemLevel } });
    }
    if (!newActions.length) {
      setError(t("topGear.noVirtualChange"));
      return;
    }
    newActions[newActions.length - 1].cost = additionalCost;
    actions.push(...newActions);
    const variant: ItemVariant = {
      ...base,
      key: `virtual-${base.slot}-${base.sourceItemId}-${variantStamp}`,
      rank: draft.rank,
      gemIds,
      enchantId,
      simcOptions,
      cost,
      actions,
      uniqueGroups: draft.uniqueGroup ? [draft.uniqueGroup] : [],
      weaponKind: draft.weaponKind,
      embellishment: draft.embellishment,
      changed: true,
    };
    setRequest({ ...request, variants: [...request.variants, variant] });
    setPreview(null);
    setDraft({ ...emptyVariant, baseKey: base.key });
  };
  const sessionAction = async (kind: "cancel" | "retry") => {
    if (!session) return;
    setBusy(true); setError(null);
    try { recordSession(kind === "cancel" ? await topGearCancel(session.id) : await topGearRetry(session.id)); }
    catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const advanceSession = async () => {
    if (!session) return;
    setBusy(true); setError(null);
    try { recordSession(await topGearAdvance(session.id)); }
    catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const exportResult = async () => {
    if (!result) return;
    setBusy(true); setError(null);
    try {
      const exportedResult = await topGearExport(result.sessionId);
      setExported(t("topGear.exported", { count: exportedResult.fileCount, path: exportedResult.directory }));
    } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };

  return (
    <div className="page top-gear-page">
      <p className="eyebrow">{t("topGear.eyebrow")}</p>
      <h1>{t("topGear.title")}</h1>
      <p className="settings-lead">{t("topGear.body")}</p>

      <div className="top-gear-grid">
        <section className="settings-form" aria-labelledby="budget-heading">
          <h2 id="budget-heading"><ShieldCheck aria-hidden="true" size={18} />{t("topGear.budget")}</h2>
          {(["crest", "valor"] as const).map((currency) => <div className="currency-row" key={currency}>
            <strong>{t(`topGear.${currency}`)}</strong>
            <label>{t("topGear.balance")}<input min={0} type="number" value={request.balances[currency] ?? 0} onChange={(event) => updateCurrency("balances", currency, Number(event.target.value))} /></label>
            <label>{t("topGear.reserve")}<input min={0} type="number" value={request.reserves[currency] ?? 0} onChange={(event) => updateCurrency("reserves", currency, Number(event.target.value))} /></label>
          </div>)}
          <p className="safe-note">{t("topGear.currencyNote")}</p>
        </section>

        <section className="settings-form" aria-labelledby="precision-heading">
          <h2 id="precision-heading"><Sparkles aria-hidden="true" size={18} />{t("topGear.precision")}</h2>
          <label>{t("topGear.limit")}<input min={1} max={256} type="number" value={request.combinationLimit} onChange={(event) => updateNumber("combinationLimit", Number(event.target.value))} /></label>
          <label>{t("topGear.lowIterations")}<input min={100} type="number" value={request.lowIterations} onChange={(event) => updateNumber("lowIterations", Number(event.target.value))} /></label>
          <label>{t("topGear.highIterations")}<input min={100} type="number" value={request.highIterations} onChange={(event) => updateNumber("highIterations", Number(event.target.value))} /></label>
          <label>{t("topGear.finalists")}<input min={1} max={256} type="number" value={request.finalistCount} onChange={(event) => updateNumber("finalistCount", Number(event.target.value))} /></label>
        </section>
      </div>

      <section className="variant-builder" aria-labelledby="variant-heading">
        <div className="section-heading"><h2 id="variant-heading"><Hammer aria-hidden="true" size={18} />{t("topGear.virtualTitle")}</h2><p>{t("topGear.virtualBody")}</p></div>
        <div className="variant-form">
          <label>{t("topGear.baseItem")}<select value={draft.baseKey} onChange={(event) => { const base = request.variants.find((variant) => variant.key === event.target.value); setDraft({ ...draft, baseKey: event.target.value, rank: base?.rank ?? 0, gemIds: base?.gemIds.join("/") ?? "", enchantId: base?.enchantId ? String(base.enchantId) : "", itemLevel: "" }); }}>{request.variants.map((variant) => <option key={variant.key} value={variant.key}>{t(`topGear.slot_${variant.slot}`)} · {variant.sourceItemId} · {variant.key}</option>)}</select></label>
          <label><Gem aria-hidden="true" size={15} />{t("topGear.gems")}<input placeholder="213455/213456" value={draft.gemIds} onChange={(event) => setDraft({ ...draft, gemIds: event.target.value })} /></label>
          <label>{t("topGear.enchant")}<input inputMode="numeric" value={draft.enchantId} onChange={(event) => setDraft({ ...draft, enchantId: event.target.value })} /></label>
          <label>{t("topGear.rank")}<input min={0} type="number" value={draft.rank} onChange={(event) => setDraft({ ...draft, rank: Number(event.target.value) })} /></label>
          <label>{t("topGear.itemLevel")}<input inputMode="numeric" value={draft.itemLevel} onChange={(event) => setDraft({ ...draft, itemLevel: event.target.value })} /></label>
          <label>{t("topGear.effectiveCrest")}<input min={0} type="number" value={draft.crest} onChange={(event) => setDraft({ ...draft, crest: Number(event.target.value) })} /></label>
          <label>{t("topGear.effectiveValor")}<input min={0} type="number" value={draft.valor} onChange={(event) => setDraft({ ...draft, valor: Number(event.target.value) })} /></label>
          <label>{t("topGear.weapon")}<select value={draft.weaponKind} onChange={(event) => setDraft({ ...draft, weaponKind: event.target.value as ItemVariant["weaponKind"] })}><option value="none">—</option><option value="one_hand">{t("topGear.oneHand")}</option><option value="two_hand">{t("topGear.twoHand")}</option><option value="off_hand">{t("topGear.offHand")}</option></select></label>
          <label>{t("topGear.uniqueGroup")}<input value={draft.uniqueGroup} onChange={(event) => setDraft({ ...draft, uniqueGroup: event.target.value })} /></label>
          <label className="check-line"><input checked={draft.embellishment} type="checkbox" onChange={(event) => setDraft({ ...draft, embellishment: event.target.checked })} />{t("topGear.embellishment")}</label>
        </div>
        <button className="secondary-button" disabled={!draft.baseKey} type="button" onClick={addVariant}>{t("topGear.addVariant")}</button>
        <ul className="variant-list">{request.variants.map((variant) => <li key={variant.key}><span className="variant-identity"><EntityTooltip model={itemTooltip(variant)} /><span>{t(`topGear.slot_${variant.slot}`)} · {variant.sourceItemId}</span></span><small>{variant.changed ? t("topGear.candidate") : t("topGear.worn")} · {variant.gemIds.length} {t("topGear.gemsShort")} · {variant.enchantId ?? "—"}</small>{variant.changed ? <button type="button" className="text-button" onClick={() => { setRequest({ ...request, variants: request.variants.filter((item) => item.key !== variant.key) }); setPreview(null); }}>{t("topGear.remove")}</button> : null}</li>)}</ul>
      </section>

      {preview ? <section className="preview-card" aria-live="polite"><h2>{t("topGear.preview")}</h2><div className="metric-grid"><article><span>{t("topGear.raw")}</span><strong>{preview.rawCombinations}</strong></article><article><span>{t("topGear.valid")}</span><strong>{preview.validCombinations}</strong></article><article><span>{t("topGear.executions")}</span><strong>{preview.executionCount}</strong></article><article><span>{t("topGear.rule")}</span><strong>{preview.ruleRevision}</strong></article></div><details className="rejection-details"><summary>{t("topGear.rejections")}</summary><ul>{Object.entries(preview.rejections).map(([reason, count]) => <li key={reason}><span>{t(`topGear.rejection_${reason}`)}</span><strong>{count}</strong></li>)}</ul></details>{preview.estimated ? <p className="status-warning"><span aria-hidden="true" />{t("topGear.estimated")}</p> : null}<p className="safe-note">{preview.ruleSource}</p></section> : null}

      {session ? <section className="job-card" aria-live="polite"><div className="job-title"><div><span>{t("topGear.session", { id: session.id })}</span><strong>{t(`topGear.stage_${session.stage}`)}</strong></div><Sparkles aria-hidden="true" size={28} /></div><div className="progress-track" role="progressbar" aria-label={t("topGear.progress", { done: session.completedExecutions, total: session.totalExecutions })} aria-valuemin={0} aria-valuemax={session.totalExecutions} aria-valuenow={session.completedExecutions}><span style={{ width: `${session.totalExecutions ? session.completedExecutions / session.totalExecutions * 100 : 0}%` }} /></div><p className="muted">{t("topGear.progress", { done: session.completedExecutions, total: session.totalExecutions })}</p><div className="button-row">{["queued", "running"].includes(session.currentJob.state) ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void sessionAction("cancel")}><CircleStop aria-hidden="true" size={18} />{t("jobsPage.cancel")}</button> : null}{["failed", "canceled", "interrupted"].includes(session.currentJob.state) ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void sessionAction("retry")}><RotateCcw aria-hidden="true" size={18} />{t("jobsPage.retry")}</button> : null}{session.canAdvance ? <button className="primary-button" disabled={busy} type="button" onClick={() => void advanceSession()}>{t("topGear.continueStage")}</button> : null}</div></section> : null}

      {result ? <section className="top-gear-results"><h2>{t("topGear.results")}</h2><div className="result-controls"><label>{t("topGear.filter")}<select value={resultFilter} onChange={(event) => setResultFilter(event.target.value as typeof resultFilter)}><option value="all">{t("topGear.filterAll")}</option><option value="pareto">{t("topGear.filterPareto")}</option><option value="changed">{t("topGear.filterChanged")}</option></select></label><label>{t("topGear.sort")}<select value={resultSort} onChange={(event) => setResultSort(event.target.value as typeof resultSort)}><option value="rank">{t("topGear.sortRank")}</option><option value="delta">{t("topGear.sortDelta")}</option><option value="cost">{t("topGear.sortCost")}</option></select></label><small>{t("topGear.boundedRows", { count: visibleRanked.length })}</small></div><div className="table-scroll"><table><thead><tr><th>{t("topGear.compare")}</th><th>{t("topGear.rankLabel")}</th><th>{t("topGear.metric")}</th><th>{t("topGear.delta")}</th><th>{t("topGear.error")}</th><th>{t("topGear.cost")}</th><th>{t("topGear.flags")}</th></tr></thead><tbody>{visibleRanked.map((entry) => <tr key={entry.loadout.key}><td><input aria-label={t("topGear.compareRank", { rank: entry.rank })} checked={compareKeys.includes(entry.loadout.key)} disabled={!compareKeys.includes(entry.loadout.key) && compareKeys.length >= 2} type="checkbox" onChange={(event) => setCompareKeys(event.target.checked ? [...compareKeys, entry.loadout.key] : compareKeys.filter((key) => key !== entry.loadout.key))} /></td><td>{entry.rank}</td><td>{entry.mean.toLocaleString(undefined, { maximumFractionDigits: 1 })}</td><td>{entry.delta.toLocaleString(undefined, { signDisplay: "always", maximumFractionDigits: 1 })}</td><td>±{entry.combinedError.toLocaleString(undefined, { maximumFractionDigits: 1 })}</td><td>{Object.entries(entry.loadout.cost).map(([currency, amount]) => `${currency} ${amount}`).join(" · ") || "—"}</td><td>{[entry.equivalentToBaseline ? t("topGear.equivalent") : "", entry.paretoOptimal ? t("topGear.pareto") : ""].filter(Boolean).join(" · ")}</td></tr>)}</tbody></table></div>{compared.length === 2 ? <div className="comparison-card" role="status"><strong>{t("topGear.comparison")}</strong><span>{t("topGear.comparisonDelta", { first: compared[0].rank, second: compared[1].rank, delta: Math.abs(compared[0].mean - compared[1].mean).toLocaleString(undefined, { maximumFractionDigits: 1 }) })}</span></div> : null}</section> : null}
      {result?.actionPlan.length ? <section className="top-gear-results"><h2>{t("topGear.actionPlan")}</h2><ol className="action-plan">{result.actionPlan.map((action) => <li key={action.id}><strong>{action.label}</strong><span>{t("topGear.marginal", { gain: action.marginalGain.toLocaleString(undefined, { maximumFractionDigits: 1 }), cumulative: action.cumulativeGain.toLocaleString(undefined, { maximumFractionDigits: 1 }) })}</span><small>{t("topGear.remaining", { currencies: Object.entries(action.remaining).map(([currency, amount]) => `${currency} ${amount}`).join(" · ") })}</small></li>)}</ol></section> : null}
      {result ? <section className="generated-panel top-gear-final"><h2>{t("topGear.finalInput")}</h2><p>{t("topGear.identity", { simc: result.runtime.simc_version, revision: result.runtime.git_revision, build: result.runtime.game_build, rule: result.ruleRevision })}</p><p>{t("topGear.snapshot", { time: new Date(result.budget.confirmedAtUnixSeconds * 1000).toLocaleString() })}</p><pre tabIndex={0}>{result.finalGeneratedInput}</pre><button className="secondary-button" disabled={busy} type="button" onClick={() => void exportResult()}><Download aria-hidden="true" size={18} />{t("topGear.export")}</button>{exported ? <p role="status">{exported}</p> : null}</section> : null}

      {error ? <div className="inline-error" role="alert"><strong>{t("topGear.errorTitle")}</strong><code>{error}</code></div> : null}
      {!session ? <div className="button-row quick-actions"><button className="secondary-button" disabled={busy} type="button" onClick={() => void refresh()}><RefreshCw aria-hidden="true" size={18} />{busy ? t("quick.refreshing") : t("quick.refresh")}</button><button className="primary-button" disabled={busy || !preview} type="button" onClick={() => void start()}><Play aria-hidden="true" size={18} />{busy ? t("quick.starting") : t("topGear.run")}</button></div> : null}
    </div>
  );
}
