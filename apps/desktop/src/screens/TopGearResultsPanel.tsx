import { Download } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { EntityTooltip, itemTooltipModel } from "../tooltips";
import { topGearExport, type GearSlot, type ItemVariant, type RankedLoadout, type TopGearResultView } from "../topGear";

const slotOrder: GearSlot[] = ["head", "neck", "shoulders", "back", "chest", "wrists", "hands", "waist", "legs", "feet", "finger1", "finger2", "trinket1", "trinket2", "main_hand", "off_hand", "shirt", "tabard"];
const totalCost = (entry: RankedLoadout) => Object.values(entry.loadout.cost).reduce((sum, amount) => sum + amount, 0);

export function TopGearResultsPanel({ result, picker }: { result: TopGearResultView; picker: ReactNode }) {
  const { t, i18n } = useTranslation();
  const [filter, setFilter] = useState<"all" | "pareto" | "changed">("all");
  const [sort, setSort] = useState<"rank" | "delta" | "cost">("rank");
  const [referenceKey, setReferenceKey] = useState(result.baselineKey);
  const [exporting, setExporting] = useState(false);
  const [exported, setExported] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setReferenceKey(result.baselineKey), [result.baselineKey, result.sessionId]);

  const visible = useMemo(() => [...result.ranked]
    .filter((entry) => filter === "all" || (filter === "pareto" ? entry.paretoOptimal : entry.loadout.changedSlots > 0 || entry.loadout.changedOptions > 0))
    .sort((left, right) => sort === "rank" ? left.rank - right.rank : sort === "delta" ? right.delta - left.delta : totalCost(left) - totalCost(right))
    .slice(0, 256), [filter, result.ranked, sort]);
  const best = result.ranked[0] ?? null;
  const reference = result.ranked.find((entry) => entry.loadout.key === referenceKey)
    ?? result.ranked.find((entry) => entry.loadout.key === result.baselineKey)
    ?? best;
  const baseline = Boolean(best && (best.loadout.key === result.baselineKey || (best.loadout.changedSlots === 0 && best.loadout.changedOptions === 0)));
  const number = (value: number) => value.toLocaleString(i18n.language, { maximumFractionDigits: 1 });
  const signed = (value: number) => value.toLocaleString(i18n.language, { signDisplay: "always", maximumFractionDigits: 1 });
  const relative = (entry: RankedLoadout) => {
    const delta = reference ? entry.mean - reference.mean : entry.delta;
    const percent = reference?.mean ? delta / reference.mean * 100 : 0;
    const combinedError = reference ? Math.hypot(entry.meanError, reference.meanError) : entry.combinedError;
    return { delta, percent, combinedError, equivalent: Math.abs(delta) <= combinedError };
  };
  const items = (entry: RankedLoadout | null) => entry
    ? slotOrder.flatMap((slot) => entry.loadout.items[slot] ? [entry.loadout.items[slot]!] : [])
    : [];
  const tooltip = (item: ItemVariant) => itemTooltipModel({
    id: item.sourceItemId, slot: item.slot, itemLevel: item.simcOptions.ilevel, rank: item.rank,
    gemIds: item.gemIds, enchantId: item.enchantId, changed: item.changed,
  }, {
    title: item.displayName ?? t("tooltip.itemTitle", { id: item.sourceItemId }),
    category: t("tooltip.itemCategory", { slot: t(`topGear.slot_${item.slot}`) }),
    itemLevel: t("tooltip.itemLevel"), rank: t("tooltip.rank"), gems: t("tooltip.gems"),
    enchant: t("tooltip.enchant"), state: t("tooltip.state"), candidate: t("topGear.candidate"),
    worn: t("topGear.worn"), none: t("tooltip.none"),
  });

  return <div className="page results-page top-gear-result-page">
    <p className="eyebrow">{t("topGear.eyebrow")}</p><h1>{t("topGear.resultTitle")}</h1>{picker}
    {best ? <section className="top-gear-results best-loadout"><div className="best-loadout-heading"><div><p className="eyebrow">{t("topGear.bestCombination")}</p><h2>{t(baseline ? "topGear.bestBaseline" : best.equivalentToBaseline ? "topGear.bestSidegrade" : "topGear.bestUpgrade")}</h2></div><div className="best-loadout-metric"><strong>{number(best.mean)}</strong><span>{t("topGear.dps")}</span></div></div><div className="best-loadout-stats"><span>{t("topGear.bestDelta", { delta: signed(best.delta) })}</span><span>{t("topGear.bestError", { error: number(best.combinedError) })}</span><span>{t("topGear.bestChanges", { gear: best.loadout.changedSlots, options: best.loadout.changedOptions })}</span>{result.enhancementPolicy === "budget_constrained" ? <span>{t("topGear.budgetRemaining", { currencies: Object.entries(result.budget.balances).map(([currency, balance]) => `${currency} ${Math.max(0, balance - (result.budget.reserves[currency] ?? 0) - (best.loadout.cost[currency] ?? 0))}`).join(" · ") })}</span> : null}</div><LoadoutItems entry={best} items={items(best)} tooltip={tooltip} /></section> : null}
    {reference ? <section className="top-gear-results selected-loadout" aria-live="polite"><div className="section-heading"><div><p className="eyebrow">{t("topGear.comparisonReference")}</p><h2>{t("topGear.selectedRank", { rank: reference.rank })}</h2><p>{t("topGear.referenceHelp")}</p></div><strong>{number(reference.mean)} {t("topGear.dps")}</strong></div><LoadoutItems entry={reference} items={items(reference)} tooltip={tooltip} /></section> : null}
    <section className="top-gear-results"><h2>{t("topGear.results")}</h2><div className="result-controls"><label>{t("topGear.filter")}<select value={filter} onChange={(event) => setFilter(event.target.value as typeof filter)}><option value="all">{t("topGear.filterAll")}</option><option value="pareto">{t("topGear.filterPareto")}</option><option value="changed">{t("topGear.filterChanged")}</option></select></label><label>{t("topGear.sort")}<select value={sort} onChange={(event) => setSort(event.target.value as typeof sort)}><option value="rank">{t("topGear.sortRank")}</option><option value="delta">{t("topGear.sortDelta")}</option><option value="cost">{t("topGear.sortCost")}</option></select></label><small>{t("topGear.boundedRows", { count: visible.length })}</small></div><div className="table-scroll"><table className="ranking-table"><thead><tr><th>{t("topGear.rankLabel")}</th><th>{t("topGear.metric")}</th><th>{t("topGear.referenceDelta")}</th><th>{t("topGear.referencePercent")}</th><th>{t("topGear.error")}</th><th>{t("topGear.cost")}</th><th>{t("topGear.flags")}</th></tr></thead><tbody>{visible.map((entry) => {
      const comparison = relative(entry);
      const selected = reference?.loadout.key === entry.loadout.key;
      return <tr className={selected ? "ranking-row-selected" : undefined} key={entry.loadout.key} onClick={() => setReferenceKey(entry.loadout.key)}><td><button className="rank-reference-button" aria-label={t("topGear.useAsReference", { rank: entry.rank })} aria-pressed={selected} type="button" onClick={() => setReferenceKey(entry.loadout.key)}>{entry.rank}</button></td><td>{number(entry.mean)}</td><td>{signed(comparison.delta)}</td><td>{signed(comparison.percent)}%</td><td>±{number(comparison.combinedError)}</td><td>{Object.entries(entry.loadout.cost).map(([currency, amount]) => `${currency} ${amount}`).join(" · ") || "—"}</td><td>{[selected ? t("topGear.reference") : "", comparison.equivalent && !selected ? t("topGear.equivalent") : "", entry.paretoOptimal ? t("topGear.pareto") : ""].filter(Boolean).join(" · ")}</td></tr>;
    })}</tbody></table></div></section>
    {result.actionPlan.length ? <section className="top-gear-results"><h2>{t("topGear.actionPlan")}</h2><ol className="action-plan">{result.actionPlan.map((action) => <li key={action.id}><strong>{action.label}</strong><span>{t("topGear.marginal", { gain: number(action.marginalGain), cumulative: number(action.cumulativeGain) })}</span><small>{t("topGear.remaining", { currencies: Object.entries(action.remaining).map(([currency, amount]) => `${currency} ${amount}`).join(" · ") })}</small></li>)}</ol></section> : null}
    <section className="generated-panel top-gear-final"><h2>{t("topGear.finalInput")}</h2><p>{t("topGear.identity", { simc: result.runtime.simc_version, revision: result.runtime.git_revision, build: result.runtime.game_build, rule: result.ruleRevision })}</p><p>{t("topGear.snapshot", { time: new Date(result.budget.confirmedAtUnixSeconds * 1000).toLocaleString(i18n.language) })}</p><pre tabIndex={0}>{result.finalGeneratedInput}</pre><button className="secondary-button" disabled={exporting} type="button" onClick={() => { setExporting(true); setError(null); void topGearExport(result.sessionId).then((value) => setExported(t("topGear.exported", { count: value.fileCount, path: value.directory }))).catch((reason) => setError(String(reason))).finally(() => setExporting(false)); }}><Download aria-hidden="true" size={18} />{t("topGear.export")}</button>{exported ? <p role="status">{exported}</p> : null}{error ? <div className="inline-error" role="alert"><code>{error}</code></div> : null}</section>
  </div>;
}

function LoadoutItems({ entry, items, tooltip }: { entry: RankedLoadout; items: ItemVariant[]; tooltip: (item: ItemVariant) => ReturnType<typeof itemTooltipModel> }) {
  const { t } = useTranslation();
  const options = Object.values(entry.loadout.profileOptions);
  return <div className="loadout-detail"><div><h3>{t("topGear.equipment")}</h3>{items.length ? <ul className="best-loadout-items">{items.map((item) => <li className={item.changed ? "loadout-item-changed" : undefined} key={`${item.slot}-${item.key}`}><EntityTooltip model={tooltip(item)} /><span><span className="loadout-item-heading"><strong>{item.displayName ?? t("tooltip.itemTitle", { id: item.sourceItemId })}</strong>{item.changed ? <span className="loadout-change-badge">{t("topGear.changedFromWorn")}</span> : null}</span><small>{t(`topGear.slot_${item.slot}`)} · {item.simcOptions.ilevel ? t("topGear.itemLevelValue", { level: item.simcOptions.ilevel }) : `ID ${item.sourceItemId}`} · {item.changed ? t("topGear.candidate") : t("topGear.worn")}{item.upgrade && item.rank !== item.upgrade.currentRank ? ` · ${t("topGear.rankChange", { current: item.upgrade.currentRank, target: item.rank })}` : ""}</small></span></li>)}</ul> : <p className="muted">{t("topGear.noEquipmentDetails")}</p>}</div><div className="loadout-options"><h3>{t("topGear.talentsAndOptions")}</h3><p><strong>{t("topGear.talent")}</strong><span>{entry.loadout.talent.label}</span></p>{options.map((option) => <p key={`${option.option}-${option.key}`}><strong>{option.option}</strong><span>{option.label}</span></p>)}<p><strong>{t("topGear.cost")}</strong><span>{Object.entries(entry.loadout.cost).map(([currency, amount]) => `${currency} ${amount}`).join(" · ") || "—"}</span></p></div></div>;
}
