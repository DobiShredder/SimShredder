import { Gem, Hammer, Lock, Play, RefreshCw, RotateCcw, ShieldCheck, Sparkles, Unlock } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { QuickSimRequest } from "../quick";
import { EntityTooltip, itemTooltipModel } from "../tooltips";
import {
  defaultTopGearRequest,
  topGearPrepare,
  topGearStart,
  type GearSlot,
  type ItemVariant,
  type ProfileOptionVariant,
  type PreparedTopGear,
  type TopGearRequest,
  type TopGearSessionView,
} from "../topGear";

type VariantDraft = {
  baseKey: string;
  gemIds: string;
  enchantId: string;
  rank: number;
  itemLevel: string;
  adventurerMistcrest: number;
  veteranMistcrest: number;
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
  adventurerMistcrest: 0,
  veteranMistcrest: 0,
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
type CandidateSlot = Exclude<GearSlot, "finger1" | "finger2" | "trinket1" | "trinket2"> | "finger" | "trinket";
const candidateSlotOrder: CandidateSlot[] = ["head", "neck", "shoulders", "back", "chest", "wrists", "hands", "waist", "legs", "feet", "finger", "trinket", "main_hand", "off_hand", "shirt", "tabard"];
const candidateSlot = (slot: GearSlot): CandidateSlot => slot === "finger1" || slot === "finger2" ? "finger" : slot === "trinket1" || slot === "trinket2" ? "trinket" : slot;
const memberSlots = (slot: CandidateSlot): GearSlot[] => slot === "finger" ? ["finger1", "finger2"] : slot === "trinket" ? ["trinket1", "trinket2"] : [slot];
const budgetCurrencies = ["adventurer_mistcrest", "veteran_mistcrest", "champion_mistcrest", "hero_mistcrest", "myth_mistcrest", "spark_of_tides"] as const;
const unlimitedResourceValue = 99_999;
const nonNegativeInteger = (value: number) => Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
const candidateOrigin = (variant: ItemVariant) => variant.key.startsWith("worn-")
  ? "equipped"
  : variant.key.startsWith("bag-")
    ? "bag"
    : variant.key.startsWith("virtual-")
      ? "virtual"
      : "manual";

export function TopGearPage({ quick, onStarted, onImport }: { quick: QuickSimRequest | null; onStarted: (session: TopGearSessionView) => void; onImport: () => void }) {
  const { t } = useTranslation();
  const [request, setRequest] = useState<TopGearRequest | null>(() => quick ? defaultTopGearRequest(quick) : null);
  const [preview, setPreview] = useState<PreparedTopGear | null>(null);
  const [draft, setDraft] = useState<VariantDraft>(emptyVariant);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [setName, setSetName] = useState("");
  const [setMinimum, setSetMinimum] = useState(2);
  const [optionKind, setOptionKind] = useState("food");
  const [optionValue, setOptionValue] = useState("");
  const [omniumValue, setOmniumValue] = useState("");
  const [itemSlot, setItemSlot] = useState<GearSlot>("head");
  const [itemId, setItemId] = useState("");
  const [itemName, setItemName] = useState("");
  const [itemOptions, setItemOptions] = useState("");

  const variantsBySlot = useMemo(() => {
    const groups = new Map<CandidateSlot, ItemVariant[]>();
    for (const variant of request?.variants ?? []) {
      const slot = candidateSlot(variant.slot);
      groups.set(slot, [...(groups.get(slot) ?? []), variant]);
    }
    return candidateSlotOrder.flatMap((slot) => groups.has(slot) ? [[slot, groups.get(slot)!] as const] : []);
  }, [request?.variants]);
  const ownedItems = useMemo(() => {
    const items = new Map<string, ItemVariant>();
    for (const variant of request?.variants ?? []) {
      const key = variant.upgrade?.ownedItemKey ?? variant.key;
      if (!items.has(key) || variant.rank < (items.get(key)?.rank ?? Number.MAX_SAFE_INTEGER)) items.set(key, variant);
    }
    return [...items.entries()];
  }, [request?.variants]);
  const hasUpgradeCandidates = useMemo(() => (request?.variants ?? []).some((variant) => variant.enabled && variant.actions.some((action) => action.kind === "upgrade")), [request?.variants]);
  const hasCostedUpgradeCandidates = useMemo(() => (request?.variants ?? []).some((variant) => variant.enabled && variant.changed && budgetCurrencies.some((currency) => (variant.cost[currency] ?? 0) > 0)), [request?.variants]);
  const hasCatalystCandidates = useMemo(() => (request?.variants ?? []).some((variant) => variant.enabled && variant.changed && variant.catalyst), [request?.variants]);
  const hasDebugUpgradePaths = (request?.upgradeMetadata?.itemUpgradePaths?.length ?? 0) > 0;
  const largestReduciblePool = variantsBySlot
    .filter(([slot, variants]) => !memberSlots(slot).every((member) => request?.lockedSlots.includes(member)) && variants.filter((variant) => !variant.changed || variant.enabled).length > memberSlots(slot).length)
    .map(([slot, variants]) => ({ slot, factor: Math.max(1, variants.filter((variant) => !variant.changed || variant.enabled).length / memberSlots(slot).length) }))
    .sort((left, right) => right.factor - left.factor)[0] ?? null;
  const optionalAxisFactor = request == null ? 1 : Math.max(1,
    request.talentLoadouts.filter((candidate) => candidate.enabled).length
    * Object.values(request.profileOptions).reduce((product, candidates) => product * Math.max(1, candidates.filter((candidate) => candidate.enabled).length), 1));

  const itemTooltip = (variant: ItemVariant) => itemTooltipModel({
    id: variant.sourceItemId,
    slot: variant.slot,
    itemLevel: variant.simcOptions.ilevel,
    rank: variant.rank,
    gemIds: variant.gemIds,
    enchantId: variant.enchantId,
    changed: variant.changed,
  }, {
    title: variant.displayName ?? t("tooltip.itemTitle", { id: variant.sourceItemId }),
    category: t("tooltip.itemCategory", { slot: t(`topGear.slot_${candidateSlot(variant.slot)}`) }),
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

  useEffect(() => {
    if (!request || preview || busy) return;
    setBusy(true);
    void topGearPrepare(request)
      .then((next) => {
        setPreview(next);
        setRequest((current) => current ? {
          ...current,
          balances: current.upgradeMetadata == null && Object.values(current.balances).every((value) => value === 0)
            ? { ...current.balances, ...next.detectedBalances }
            : current.balances,
          variants: next.variants.map((variant) => ({ ...variant, enabled: variant.enabled ?? true, displayName: variant.displayName ?? null, setGroups: variant.setGroups ?? [], catalyst: variant.catalyst ?? false, upgrade: variant.upgrade ?? { ownedItemKey: variant.key, currentRank: variant.rank, maxRank: null, source: "unknown" } })),
          talentLoadouts: next.talentLoadouts ?? current.talentLoadouts,
          profileOptions: next.profileOptions ?? current.profileOptions,
          upgradeMetadata: next.upgradeMetadata ?? current.upgradeMetadata,
        } : current);
        const base = next.variants[0];
        setDraft((current) => ({ ...current, baseKey: base?.key ?? "", rank: base?.rank ?? 0, gemIds: base?.gemIds.join("/") ?? "", enchantId: base?.enchantId ? String(base.enchantId) : "" }));
      })
      .catch((reason) => setError(String(reason)))
      .finally(() => setBusy(false));
  }, [busy, preview, request]);

  if (!quick || !request) {
    return <div className="page placeholder-page"><p className="eyebrow">{t("topGear.eyebrow")}</p><h1>{t("topGear.title")}</h1><p className="placeholder-description">{t("topGear.importFirst")}</p><button className="primary-button" type="button" onClick={onImport}>{t("quick.goImport")}</button></div>;
  }

  const updateNumber = (field: "combinationLimit", value: number) =>
    setRequest({ ...request, [field]: value });
  const updateCurrency = (kind: "balances" | "reserves", currency: string, value: number) => {
    setRequest({ ...request, [kind]: { ...request[kind], [currency]: nonNegativeInteger(value) }, currencyConfirmedAtUnixSeconds: Math.floor(Date.now() / 1000) });
    setPreview(null);
  };
  const updateCatalystCharges = (value: number) => {
    setRequest({ ...request, catalystCharges: nonNegativeInteger(value) });
    setPreview(null);
  };
  const maximizeResources = () => {
    setRequest({
      ...request,
      balances: { ...request.balances, ...Object.fromEntries(budgetCurrencies.map((currency) => [currency, unlimitedResourceValue])) },
      currencyConfirmedAtUnixSeconds: Math.floor(Date.now() / 1000),
    });
    setPreview(null);
  };
  const updateCandidate = (key: string, enabled: boolean) => {
    setRequest({ ...request, variants: request.variants.map((variant) => variant.key === key ? { ...variant, enabled } : variant) });
    setPreview(null);
  };
  const updateTargetRank = (ownedItemKey: string, value: string) => {
    const targetRankOverrides = { ...request.targetRankOverrides };
    if (value === "") delete targetRankOverrides[ownedItemKey];
    else targetRankOverrides[ownedItemKey] = Math.max(0, Number(value));
    setRequest({ ...request, targetRankOverrides });
    setPreview(null);
  };
  const toggleSlotLock = (slot: CandidateSlot) => {
    const members = memberSlots(slot);
    const locked = members.every((member) => request.lockedSlots.includes(member));
    const next = locked
      ? request.lockedSlots.filter((entry) => !members.includes(entry))
      : [...new Set([...request.lockedSlots, ...members])];
    setRequest({ ...request, lockedSlots: next });
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
  const addProfileOption = (axis = optionKind, rawValue = optionValue) => {
    const value = rawValue.trim();
    if (!value) return;
    const existing = request.profileOptions[axis] ?? [];
    if (existing.some((candidate) => candidate.value === value)) { setError(t("topGear.duplicateProfileOption")); return; }
    const candidate: ProfileOptionVariant = { key: `custom-${axis}-${Date.now()}`, label: value, option: axis, value, changed: true, enabled: true };
    setRequest({ ...request, profileOptions: { ...request.profileOptions, [axis]: [...existing, candidate] } });
    if (axis === "omnium_talents") setOmniumValue("");
    else setOptionValue("");
    setPreview(null); setError(null);
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
      simcOptions, cost: {}, upgrade: { ownedItemKey: `manual-${sourceItemId}-${stamp}`, currentRank: 0, maxRank: null, source: "manual" }, actions: [{ id: `equip-${stamp}`, label: t("topGear.equipAction"), kind: "equip", cost: {}, dependsOn: [], fromRank: null, toRank: null, slot: itemSlot, sourceItemId, simcOptionsPatch: simcOptions }],
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
  const keepOnlyBaselineOptions = () => {
    setRequest({
      ...request,
      talentLoadouts: request.talentLoadouts.map((candidate) => ({ ...candidate, enabled: !candidate.changed })),
      profileOptions: Object.fromEntries(Object.entries(request.profileOptions).map(([axis, candidates]) => [axis, candidates.map((candidate) => ({ ...candidate, enabled: !candidate.changed }))])),
    });
    setPreview(null);
  };

  const refresh = async () => {
    setBusy(true); setError(null);
    try { setPreview(await topGearPrepare(request)); } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const start = async () => {
    setBusy(true); setError(null);
    try {
      const next = await topGearPrepare(request);
      setPreview(next);
      const started = await topGearStart(request);
      onStarted(started);
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
    const additionalCost = { adventurer_mistcrest: draft.adventurerMistcrest, veteran_mistcrest: draft.veteranMistcrest, champion_mistcrest: draft.championMistcrest, hero_mistcrest: draft.heroMistcrest, myth_mistcrest: draft.mythMistcrest, spark_of_tides: draft.sparkOfTides };
    const cost = { adventurer_mistcrest: (base.cost.adventurer_mistcrest ?? 0) + draft.adventurerMistcrest, veteran_mistcrest: (base.cost.veteran_mistcrest ?? 0) + draft.veteranMistcrest, champion_mistcrest: (base.cost.champion_mistcrest ?? 0) + draft.championMistcrest, hero_mistcrest: (base.cost.hero_mistcrest ?? 0) + draft.heroMistcrest, myth_mistcrest: (base.cost.myth_mistcrest ?? 0) + draft.mythMistcrest, spark_of_tides: (base.cost.spark_of_tides ?? 0) + draft.sparkOfTides };
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
      upgrade: { ...(base.upgrade ?? { ownedItemKey: base.key, currentRank: base.rank, maxRank: null, source: "unknown" }), maxRank: Math.max(draft.rank, base.upgrade?.maxRank ?? base.rank), source: "manual" },
      actions,
      uniqueGroups: draft.uniqueGroup ? [draft.uniqueGroup] : [],
      setGroups: draft.setGroup ? [draft.setGroup] : [],
      weaponKind: draft.weaponKind,
      embellishment: draft.embellishment,
      catalyst: draft.catalyst,
      enabled: true,
      changed: true,
    };
    const ownedItemKey = variant.upgrade?.ownedItemKey ?? variant.key;
    const variants = request.variants.map((candidate) => (candidate.upgrade?.ownedItemKey ?? candidate.key) === ownedItemKey ? { ...candidate, upgrade: { ...(candidate.upgrade ?? { ownedItemKey, currentRank: candidate.rank, maxRank: null, source: "unknown" }), maxRank: variant.upgrade?.maxRank ?? null, source: "manual" as const } } : candidate);
    setRequest({ ...request, variants: [...variants, variant] });
    setPreview(null);
    setDraft({ ...emptyVariant, baseKey: base.key });
  };
  return (
    <div className="page top-gear-page">
      <p className="eyebrow">{t("topGear.eyebrow")}</p>
      <h1>{t("topGear.title")}</h1>
      <p className="settings-lead">{t("topGear.body")}</p>

      <nav className="optimizer-nav" aria-label={t("topGear.quickNav")}>
        <a href="#optimizer-gear">{t("topGear.navGear")}</a><a href="#optimizer-enhancements">{t("topGear.navEnhancements")}</a><a href="#optimizer-consumables">{t("topGear.navConsumables")}</a><a href="#optimizer-omnium">{t("topGear.navOmnium")}</a><a href="#optimizer-talents">{t("topGear.navTalents")}</a><a href="#optimizer-options">{t("topGear.navOptions")}</a>
      </nav>

      <section className="candidate-browser" id="optimizer-gear" aria-labelledby="candidate-heading">
        <div className="section-heading"><div><h2 id="candidate-heading">{t("topGear.candidateGear")}</h2><p>{t("topGear.candidateGearHelp")}</p></div><button className="text-button" type="button" onClick={resetSelections}><RotateCcw aria-hidden="true" size={15} />{t("topGear.resetSelections")}</button></div>
        <div className="slot-candidate-grid">{variantsBySlot.map(([slot, variants]) => {
          const locked = memberSlots(slot).every((member) => request.lockedSlots.includes(member));
          return <article className="slot-candidate-card" key={slot}>
            <header><strong>{t(`topGear.slot_${slot}`)}</strong><button aria-label={t(locked ? "topGear.unlockSlot" : "topGear.lockSlot", { slot: t(`topGear.slot_${slot}`) })} className="icon-button" type="button" onClick={() => toggleSlotLock(slot)}>{locked ? <Lock aria-hidden="true" size={15} /> : <Unlock aria-hidden="true" size={15} />}</button></header>
            <ul>{variants.map((variant) => {
              const selected = !variant.changed || (variant.enabled && !locked);
              const currentRank = variant.upgrade?.currentRank ?? variant.rank;
              const targetRank = request.targetRankOverrides[variant.upgrade?.ownedItemKey ?? variant.key] ?? variant.upgrade?.maxRank;
              return <li className="candidate-item-row" key={variant.key}>
                <EntityTooltip model={itemTooltip(variant)} />
                <label className="candidate-check">
                  <input checked={selected} disabled={!variant.changed || locked} type="checkbox" onChange={(event) => updateCandidate(variant.key, event.target.checked)} />
                  <span className="candidate-copy">
                    <span className="candidate-badges"><span className="candidate-origin-badge">{t(`topGear.origin_${candidateOrigin(variant)}`)}</span><span className="candidate-state-badge">{t(selected ? "topGear.selected" : "topGear.notSelected")}</span></span>
                    <strong>{variant.displayName ?? t("tooltip.itemTitle", { id: variant.sourceItemId })}</strong>
                    <small>{t("topGear.itemIdValue", { id: variant.sourceItemId })} · {variant.simcOptions.ilevel ? t("topGear.itemLevelValue", { level: variant.simcOptions.ilevel }) : t("topGear.itemLevelUnavailable")}</small>
                    <small>{t("topGear.gemEnchantState", { gems: variant.gemIds.length ? variant.gemIds.join("/") : t("tooltip.none"), enchant: variant.enchantId ?? t("tooltip.none") })}</small>
                    <small>{targetRank == null ? t("topGear.currentOnlyUpgrade") : t("topGear.upgradeState", { current: currentRank, target: targetRank })}</small>
                    <small>{t(variant.catalyst ? "topGear.catalystConfirmed" : "topGear.catalystNotConfirmed")}</small>
                    {locked && variant.changed ? <span className="candidate-exclusion">{t("topGear.excludedSlotLocked")}</span>
                      : !variant.enabled ? <span className="candidate-exclusion">{t("topGear.excludedNotSelected")}</span>
                        : request.enhancementPolicy === "current_state" && variant.actions.some((action) => action.kind === "upgrade") ? <span className="candidate-exclusion">{t("topGear.excludedCurrentPolicy")}</span>
                          : variant.catalyst && request.catalystCharges < 1 ? <span className="candidate-exclusion">{t("topGear.excludedCatalystBudget")}</span>
                            : request.enhancementPolicy === "budget_constrained" && Object.entries(variant.cost).some(([currency, cost]) => cost > Math.max(0, (request.balances[currency] ?? 0) - (request.reserves[currency] ?? 0))) ? <span className="candidate-exclusion">{t("topGear.excludedCurrencyBudget")}</span>
                              : null}
                  </span>
                </label>
              </li>;
            })}</ul>
          </article>;
        })}</div>
        <details className="inline-disclosure"><summary>{t("topGear.addExactItem")}</summary><div className="exact-item-form"><label>{t("topGear.itemSlot")}<select value={itemSlot} onChange={(event) => setItemSlot(event.target.value as GearSlot)}>{slotOrder.filter((slot) => !["shirt", "tabard", "finger2", "trinket2"].includes(slot)).map((slot) => <option key={slot} value={slot}>{t(`topGear.slot_${candidateSlot(slot)}`)}</option>)}</select></label><label>{t("topGear.itemId")}<input inputMode="numeric" value={itemId} onChange={(event) => setItemId(event.target.value)} /></label><label>{t("topGear.itemNameOptional")}<input value={itemName} onChange={(event) => setItemName(event.target.value)} /></label><label>{t("topGear.itemOptions")}<input placeholder="bonus_id=… , context=…" value={itemOptions} onChange={(event) => setItemOptions(event.target.value)} /></label><button className="secondary-button" disabled={!itemId} type="button" onClick={addExactItem}>{t("topGear.addItemCandidate")}</button></div><small>{t("topGear.itemSearchBoundary")}</small></details>
      </section>

      <div className="top-gear-grid">
        <section className="settings-form" aria-labelledby="set-heading">
          <h2 id="set-heading"><ShieldCheck aria-hidden="true" size={18} />{t("topGear.setAndCatalyst")}</h2>
          <div className="catalyst-resource-row">
            <label>{t("topGear.catalystCharges")}<input min={0} type="number" value={request.catalystCharges} onChange={(event) => updateCatalystCharges(Number(event.target.value))} /></label>
            <div className="resource-quick-actions" role="group" aria-label={t("topGear.catalystQuickActions")}>
              <button className="compact-button" type="button" onClick={() => updateCatalystCharges(request.catalystCharges + 1)}>+1</button>
            </div>
          </div>
          {!hasCatalystCandidates ? <p className="safe-note resource-warning">{t("topGear.noCatalystCandidates")}</p> : null}
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
        <div className="profile-option-groups">{Object.entries(request.profileOptions).filter(([axis]) => axis !== "omnium_talents").map(([axis, candidates]) => <fieldset key={axis}><legend>{t(`topGear.option_${axis}`)}</legend>{candidates.map((candidate) => <label className="checkbox-line" key={candidate.key}><input checked={candidate.enabled} disabled={!candidate.changed} type="checkbox" onChange={(event) => updateProfileOption(axis, candidate.key, event.target.checked)} /><span><strong>{candidate.changed ? candidate.label : t("topGear.profileDefault")}</strong><small>{candidate.value || t("topGear.noOverride")}</small></span></label>)}</fieldset>)}</div>
        <div className="inline-fields profile-option-add"><label>{t("topGear.optionType")}<select value={optionKind} onChange={(event) => setOptionKind(event.target.value)}>{Object.keys(request.profileOptions).filter((axis) => axis !== "omnium_talents").map((axis) => <option key={axis} value={axis}>{t(`topGear.option_${axis}`)}</option>)}</select></label><label>{t("topGear.simcValue")}<input placeholder={t("topGear.simcValuePlaceholder")} value={optionValue} onChange={(event) => setOptionValue(event.target.value)} /></label><button className="secondary-button" disabled={!optionValue.trim()} type="button" onClick={() => addProfileOption()}>{t("topGear.addOption")}</button></div>
      </section>

      <section className="variant-builder seasonal-content-card" id="optimizer-omnium" aria-labelledby="omnium-heading">
        <div className="section-heading"><div><p className="eyebrow">{t("topGear.seasonalContent")}</p><h2 id="omnium-heading">{t("topGear.omniumFolio")}</h2></div><p>{t("topGear.omniumHelp")}</p></div>
        <div className="omnium-candidates">{(request.profileOptions.omnium_talents ?? []).map((candidate) => <div className="omnium-candidate" key={candidate.key}><EntityTooltip model={{ kind: "talent", id: null, title: t("topGear.omniumFolio"), category: t("topGear.seasonalContent"), details: [{ label: t("topGear.simcValue"), value: candidate.value || t("topGear.noOverride") }] }} /><label className="checkbox-line"><input checked={candidate.enabled} disabled={!candidate.changed} type="checkbox" onChange={(event) => updateProfileOption("omnium_talents", candidate.key, event.target.checked)} /><span><strong>{candidate.changed ? candidate.label : t("topGear.profileDefault")}</strong><small>{candidate.value || t("topGear.noOmnium")}</small></span></label></div>)}</div>
        <div className="omnium-add"><label>{t("topGear.omniumEncodedValue")}<input placeholder="123:1/456:2" value={omniumValue} onChange={(event) => setOmniumValue(event.target.value)} /></label><button className="secondary-button" disabled={!omniumValue.trim()} type="button" onClick={() => addProfileOption("omnium_talents", omniumValue)}>{t("topGear.addOmniumCandidate")}</button></div>
        <small>{t("topGear.omniumExactHelp")}</small>
      </section>

      <section className="settings-form" aria-labelledby="enhancement-policy-heading">
        <h2 id="enhancement-policy-heading"><Hammer aria-hidden="true" size={18} />{t("topGear.enhancementPolicy")}</h2>
        <label>{t("topGear.enhancementPolicyLabel")}<select value={request.enhancementPolicy} onChange={(event) => { setRequest({ ...request, enhancementPolicy: event.target.value as TopGearRequest["enhancementPolicy"] }); setPreview(null); }}><option value="max_potential">{t("topGear.policy_max_potential")}</option><option value="budget_constrained">{t("topGear.policy_budget_constrained")}</option><option value="current_state">{t("topGear.policy_current_state")}</option></select></label>
        <p className="safe-note">{t(`topGear.policy_${request.enhancementPolicy}_help`)}</p>
        {!hasDebugUpgradePaths && request.enhancementPolicy !== "current_state" ? <p className="safe-note resource-warning" role="status">{t("topGear.debugUpgradeRequired")}</p> : null}
        {hasDebugUpgradePaths && !hasUpgradeCandidates && request.enhancementPolicy !== "current_state" ? <p className="safe-note resource-warning" role="status">{t("topGear.debugUpgradeCurrentOnly")}</p> : null}
        {request.enhancementPolicy !== "current_state" ? <details className="inline-disclosure"><summary>{t("topGear.itemUpgradeTargets")}</summary><div className="currency-list">{ownedItems.map(([ownedItemKey, variant]) => <div className="currency-row upgrade-target-row" key={ownedItemKey}><div className="upgrade-item-identity"><EntityTooltip model={itemTooltip(variant)} /><strong>{variant.displayName ?? t("tooltip.itemTitle", { id: variant.sourceItemId })}</strong></div><span>{t("topGear.currentRankValue", { rank: variant.upgrade?.currentRank ?? variant.rank })}</span><label>{t("topGear.targetRank")}<input min={variant.upgrade?.currentRank ?? variant.rank} type="number" placeholder={variant.upgrade?.maxRank == null ? t("topGear.unconfirmed") : String(variant.upgrade.maxRank)} value={request.targetRankOverrides[ownedItemKey] ?? ""} onChange={(event) => updateTargetRank(ownedItemKey, event.target.value)} /></label><small>{t(`topGear.metadataSource_${variant.upgrade?.source ?? "unknown"}`)}</small></div>)}</div></details> : null}
        {request.upgradeMetadata ? <details className="inline-disclosure"><summary>{t("topGear.importedUpgradeMetadata")}</summary><pre>{[...Object.entries(request.upgradeMetadata.rawFields).map(([key, value]) => `${key}=${value}`), ...request.upgradeMetadata.itemUpgradePaths.map((path) => `line ${path.sourceLine} · ${path.slot} · id=${path.itemId} · upgrade_levels=${path.raw}`)].join("\n")}</pre>{request.upgradeMetadata.itemUpgradeDiagnostics.length > 0 ? <ul className="constraint-list">{request.upgradeMetadata.itemUpgradeDiagnostics.map((diagnostic, index) => <li key={`${diagnostic.sourceLine}-${index}`}>{t("topGear.upgradeDiagnostic", { line: diagnostic.sourceLine, code: diagnostic.code })}</li>)}</ul> : null}<label className="checkbox-line"><input checked={request.upgradeMetadataConfirmed} type="checkbox" onChange={(event) => { setRequest({ ...request, upgradeMetadataConfirmed: event.target.checked }); setPreview(null); }} /><span><strong>{t("topGear.confirmUpgradeMetadata")}</strong><small>{t("topGear.confirmUpgradeMetadataHelp")}</small></span></label></details> : null}
      </section>

      <div className="top-gear-grid">
        {request.enhancementPolicy === "budget_constrained" ? <section className="settings-form" aria-labelledby="budget-heading">
          <div className="section-heading compact-section-heading"><h2 id="budget-heading"><ShieldCheck aria-hidden="true" size={18} />{t("topGear.budget")}</h2><button className="secondary-button" type="button" onClick={maximizeResources}>{t("topGear.maxAll")}</button></div>
          {budgetCurrencies.map((currency) => <div className="currency-row" key={currency}>
            <strong>{t(`topGear.${currency}`)}</strong>
            <label>{t("topGear.balance")}<input min={0} type="number" value={request.balances[currency] ?? 0} onChange={(event) => updateCurrency("balances", currency, Number(event.target.value))} /></label>
            <label>{t("topGear.reserve")}<input min={0} type="number" value={request.reserves[currency] ?? 0} onChange={(event) => updateCurrency("reserves", currency, Number(event.target.value))} /></label>
            <div className="resource-quick-actions" role="group" aria-label={t("topGear.currencyQuickActions", { currency: t(`topGear.${currency}`) })}>
              <button className="compact-button" type="button" onClick={() => updateCurrency("balances", currency, (request.balances[currency] ?? 0) + 10)}>+10</button>
              <button className="compact-button" type="button" onClick={() => updateCurrency("balances", currency, (request.balances[currency] ?? 0) + 100)}>+100</button>
              <button className="compact-button" type="button" onClick={() => updateCurrency("balances", currency, unlimitedResourceValue)}>{t("topGear.max")}</button>
            </div>
          </div>)}
          <p className="safe-note">{t("topGear.currencyNote")}</p>
          {!hasCostedUpgradeCandidates ? <p className="safe-note resource-warning">{t("topGear.noCostedUpgradeCandidates")}</p> : null}
        </section> : null}

        <section className="settings-form" aria-labelledby="precision-heading">
          <h2 id="precision-heading"><Sparkles aria-hidden="true" size={18} />{t("topGear.precision")}</h2>
          <label>{t("topGear.limit")}<input min={1} max={2048} type="number" value={request.combinationLimit} onChange={(event) => updateNumber("combinationLimit", Number(event.target.value))} /></label>
          <p className="safe-note">{t("topGear.automaticPrecision", { low: request.lowTargetError * 100, medium: request.mediumTargetError * 100, high: request.highTargetError * 100 })}</p>
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
          <label>{t("topGear.baseItem")}<select value={draft.baseKey} onChange={(event) => { const base = request.variants.find((variant) => variant.key === event.target.value); setDraft({ ...draft, baseKey: event.target.value, rank: base?.rank ?? 0, gemIds: base?.gemIds.join("/") ?? "", enchantId: base?.enchantId ? String(base.enchantId) : "", itemLevel: "" }); }}>{request.variants.map((variant) => <option key={variant.key} value={variant.key}>{t(`topGear.slot_${candidateSlot(variant.slot)}`)} · {variant.sourceItemId} · {variant.key}</option>)}</select></label>
          <label><Gem aria-hidden="true" size={15} />{t("topGear.gems")}<input placeholder="213455/213456" value={draft.gemIds} onChange={(event) => setDraft({ ...draft, gemIds: event.target.value })} /></label>
          <label>{t("topGear.enchant")}<input inputMode="numeric" value={draft.enchantId} onChange={(event) => setDraft({ ...draft, enchantId: event.target.value })} /></label>
          <label>{t("topGear.rank")}<input min={0} type="number" value={draft.rank} onChange={(event) => setDraft({ ...draft, rank: Number(event.target.value) })} /></label>
          <label>{t("topGear.itemLevel")}<input inputMode="numeric" value={draft.itemLevel} onChange={(event) => setDraft({ ...draft, itemLevel: event.target.value })} /></label>
          <label>{t("topGear.effectiveAdventurerMistcrest")}<input min={0} type="number" value={draft.adventurerMistcrest} onChange={(event) => setDraft({ ...draft, adventurerMistcrest: Number(event.target.value) })} /></label>
          <label>{t("topGear.effectiveVeteranMistcrest")}<input min={0} type="number" value={draft.veteranMistcrest} onChange={(event) => setDraft({ ...draft, veteranMistcrest: Number(event.target.value) })} /></label>
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
        <ul className="variant-list">{request.variants.map((variant) => <li key={variant.key}><span className="variant-identity"><EntityTooltip model={itemTooltip(variant)} /><span>{t(`topGear.slot_${candidateSlot(variant.slot)}`)} · {variant.sourceItemId}</span></span><small>{variant.changed ? t("topGear.candidate") : t("topGear.worn")} · {variant.gemIds.length} {t("topGear.gemsShort")} · {variant.enchantId ?? "—"}</small>{variant.changed ? <button type="button" className="text-button" onClick={() => { setRequest({ ...request, variants: request.variants.filter((item) => item.key !== variant.key) }); setPreview(null); }}>{t("topGear.remove")}</button> : null}</li>)}</ul>
      </section>

      {preview ? <section className="preview-card" aria-live="polite"><h2>{t("topGear.preview")}</h2><div className="metric-grid"><article><span>{t("topGear.raw")}</span><strong>{preview.rawCombinations}</strong></article><article><span>{t("topGear.valid")}</span><strong>{preview.validCombinations}</strong></article><article><span>{t("topGear.executions")}</span><strong>{t("topGear.executionUpperBound", { count: preview.executionCount })}</strong></article><article><span>{t("topGear.workload")}</span><strong>{t(`topGear.workload_${preview.validCombinations <= 64 ? "small" : preview.validCombinations <= 512 ? "medium" : "large"}`)}</strong><small>{t("topGear.safeLimit", { limit: request.combinationLimit })}</small></article></div><div className="stage-count-grid"><article><span>{t("topGear.lowStage")}</span><strong>{preview.validCombinations}</strong><small>{t("topGear.exactCandidates")}</small></article><article><span>{t("topGear.mediumStage")}</span><strong>≤ {preview.validCombinations}</strong><small>{t("topGear.survivorBound")}</small></article><article><span>{t("topGear.highStage")}</span><strong>≤ {preview.validCombinations}</strong><small>{t("topGear.survivorBound")}</small></article></div><div className="workload-advice"><h3>{t("topGear.reduceWorkload")}</h3><p>{t("topGear.workloadSummary", { count: preview.validCombinations, limit: request.combinationLimit })}</p><div className="button-row"><button className="secondary-button" disabled={request.combinationLimit <= 256} type="button" onClick={() => { setRequest({ ...request, combinationLimit: 256 }); setPreview(null); }}>{t("topGear.applySafeLimit", { current: preview.validCombinations, reduced: Math.min(preview.validCombinations, 256) })}</button>{largestReduciblePool ? <button className="secondary-button" type="button" onClick={() => toggleSlotLock(largestReduciblePool.slot)}>{t("topGear.applySlotLockReduction", { slot: t(`topGear.slot_${largestReduciblePool.slot}`), current: preview.validCombinations, reduced: Math.ceil(preview.validCombinations / largestReduciblePool.factor) })}</button> : null}<button className="secondary-button" disabled={optionalAxisFactor <= 1} type="button" onClick={keepOnlyBaselineOptions}>{t("topGear.applyOptionalAxisReduction", { current: preview.validCombinations, reduced: Math.ceil(preview.validCombinations / optionalAxisFactor) })}</button></div><small>{t("topGear.reductionConsent")}</small></div><details className="rejection-details"><summary>{t("topGear.rejections")}</summary><ul>{Object.entries(preview.rejections).map(([reason, count]) => <li key={reason}><span>{t(`topGear.rejection_${reason}`)}</span><strong>{count}</strong></li>)}</ul></details>{preview.estimated ? <p className="status-warning"><span aria-hidden="true" />{t("topGear.estimated")}</p> : null}<p className="safe-note">{preview.ruleSource}</p></section> : null}

      {error ? <div className="inline-error" role="alert"><strong>{t("topGear.errorTitle")}</strong><code>{error}</code></div> : null}
      <div className="button-row quick-actions"><button className="secondary-button" disabled={busy} type="button" onClick={() => void refresh()}><RefreshCw aria-hidden="true" size={18} />{busy ? t("quick.refreshing") : t("quick.refresh")}</button><button className="primary-button" disabled={busy || !preview} type="button" onClick={() => void start()}><Play aria-hidden="true" size={18} />{busy ? t("quick.starting") : t("topGear.run")}</button></div>
    </div>
  );
}
