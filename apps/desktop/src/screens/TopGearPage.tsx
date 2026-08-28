import { CircleStop, Download, Gem, Hammer, Lock, Play, RefreshCw, RotateCcw, ShieldCheck, Sparkles, Unlock } from "lucide-react";
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
  type ProfileOptionVariant,
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
  championMistcrest: number;
  heroMistcrest: number;
  mythMistcrest: number;
  sparkOfTides: number;
  weaponKind: ItemVariant["weaponKind"];
  uniqueGroup: string;
  setGroup: string;
  embellishment: boolean;
  catalyst: boolean;
};

const emptyVariant: VariantDraft = {
  baseKey: "",
  gemIds: "",
  enchantId: "",
  rank: 0,
  itemLevel: "",
  championMistcrest: 0,
  heroMistcrest: 0,
  mythMistcrest: 0,
  sparkOfTides: 0,
  weaponKind: "none",
  uniqueGroup: "",
  setGroup: "",
  embellishment: false,
  catalyst: false,
};

const slotOrder: GearSlot[] = ["head", "neck", "shoulders", "back", "chest", "wrists", "hands", "waist", "legs", "feet", "finger1", "finger2", "trinket1", "trinket2", "main_hand", "off_hand", "shirt", "tabard"];

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
  const [setName, setSetName] = useState("");
  const [setMinimum, setSetMinimum] = useState(2);
  const [optionKind, setOptionKind] = useState("food");
  const [optionValue, setOptionValue] = useState("");
  const [itemSlot, setItemSlot] = useState<GearSlot>("head");
  const [itemId, setItemId] = useState("");
  const [itemName, setItemName] = useState("");
  const [itemOptions, setItemOptions] = useState("");

  const variantsBySlot = useMemo(() => {
    const groups = new Map<GearSlot, ItemVariant[]>();
    for (const variant of request?.variants ?? []) groups.set(variant.slot, [...(groups.get(variant.slot) ?? []), variant]);
    return slotOrder.flatMap((slot) => groups.has(slot) ? [[slot, groups.get(slot)!] as const] : []);
  }, [request?.variants]);

  const visibleRanked = useMemo(() => {
    const totalCost = (cost: Record<string, number>) => Object.values(cost).reduce((sum, amount) => sum + amount, 0);
    return [...(result?.ranked ?? [])]
      .filter((entry) => resultFilter === "all" || (resultFilter === "pareto" ? entry.paretoOptimal : entry.loadout.changedSlots > 0 || entry.loadout.changedOptions > 0))
      .sort((left, right) => resultSort === "rank" ? left.rank - right.rank : resultSort === "delta" ? right.delta - left.delta : totalCost(left.loadout.cost) - totalCost(right.loadout.cost))
      .slice(0, 256);
  }, [result, resultFilter, resultSort]);
  const compared = useMemo(() => (result?.ranked ?? []).filter((entry) => compareKeys.includes(entry.loadout.key)), [compareKeys, result]);
  const bestResult = result?.ranked[0] ?? null;
  const bestItems = bestResult
    ? slotOrder.flatMap((slot) => bestResult.loadout.items[slot] ? [bestResult.loadout.items[slot]] : [])
    : [];
  const bestIsBaseline = Boolean(bestResult && result && (bestResult.loadout.key === result.baselineKey
    || (bestResult.loadout.changedSlots === 0 && bestResult.loadout.changedOptions === 0)));
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
        setRequest((current) => current ? {
          ...current,
          variants: next.variants.map((variant) => ({ ...variant, enabled: variant.enabled ?? true, displayName: variant.displayName ?? null, setGroups: variant.setGroups ?? [], catalyst: variant.catalyst ?? false })),
          talentLoadouts: next.talentLoadouts ?? current.talentLoadouts,
          profileOptions: next.profileOptions ?? current.profileOptions,
        } : current);
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
    return <div className="page placeholder-page"><p className="eyebrow">{t("topGear.eyebrow")}</p><h1>{t("topGear.title")}</h1><p className="placeholder-description">{t("topGear.importFirst")}</p><button className="primary-button" type="button" onClick={onImport}>{t("quick.goImport")}</button></div>;
  }

  const updateNumber = (field: "combinationLimit" | "lowIterations" | "highIterations" | "finalistCount", value: number) =>
    setRequest({ ...request, [field]: value });
  const updateCurrency = (kind: "balances" | "reserves", currency: string, value: number) =>
    setRequest({ ...request, [kind]: { ...request[kind], [currency]: Math.max(0, value) }, currencyConfirmedAtUnixSeconds: Math.floor(Date.now() / 1000) });
  const updateCandidate = (key: string, enabled: boolean) => {
    setRequest({ ...request, variants: request.variants.map((variant) => variant.key === key ? { ...variant, enabled } : variant) });
    setPreview(null);
  };
  const toggleSlotLock = (slot: GearSlot) => {
    const locked = request.lockedSlots.includes(slot);
    setRequest({ ...request, lockedSlots: locked ? request.lockedSlots.filter((entry) => entry !== slot) : [...request.lockedSlots, slot] });
    setPreview(null);
  };
  const updateTalent = (key: string, enabled: boolean) => {
    setRequest({ ...request, talentLoadouts: request.talentLoadouts.map((talent) => talent.key === key ? { ...talent, enabled } : talent) });
    setPreview(null);
  };
  const updateProfileOption = (axis: string, key: string, enabled: boolean) => {
    setRequest({ ...request, profileOptions: { ...request.profileOptions, [axis]: request.profileOptions[axis].map((candidate) => candidate.key === key ? { ...candidate, enabled } : candidate) } });
    setPreview(null);
  };
  const addProfileOption = () => {
    const value = optionValue.trim();
    if (!value) return;
    const existing = request.profileOptions[optionKind] ?? [];
    if (existing.some((candidate) => candidate.value === value)) { setError(t("topGear.duplicateProfileOption")); return; }
    const candidate: ProfileOptionVariant = { key: `custom-${optionKind}-${Date.now()}`, label: value, option: optionKind, value, changed: true, enabled: true };
    setRequest({ ...request, profileOptions: { ...request.profileOptions, [optionKind]: [...existing, candidate] } });
    setOptionValue(""); setPreview(null); setError(null);
  };
  const addExactItem = () => {
    const sourceItemId = Number(itemId);
    if (!Number.isSafeInteger(sourceItemId) || sourceItemId <= 0) { setError(t("topGear.invalidItemId")); return; }
    const simcOptions: Record<string, string> = {};
    for (const token of itemOptions.split(",").map((value) => value.trim()).filter(Boolean)) {
      const [key, value, ...rest] = token.split("=");
      if (!key || !value || rest.length) { setError(t("topGear.invalidItemOptions")); return; }
      simcOptions[key] = value;
    }
    const stamp = Date.now();
    const candidate: ItemVariant = {
      key: `manual-${itemSlot}-${sourceItemId}-${stamp}`, sourceItemId, slot: itemSlot,
      displayName: itemName.trim() || null, rank: 0, gemIds: [], enchantId: null,
      simcOptions, cost: {}, actions: [{ id: `equip-${stamp}`, label: t("topGear.equipAction"), kind: "equip", cost: {}, dependsOn: [], fromRank: null, toRank: null, slot: itemSlot, sourceItemId, simcOptionsPatch: simcOptions }],
      uniqueGroups: [], setGroups: [], weaponKind: "none", embellishment: false, catalyst: false, enabled: true, changed: true,
    };
    setRequest({ ...request, variants: [...request.variants, candidate] });
    setItemId(""); setItemName(""); setItemOptions(""); setError(null); setPreview(null);
  };
  const resetSelections = () => {
    setRequest({
      ...request,
      variants: request.variants.map((variant) => ({ ...variant, enabled: !variant.changed })),
      talentLoadouts: request.talentLoadouts.map((talent) => ({ ...talent, enabled: !talent.changed })),
      profileOptions: Object.fromEntries(Object.entries(request.profileOptions).map(([axis, candidates]) => [axis, candidates.map((candidate) => ({ ...candidate, enabled: !candidate.changed }))])),
      lockedSlots: [], minimumSetPieces: {}, catalystCharges: 0,
    });
    setPreview(null);
  };

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
    const additionalCost = { champion_mistcrest: draft.championMistcrest, hero_mistcrest: draft.heroMistcrest, myth_mistcrest: draft.mythMistcrest, spark_of_tides: draft.sparkOfTides };
    const cost = { champion_mistcrest: (base.cost.champion_mistcrest ?? 0) + draft.championMistcrest, hero_mistcrest: (base.cost.hero_mistcrest ?? 0) + draft.heroMistcrest, myth_mistcrest: (base.cost.myth_mistcrest ?? 0) + draft.mythMistcrest, spark_of_tides: (base.cost.spark_of_tides ?? 0) + draft.sparkOfTides };
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
      setGroups: draft.setGroup ? [draft.setGroup] : [],
      weaponKind: draft.weaponKind,
      embellishment: draft.embellishment,
      catalyst: draft.catalyst,
      enabled: true,
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

      <nav className="optimizer-nav" aria-label={t("topGear.quickNav")}>
        <a href="#optimizer-gear">{t("topGear.navGear")}</a><a href="#optimizer-enhancements">{t("topGear.navEnhancements")}</a><a href="#optimizer-consumables">{t("topGear.navConsumables")}</a><a href="#optimizer-talents">{t("topGear.navTalents")}</a><a href="#optimizer-options">{t("topGear.navOptions")}</a>
      </nav>

      <section className="candidate-browser" id="optimizer-gear" aria-labelledby="candidate-heading">
        <div className="section-heading"><div><h2 id="candidate-heading">{t("topGear.candidateGear")}</h2><p>{t("topGear.candidateGearHelp")}</p></div><button className="text-button" type="button" onClick={resetSelections}><RotateCcw aria-hidden="true" size={15} />{t("topGear.resetSelections")}</button></div>
        <div className="slot-candidate-grid">{variantsBySlot.map(([slot, variants]) => {
          const locked = request.lockedSlots.includes(slot);
          return <article className="slot-candidate-card" key={slot}>
            <header><strong>{t(`topGear.slot_${slot}`)}</strong><button aria-label={t(locked ? "topGear.unlockSlot" : "topGear.lockSlot", { slot: t(`topGear.slot_${slot}`) })} className="icon-button" type="button" onClick={() => toggleSlotLock(slot)}>{locked ? <Lock aria-hidden="true" size={15} /> : <Unlock aria-hidden="true" size={15} />}</button></header>
            <ul>{variants.map((variant) => <li key={variant.key}><label className="candidate-check"><input checked={!variant.changed || (variant.enabled && !locked)} disabled={!variant.changed || locked} type="checkbox" onChange={(event) => updateCandidate(variant.key, event.target.checked)} /><span><strong>{variant.displayName ?? t("tooltip.itemTitle", { id: variant.sourceItemId })}</strong><small>{variant.changed ? t("topGear.candidate") : t("topGear.worn")} · {variant.simcOptions.ilevel ? t("topGear.itemLevelValue", { level: variant.simcOptions.ilevel }) : `ID ${variant.sourceItemId}`}</small></span></label></li>)}</ul>
          </article>;
        })}</div>
        <details className="inline-disclosure"><summary>{t("topGear.addExactItem")}</summary><div className="exact-item-form"><label>{t("topGear.itemSlot")}<select value={itemSlot} onChange={(event) => setItemSlot(event.target.value as GearSlot)}>{slotOrder.filter((slot) => !["shirt", "tabard"].includes(slot)).map((slot) => <option key={slot} value={slot}>{t(`topGear.slot_${slot}`)}</option>)}</select></label><label>{t("topGear.itemId")}<input inputMode="numeric" value={itemId} onChange={(event) => setItemId(event.target.value)} /></label><label>{t("topGear.itemNameOptional")}<input value={itemName} onChange={(event) => setItemName(event.target.value)} /></label><label>{t("topGear.itemOptions")}<input placeholder="bonus_id=… , context=…" value={itemOptions} onChange={(event) => setItemOptions(event.target.value)} /></label><button className="secondary-button" disabled={!itemId} type="button" onClick={addExactItem}>{t("topGear.addItemCandidate")}</button></div><small>{t("topGear.itemSearchBoundary")}</small></details>
      </section>

      <div className="top-gear-grid">
        <section className="settings-form" aria-labelledby="set-heading">
          <h2 id="set-heading"><ShieldCheck aria-hidden="true" size={18} />{t("topGear.setAndCatalyst")}</h2>
          <label>{t("topGear.catalystCharges")}<input min={0} max={6} type="number" value={request.catalystCharges} onChange={(event) => { setRequest({ ...request, catalystCharges: Math.max(0, Number(event.target.value)) }); setPreview(null); }} /></label>
          <div className="inline-fields"><label>{t("topGear.setGroup")}<input value={setName} onChange={(event) => setSetName(event.target.value)} /></label><label>{t("topGear.minimumPieces")}<input min={1} max={8} type="number" value={setMinimum} onChange={(event) => setSetMinimum(Number(event.target.value))} /></label><button className="secondary-button" disabled={!setName.trim()} type="button" onClick={() => { setRequest({ ...request, minimumSetPieces: { ...request.minimumSetPieces, [setName.trim()]: setMinimum } }); setSetName(""); setPreview(null); }}>{t("topGear.addConstraint")}</button></div>
          <ul className="constraint-list">{Object.entries(request.minimumSetPieces).map(([group, minimum]) => <li key={group}><span>{group} · {t("topGear.pieces", { count: minimum })}</span><button className="text-button" type="button" onClick={() => { const next = { ...request.minimumSetPieces }; delete next[group]; setRequest({ ...request, minimumSetPieces: next }); setPreview(null); }}>{t("topGear.remove")}</button></li>)}</ul>
          <small>{t("topGear.setMetadataHelp")}</small>
        </section>

        <section className="settings-form" id="optimizer-talents" aria-labelledby="talents-heading">
          <h2 id="talents-heading"><Sparkles aria-hidden="true" size={18} />{t("topGear.talentLoadouts")}</h2>
          {request.talentLoadouts.map((talent) => <label className="checkbox-line" key={talent.key}><input checked={talent.enabled} disabled={!talent.changed} type="checkbox" onChange={(event) => updateTalent(talent.key, event.target.checked)} /><span><strong>{talent.changed ? talent.label : t("topGear.activeTalents")}</strong><small>{talent.changed ? t("topGear.savedTalent") : t("topGear.activeTalentHelp")}</small></span></label>)}
          <small>{t("topGear.talentHelp")}</small>
        </section>
      </div>

      <section className="variant-builder" id="optimizer-consumables" aria-labelledby="profile-options-heading">
        <div className="section-heading"><h2 id="profile-options-heading">{t("topGear.profileOptions")}</h2><p>{t("topGear.profileOptionsHelp")}</p></div>
        <div className="profile-option-groups">{Object.entries(request.profileOptions).map(([axis, candidates]) => <fieldset key={axis}><legend>{t(`topGear.option_${axis}`)}</legend>{candidates.map((candidate) => <label className="checkbox-line" key={candidate.key}><input checked={candidate.enabled} disabled={!candidate.changed} type="checkbox" onChange={(event) => updateProfileOption(axis, candidate.key, event.target.checked)} /><span><strong>{candidate.changed ? candidate.label : t("topGear.profileDefault")}</strong><small>{candidate.value || t("topGear.noOverride")}</small></span></label>)}</fieldset>)}</div>
        <div className="inline-fields profile-option-add"><label>{t("topGear.optionType")}<select value={optionKind} onChange={(event) => setOptionKind(event.target.value)}>{Object.keys(request.profileOptions).map((axis) => <option key={axis} value={axis}>{t(`topGear.option_${axis}`)}</option>)}</select></label><label>{t("topGear.simcValue")}<input placeholder={t("topGear.simcValuePlaceholder")} value={optionValue} onChange={(event) => setOptionValue(event.target.value)} /></label><button className="secondary-button" disabled={!optionValue.trim()} type="button" onClick={addProfileOption}>{t("topGear.addOption")}</button></div>
      </section>

      <div className="top-gear-grid">
        <section className="settings-form" aria-labelledby="budget-heading">
          <h2 id="budget-heading"><ShieldCheck aria-hidden="true" size={18} />{t("topGear.budget")}</h2>
          {(["champion_mistcrest", "hero_mistcrest", "myth_mistcrest", "spark_of_tides"] as const).map((currency) => <div className="currency-row" key={currency}>
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

      <details className="advanced-options" id="optimizer-options">
        <summary><span><strong>{t("topGear.simulationOptions")}</strong><small>{t("topGear.simulationSummary", { style: request.quick.fightStyle, targets: request.quick.desiredTargets, time: request.quick.maxTimeSeconds })}</small></span></summary>
        <div className="advanced-options-body"><div className="advanced-grid">
          <label>{t("quick.fightLength")}<input min={10} max={3600} type="number" value={request.quick.maxTimeSeconds} onChange={(event) => { setRequest({ ...request, quick: { ...request.quick, maxTimeSeconds: Number(event.target.value) } }); setPreview(null); }} /></label>
          <label>{t("quick.targets")}<input min={1} max={100} type="number" value={request.quick.desiredTargets} onChange={(event) => { setRequest({ ...request, quick: { ...request.quick, desiredTargets: Number(event.target.value) } }); setPreview(null); }} /></label>
          <label>{t("quick.fightStyle")}<select value={request.quick.fightStyle} onChange={(event) => { setRequest({ ...request, quick: { ...request.quick, fightStyle: event.target.value as QuickSimRequest["fightStyle"] } }); setPreview(null); }}>{["Patchwerk", "CastingPatchwerk", "DungeonSlice", "HecticAddCleave", "LightMovement", "HeavyMovement", "HelterSkelter", "CleaveAdd", "Beastlord"].map((style) => <option key={style} value={style}>{style}</option>)}</select></label>
          <label>{t("quick.variance")}<input min={0} max={100} step={1} type="number" value={request.quick.varyCombatLength * 100} onChange={(event) => { setRequest({ ...request, quick: { ...request.quick, varyCombatLength: Number(event.target.value) / 100 } }); setPreview(null); }} /></label>
        </div></div>
      </details>

      <section className="variant-builder" id="optimizer-enhancements" aria-labelledby="variant-heading">
        <div className="section-heading"><h2 id="variant-heading"><Hammer aria-hidden="true" size={18} />{t("topGear.virtualTitle")}</h2><p>{t("topGear.virtualBody")}</p></div>
        <div className="variant-form">
          <label>{t("topGear.baseItem")}<select value={draft.baseKey} onChange={(event) => { const base = request.variants.find((variant) => variant.key === event.target.value); setDraft({ ...draft, baseKey: event.target.value, rank: base?.rank ?? 0, gemIds: base?.gemIds.join("/") ?? "", enchantId: base?.enchantId ? String(base.enchantId) : "", itemLevel: "" }); }}>{request.variants.map((variant) => <option key={variant.key} value={variant.key}>{t(`topGear.slot_${variant.slot}`)} · {variant.sourceItemId} · {variant.key}</option>)}</select></label>
          <label><Gem aria-hidden="true" size={15} />{t("topGear.gems")}<input placeholder="213455/213456" value={draft.gemIds} onChange={(event) => setDraft({ ...draft, gemIds: event.target.value })} /></label>
          <label>{t("topGear.enchant")}<input inputMode="numeric" value={draft.enchantId} onChange={(event) => setDraft({ ...draft, enchantId: event.target.value })} /></label>
          <label>{t("topGear.rank")}<input min={0} type="number" value={draft.rank} onChange={(event) => setDraft({ ...draft, rank: Number(event.target.value) })} /></label>
          <label>{t("topGear.itemLevel")}<input inputMode="numeric" value={draft.itemLevel} onChange={(event) => setDraft({ ...draft, itemLevel: event.target.value })} /></label>
          <label>{t("topGear.effectiveChampionMistcrest")}<input min={0} type="number" value={draft.championMistcrest} onChange={(event) => setDraft({ ...draft, championMistcrest: Number(event.target.value) })} /></label>
          <label>{t("topGear.effectiveHeroMistcrest")}<input min={0} type="number" value={draft.heroMistcrest} onChange={(event) => setDraft({ ...draft, heroMistcrest: Number(event.target.value) })} /></label>
          <label>{t("topGear.effectiveMythMistcrest")}<input min={0} type="number" value={draft.mythMistcrest} onChange={(event) => setDraft({ ...draft, mythMistcrest: Number(event.target.value) })} /></label>
          <label>{t("topGear.effectiveSparkOfTides")}<input min={0} type="number" value={draft.sparkOfTides} onChange={(event) => setDraft({ ...draft, sparkOfTides: Number(event.target.value) })} /></label>
          <label>{t("topGear.weapon")}<select value={draft.weaponKind} onChange={(event) => setDraft({ ...draft, weaponKind: event.target.value as ItemVariant["weaponKind"] })}><option value="none">—</option><option value="one_hand">{t("topGear.oneHand")}</option><option value="two_hand">{t("topGear.twoHand")}</option><option value="off_hand">{t("topGear.offHand")}</option></select></label>
          <label>{t("topGear.uniqueGroup")}<input value={draft.uniqueGroup} onChange={(event) => setDraft({ ...draft, uniqueGroup: event.target.value })} /></label>
          <label>{t("topGear.setGroup")}<input value={draft.setGroup} onChange={(event) => setDraft({ ...draft, setGroup: event.target.value })} /></label>
          <label className="check-line"><input checked={draft.embellishment} type="checkbox" onChange={(event) => setDraft({ ...draft, embellishment: event.target.checked })} />{t("topGear.embellishment")}</label>
          <label className="check-line"><input checked={draft.catalyst} type="checkbox" onChange={(event) => setDraft({ ...draft, catalyst: event.target.checked })} />{t("topGear.usesCatalyst")}</label>
        </div>
        <button className="secondary-button" disabled={!draft.baseKey} type="button" onClick={addVariant}>{t("topGear.addVariant")}</button>
        <ul className="variant-list">{request.variants.map((variant) => <li key={variant.key}><span className="variant-identity"><EntityTooltip model={itemTooltip(variant)} /><span>{t(`topGear.slot_${variant.slot}`)} · {variant.sourceItemId}</span></span><small>{variant.changed ? t("topGear.candidate") : t("topGear.worn")} · {variant.gemIds.length} {t("topGear.gemsShort")} · {variant.enchantId ?? "—"}</small>{variant.changed ? <button type="button" className="text-button" onClick={() => { setRequest({ ...request, variants: request.variants.filter((item) => item.key !== variant.key) }); setPreview(null); }}>{t("topGear.remove")}</button> : null}</li>)}</ul>
      </section>

      {preview ? <section className="preview-card" aria-live="polite"><h2>{t("topGear.preview")}</h2><div className="metric-grid"><article><span>{t("topGear.raw")}</span><strong>{preview.rawCombinations}</strong></article><article><span>{t("topGear.valid")}</span><strong>{preview.validCombinations}</strong></article><article><span>{t("topGear.executions")}</span><strong>{preview.executionCount}</strong></article><article><span>{t("topGear.rule")}</span><strong>{preview.ruleRevision}</strong></article></div><details className="rejection-details"><summary>{t("topGear.rejections")}</summary><ul>{Object.entries(preview.rejections).map(([reason, count]) => <li key={reason}><span>{t(`topGear.rejection_${reason}`)}</span><strong>{count}</strong></li>)}</ul></details>{preview.estimated ? <p className="status-warning"><span aria-hidden="true" />{t("topGear.estimated")}</p> : null}<p className="safe-note">{preview.ruleSource}</p></section> : null}

      {session ? <section className="job-card" aria-live="polite"><div className="job-title"><div><span>{t("topGear.session", { id: session.id })}</span><strong>{t(`topGear.stage_${session.stage}`)}</strong></div><Sparkles aria-hidden="true" size={28} /></div><div className="progress-track" role="progressbar" aria-label={t("topGear.progress", { done: session.completedExecutions, total: session.totalExecutions })} aria-valuemin={0} aria-valuemax={session.totalExecutions} aria-valuenow={session.completedExecutions}><span style={{ width: `${session.totalExecutions ? session.completedExecutions / session.totalExecutions * 100 : 0}%` }} /></div><p className="muted">{t("topGear.progress", { done: session.completedExecutions, total: session.totalExecutions })}</p><div className="button-row">{["queued", "running"].includes(session.currentJob.state) ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void sessionAction("cancel")}><CircleStop aria-hidden="true" size={18} />{t("jobsPage.cancel")}</button> : null}{["failed", "canceled", "interrupted"].includes(session.currentJob.state) ? <button className="secondary-button" disabled={busy} type="button" onClick={() => void sessionAction("retry")}><RotateCcw aria-hidden="true" size={18} />{t("jobsPage.retry")}</button> : null}{session.canAdvance ? <button className="primary-button" disabled={busy} type="button" onClick={() => void advanceSession()}>{t("topGear.continueStage")}</button> : null}</div></section> : null}

      {bestResult ? <section className="top-gear-results best-loadout" aria-labelledby="best-loadout-heading">
        <div className="best-loadout-heading">
          <div><p className="eyebrow">{t("topGear.bestCombination")}</p><h2 id="best-loadout-heading">{t(bestIsBaseline ? "topGear.bestBaseline" : bestResult.equivalentToBaseline ? "topGear.bestSidegrade" : "topGear.bestUpgrade")}</h2></div>
          <div className="best-loadout-metric"><strong>{bestResult.mean.toLocaleString(undefined, { maximumFractionDigits: 1 })}</strong><span>{t("topGear.dps")}</span></div>
        </div>
        <div className="best-loadout-stats"><span>{t("topGear.bestDelta", { delta: bestResult.delta.toLocaleString(undefined, { signDisplay: "always", maximumFractionDigits: 1 }) })}</span><span>{t("topGear.bestError", { error: bestResult.combinedError.toLocaleString(undefined, { maximumFractionDigits: 1 }) })}</span><span>{t("topGear.bestChanges", { gear: bestResult.loadout.changedSlots, options: bestResult.loadout.changedOptions })}</span></div>
        {bestItems.length ? <ul className="best-loadout-items">{bestItems.map((item) => <li key={`${item.slot}-${item.key}`}><EntityTooltip model={itemTooltip(item)} /><span><strong>{item.displayName ?? t("tooltip.itemTitle", { id: item.sourceItemId })}</strong><small>{t(`topGear.slot_${item.slot}`)} · {item.simcOptions.ilevel ? t("topGear.itemLevelValue", { level: item.simcOptions.ilevel }) : `ID ${item.sourceItemId}`} · {item.changed ? t("topGear.candidate") : t("topGear.worn")}</small></span></li>)}</ul> : null}
        <p className="safe-note">{t("topGear.bestInterpretation")}</p>
      </section> : null}

      {result ? <section className="top-gear-results"><h2>{t("topGear.results")}</h2><div className="result-controls"><label>{t("topGear.filter")}<select value={resultFilter} onChange={(event) => setResultFilter(event.target.value as typeof resultFilter)}><option value="all">{t("topGear.filterAll")}</option><option value="pareto">{t("topGear.filterPareto")}</option><option value="changed">{t("topGear.filterChanged")}</option></select></label><label>{t("topGear.sort")}<select value={resultSort} onChange={(event) => setResultSort(event.target.value as typeof resultSort)}><option value="rank">{t("topGear.sortRank")}</option><option value="delta">{t("topGear.sortDelta")}</option><option value="cost">{t("topGear.sortCost")}</option></select></label><small>{t("topGear.boundedRows", { count: visibleRanked.length })}</small></div><div className="table-scroll"><table><thead><tr><th>{t("topGear.compare")}</th><th>{t("topGear.rankLabel")}</th><th>{t("topGear.metric")}</th><th>{t("topGear.delta")}</th><th>{t("topGear.error")}</th><th>{t("topGear.cost")}</th><th>{t("topGear.flags")}</th></tr></thead><tbody>{visibleRanked.map((entry) => <tr key={entry.loadout.key}><td><input aria-label={t("topGear.compareRank", { rank: entry.rank })} checked={compareKeys.includes(entry.loadout.key)} disabled={!compareKeys.includes(entry.loadout.key) && compareKeys.length >= 2} type="checkbox" onChange={(event) => setCompareKeys(event.target.checked ? [...compareKeys, entry.loadout.key] : compareKeys.filter((key) => key !== entry.loadout.key))} /></td><td>{entry.rank}</td><td>{entry.mean.toLocaleString(undefined, { maximumFractionDigits: 1 })}</td><td>{entry.delta.toLocaleString(undefined, { signDisplay: "always", maximumFractionDigits: 1 })}</td><td>±{entry.combinedError.toLocaleString(undefined, { maximumFractionDigits: 1 })}</td><td>{Object.entries(entry.loadout.cost).map(([currency, amount]) => `${currency} ${amount}`).join(" · ") || "—"}</td><td>{[entry.equivalentToBaseline ? t("topGear.equivalent") : "", entry.paretoOptimal ? t("topGear.pareto") : ""].filter(Boolean).join(" · ")}</td></tr>)}</tbody></table></div>{compared.length === 2 ? <div className="comparison-card" role="status"><strong>{t("topGear.comparison")}</strong><span>{t("topGear.comparisonDelta", { first: compared[0].rank, second: compared[1].rank, delta: Math.abs(compared[0].mean - compared[1].mean).toLocaleString(undefined, { maximumFractionDigits: 1 }) })}</span></div> : null}</section> : null}
      {result?.actionPlan.length ? <section className="top-gear-results"><h2>{t("topGear.actionPlan")}</h2><ol className="action-plan">{result.actionPlan.map((action) => <li key={action.id}><strong>{action.label}</strong><span>{t("topGear.marginal", { gain: action.marginalGain.toLocaleString(undefined, { maximumFractionDigits: 1 }), cumulative: action.cumulativeGain.toLocaleString(undefined, { maximumFractionDigits: 1 }) })}</span><small>{t("topGear.remaining", { currencies: Object.entries(action.remaining).map(([currency, amount]) => `${currency} ${amount}`).join(" · ") })}</small></li>)}</ol></section> : null}
      {result ? <section className="generated-panel top-gear-final"><h2>{t("topGear.finalInput")}</h2><p>{t("topGear.identity", { simc: result.runtime.simc_version, revision: result.runtime.git_revision, build: result.runtime.game_build, rule: result.ruleRevision })}</p><p>{t("topGear.snapshot", { time: new Date(result.budget.confirmedAtUnixSeconds * 1000).toLocaleString() })}</p><pre tabIndex={0}>{result.finalGeneratedInput}</pre><button className="secondary-button" disabled={busy} type="button" onClick={() => void exportResult()}><Download aria-hidden="true" size={18} />{t("topGear.export")}</button>{exported ? <p role="status">{exported}</p> : null}</section> : null}

      {error ? <div className="inline-error" role="alert"><strong>{t("topGear.errorTitle")}</strong><code>{error}</code></div> : null}
      {!session ? <div className="button-row quick-actions"><button className="secondary-button" disabled={busy} type="button" onClick={() => void refresh()}><RefreshCw aria-hidden="true" size={18} />{busy ? t("quick.refreshing") : t("quick.refresh")}</button><button className="primary-button" disabled={busy || !preview} type="button" onClick={() => void start()}><Play aria-hidden="true" size={18} />{busy ? t("quick.starting") : t("topGear.run")}</button></div> : null}
    </div>
  );
}
