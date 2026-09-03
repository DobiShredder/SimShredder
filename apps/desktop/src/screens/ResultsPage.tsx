import * as Tabs from "@radix-ui/react-tabs";
import { Download, Gauge, Layers3, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { quickExport, quickResult, type JobView, type QuickResultView, type ResultTimeline, type StatisticalMetric } from "../quick";
import { sameRun, type RunReference } from "../runs";
import { buildRunCatalog } from "../runCatalog";
import { topGearResult, type TopGearResultView, type TopGearSessionView } from "../topGear";
import { EntityTooltip, type TooltipKind, type TooltipModel } from "../tooltips";
import { RunManagementControls } from "../components/RunManagementControls";
import { TopGearResultsPanel } from "./TopGearResultsPanel";

export function ResultsPage({ quickJobs, topGearSessions, selected, runNames, onSelect, onRerun, onEditRerun, onRename, canEditRun }: {
  quickJobs: JobView[];
  topGearSessions: TopGearSessionView[];
  selected: RunReference | null;
  runNames: Record<string, string>;
  onSelect: (run: RunReference) => void;
  onRerun: (run: RunReference) => Promise<void>;
  onEditRerun: (run: RunReference) => Promise<void>;
  onRename: (run: RunReference, name: string | null) => void;
  canEditRun: (run: RunReference) => boolean;
}) {
  const { t } = useTranslation();
  const [quick, setQuick] = useState<QuickResultView | null>(null);
  const [gear, setGear] = useState<TopGearResultView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const completed = useMemo(() => buildRunCatalog(quickJobs, topGearSessions, runNames).results, [quickJobs, runNames, topGearSessions]);
  const visible = completed.filter((entry) => `${entry.displayName} ${entry.characterName} ${entry.specialization} ${entry.type} ${entry.state}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  const selectedEntry = selected ? completed.find((entry) => sameRun(selected, entry.run)) ?? null : null;

  useEffect(() => { if ((!selected || !completed.some((entry) => sameRun(selected, entry.run))) && completed[0]) onSelect(completed[0].run); }, [completed, onSelect, selected]);
  useEffect(() => {
    if (!selected || !completed.some((entry) => sameRun(selected, entry.run))) return;
    setError(null); setQuick(null); setGear(null);
    const load = selected.kind === "quick"
      ? quickResult(selected.jobId).then(setQuick)
      : topGearResult(selected.sessionId).then(setGear);
    void load.catch((reason) => setError(String(reason)));
  }, [completed, selected]);
  const picker = <section className="result-picker"><label className="run-search"><span><Search aria-hidden="true" size={17} />{t("resultsPage.searchResults")}</span><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("resultsPage.searchPlaceholder")} /></label><div className="result-picker-list" role="list" aria-label={t("resultsPage.chooseResult")}>{visible.map((entry) => { const Icon = entry.type === "gearOptimizer" ? Layers3 : Gauge; return <div role="listitem" key={entry.key}><button className={sameRun(selected, entry.run) ? "selected" : ""} type="button" onClick={() => onSelect(entry.run)}><Icon aria-hidden="true" size={18} /><span><strong>{entry.displayName}</strong><small>{entry.characterName} · {entry.specialization} · {t(`historyPage.${entry.type}`)} · {new Date(entry.createdUnixMillis).toLocaleString()} · {entry.settings.fightStyle}, {entry.settings.desiredTargets}</small></span><em>{t(`jobsPage.${entry.state}`)}</em></button></div>; })}</div>{!visible.length ? <p className="muted">{t("resultsPage.noSearchResults")}</p> : null}{selectedEntry ? <RunManagementControls run={selectedEntry.run} displayName={selectedEntry.displayName} defaultName={`${selectedEntry.characterName} · ${selectedEntry.specialization}`} canEdit={canEditRun(selectedEntry.run)} onRerun={onRerun} onEditRerun={onEditRerun} onRename={onRename} /> : null}</section>;
  if (!completed.length) return <div className="page placeholder-page"><p className="eyebrow">{t("resultsPage.eyebrow")}</p><h1>{t("resultsPage.noResult")}</h1><p>{t("resultsPage.noResultBody")}</p></div>;
  if (error) return <div className="page results-page"><p className="eyebrow">{t("resultsPage.eyebrow")}</p><h1>{t("resultsPage.loadFailed")}</h1>{picker}<div className="inline-error" role="alert"><code>{error}</code></div></div>;
  if (gear) return <TopGearResultsPanel result={gear} picker={picker} />;
  if (quick) return <QuickResultPanel result={quick} picker={picker} />;
  return <div className="page results-page"><p className="eyebrow">{t("resultsPage.eyebrow")}</p><h1>{t("resultsPage.loading")}</h1>{picker}</div>;
}

function QuickResultPanel({ result, picker }: { result: QuickResultView | null; picker: ReactNode }) {
  const { t, i18n } = useTranslation();
  const metric = result?.result.primary_metric;
  const [exporting, setExporting] = useState(false);
  const [exportPath, setExportPath] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const [damageQuery, setDamageQuery] = useState("");
  const [actor, setActor] = useState("all");
  const [sequenceQuery, setSequenceQuery] = useState("");
  const [sequenceBuff, setSequenceBuff] = useState("all");
  const [sequencePage, setSequencePage] = useState(0);
  const [selectedAction, setSelectedAction] = useState(0);

  if (!result || !metric) return <div className="page placeholder-page"><p className="eyebrow">{t("resultsPage.eyebrow")}</p><h1>{t("resultsPage.noResult")}</h1></div>;
  const normalized = result.result;
  const number = (value: number) => value.toLocaleString(i18n.language, { maximumFractionDigits: 2 });
  const percent = (value: number) => `${number(value)}%`;
  const timelines = normalized.timelines ?? { damage: null, resources: [], buffs: [] };
  const actors = [...new Set(normalized.actions.map((action) => action.actor ?? "player"))];
  const visibleDamage = normalized.actions.filter((action) => {
    const matchesActor = actor === "all" || (action.actor ?? "player") === actor;
    const haystack = `${action.name} ${action.internal_name} ${action.school} ${action.actor ?? "player"}`.toLocaleLowerCase();
    return matchesActor && haystack.includes(damageQuery.toLocaleLowerCase());
  });
  const buffNames = [...new Set(normalized.apl_sequence.flatMap((action) => action.buffs.map((buff) => buff.internal_name)))];
  const matchingSequence = normalized.apl_sequence.filter((action) => {
    const haystack = `${action.name} ${action.internal_name} ${action.target} ${Object.keys(action.resources).join(" ")} ${action.buffs.map((buff) => buff.name).join(" ")}`.toLocaleLowerCase();
    return haystack.includes(sequenceQuery.toLocaleLowerCase())
      && (sequenceBuff === "all" || action.buffs.some((buff) => buff.internal_name === sequenceBuff));
  });
  const sequencePageSize = 20;
  const sequencePages = Math.max(1, Math.ceil(matchingSequence.length / sequencePageSize));
  const page = Math.min(sequencePage, sequencePages - 1);
  const visibleSequence = matchingSequence.slice(page * sequencePageSize, (page + 1) * sequencePageSize);
  const selected = normalized.apl_sequence[selectedAction] ?? null;
  const previousSelected = selectedAction > 0 ? normalized.apl_sequence[selectedAction - 1] : null;
  const entityModel = (kind: TooltipKind, id: number | null, name: string, internalName: string, details: TooltipModel["details"]): TooltipModel => ({
    kind, id, title: name, category: t(`tooltip.categories.${kind}`), details: [
      { label: t("resultsPage.internalName"), value: internalName },
      ...details,
    ],
  });
  const entityName = (kind: TooltipKind, id: number | null, name: string, internalName: string, details: TooltipModel["details"]) => (
    <span className="result-entity">
      {id ? <EntityTooltip model={entityModel(kind, id, name, internalName, details)} /> : null}
      <span>{name}</span>
    </span>
  );
  const resourceSnapshot = (values: Record<string, number>, maxima: Record<string, number>) => {
    const entries = Object.entries(values);
    if (!entries.length) return t("resultsPage.none");
    return entries.map(([name, value]) => `${name}: ${number(value)}${maxima[name] === undefined ? "" : ` / ${number(maxima[name])}`}`).join(" · ");
  };
  const actionBuffs = (action: typeof normalized.apl_sequence[number]) => (
    action.buffs.length ? <div className="action-buff-list">{action.buffs.map((buff, index) => {
      const summary = normalized.buffs.find((candidate) =>
        (buff.id !== null && candidate.id === buff.id)
        || candidate.internal_name === buff.internal_name,
      );
      const title = summary?.name ?? buff.name;
      return <span className="action-buff" key={`${buff.internal_name}-${buff.id ?? "none"}-${index}`}>
        <EntityTooltip model={entityModel("buff", buff.id, title, buff.internal_name, [{ label: t("resultsPage.stacks"), value: number(buff.stacks) }])} />
        <span className="action-buff-name">{title}</span>
        {buff.stacks > 1 ? <span className="action-buff-stacks" aria-label={t("resultsPage.stackCount", { count: number(buff.stacks) })}>{number(buff.stacks)}</span> : null}
      </span>;
    })}</div> : <span className="muted">{t("resultsPage.none")}</span>
  );
  return (
    <div className="page results-page">
      <p className="eyebrow">{t("resultsPage.eyebrow")}</p>
      <h1>{t("resultsPage.title", { name: normalized.player.name, spec: normalized.player.specialization })}</h1>
      {picker}
      <section className="metric-grid">
        <article className="primary-metric"><span>{t("resultsPage.primary", { metric: metric.name })}</span><strong>{number(metric.mean)}</strong><small>± {number(metric.mean_error)}</small></article>
        <article><span>{t("resultsPage.error")}</span><strong>{number(metric.mean_error)}</strong></article>
        <article><span>{t("resultsPage.median")}</span><strong>{number(metric.median)}</strong></article>
        <article><span>{t("resultsPage.range")}</span><strong>{number(metric.minimum)}–{number(metric.maximum)}</strong></article>
      </section>
      <DistributionSummary metric={metric} number={number} />
      <section className="result-detail-section">
        <h2>{t("resultsPage.damageBreakdown")}</h2>
        <div className="result-filters"><label><span>{t("resultsPage.searchActions")}</span><input value={damageQuery} onChange={(event) => setDamageQuery(event.target.value)} /></label><label><span>{t("resultsPage.actor")}</span><select value={actor} onChange={(event) => setActor(event.target.value)}><option value="all">{t("resultsPage.allActors")}</option>{actors.map((name) => <option key={name} value={name}>{name === "player" ? normalized.player.name : name}</option>)}</select></label></div>
        {visibleDamage.length ? <div className="table-scroll"><table><thead><tr><th scope="col">{t("resultsPage.actor")}</th><th scope="col">{t("resultsPage.action")}</th><th scope="col">{t("resultsPage.school")}</th><th scope="col">{t("resultsPage.executes")}</th><th scope="col">{t("resultsPage.amountPerFight")}</th><th scope="col">{t("resultsPage.perSecond")}</th><th scope="col">{t("resultsPage.share")}</th></tr></thead><tbody>{visibleDamage.map((action) => <tr key={`${action.actor ?? "player"}-${action.internal_name}-${action.id ?? "none"}`}><td>{(action.actor ?? "player") === "player" ? normalized.player.name : action.actor}</td><th scope="row">{entityName("spell", action.id, action.name, action.internal_name, [])}</th><td>{action.school}</td><td>{number(action.executes)}</td><td>{number(action.amount_per_fight)}</td><td>{number(action.metric_per_second)}</td><td>{percent(action.share * 100)}</td></tr>)}</tbody></table></div> : <p className="muted">{normalized.actions.length ? t("resultsPage.noActionMatches") : t("resultsPage.noActions")}</p>}
      </section>
      <div className="result-detail-grid">
        <section className="result-detail-section">
          <h2>{t("resultsPage.resources")}</h2>
          {normalized.resources.length ? <div className="table-scroll"><table><thead><tr><th scope="col">{t("resultsPage.resource")}</th><th scope="col">{t("resultsPage.spent")}</th><th scope="col">{t("resultsPage.overflow")}</th><th scope="col">{t("resultsPage.remaining")}</th></tr></thead><tbody>{normalized.resources.map((resource) => <tr key={resource.name}><th scope="row">{resource.name}</th><td>{number(resource.spent_per_fight)}</td><td>{number(resource.overflow_per_fight)}</td><td>{number(resource.remaining_per_fight)}</td></tr>)}</tbody></table></div> : <p className="muted">{t("resultsPage.noResources")}</p>}
        </section>
        <section className="result-detail-section">
          <h2>{t("resultsPage.buffs")}</h2>
          {normalized.buffs.length ? <div className="table-scroll"><table><thead><tr><th scope="col">{t("resultsPage.buff")}</th><th scope="col">{t("resultsPage.uptime")}</th><th scope="col">{t("resultsPage.benefit")}</th><th scope="col">{t("resultsPage.starts")}</th></tr></thead><tbody>{normalized.buffs.map((buff) => <tr key={`${buff.internal_name}-${buff.id ?? "none"}`}><th scope="row">{entityName("buff", buff.id, buff.name, buff.internal_name, [])}</th><td>{percent(buff.uptime_percent)}</td><td>{buff.benefit_percent === null ? t("resultsPage.none") : percent(buff.benefit_percent)}</td><td>{number(buff.starts)}</td></tr>)}</tbody></table></div> : <p className="muted">{t("resultsPage.noBuffs")}</p>}
        </section>
      </div>
      <section className="result-detail-section">
        <h2>{t("resultsPage.timelines")}</h2>
        <p className="muted">{t("resultsPage.timelineHelp")}</p>
        {timelines.damage ? <EvidenceTimeline timeline={timelines.damage} number={number} /> : null}
        {timelines.resources.map((timeline) => <EvidenceTimeline key={`resource-${timeline.name}`} timeline={timeline} number={number} />)}
        {timelines.buffs.map((timeline) => <EvidenceTimeline key={`buff-${timeline.name}`} timeline={timeline} number={number} />)}
        {!timelines.damage && !timelines.resources.length && !timelines.buffs.length ? <div className="partial-note"><strong>{t("resultsPage.timelineUnavailable")}</strong><p>{t("resultsPage.timelineUnavailableBody")}</p></div> : null}
        <div className="partial-note"><strong>{t("resultsPage.cooldownUnavailable")}</strong><p>{t("resultsPage.cooldownUnavailableBody")}</p></div>
      </section>
      <section className="result-detail-section">
        <h2>{t("resultsPage.aplSequence")}</h2>
        <p className="muted">{t("resultsPage.aplHelp")}</p>
        {normalized.apl_sequence.length ? <><div className="result-filters"><label><span>{t("resultsPage.searchSequence")}</span><input value={sequenceQuery} onChange={(event) => { setSequenceQuery(event.target.value); setSequencePage(0); }} /></label><label><span>{t("resultsPage.buffFilter")}</span><select value={sequenceBuff} onChange={(event) => { setSequenceBuff(event.target.value); setSequencePage(0); }}><option value="all">{t("resultsPage.allBuffs")}</option>{buffNames.map((name) => <option key={name} value={name}>{name}</option>)}</select></label></div><div className="table-scroll"><table className="action-sequence-table"><thead><tr><th scope="col">{t("resultsPage.time")}</th><th scope="col">{t("resultsPage.action")}</th><th scope="col">{t("resultsPage.target")}</th><th scope="col">{t("resultsPage.resourceState")}</th><th scope="col">{t("resultsPage.activeBuffs")}</th></tr></thead><tbody>{visibleSequence.map((action) => { const index = normalized.apl_sequence.indexOf(action); return <tr className={selectedAction === index ? "selected-row" : ""} key={`${action.time_seconds}-${action.internal_name}-${index}`}><td>{number(action.time_seconds)}s</td><th scope="row"><span className="action-select-cell">{entityName("spell", action.id, action.name, action.internal_name, [])}<button className="table-row-button" type="button" onClick={() => setSelectedAction(index)}>{t("resultsPage.viewSnapshot")}</button></span></th><td>{action.target}</td><td>{resourceSnapshot(action.resources, action.resource_max)}</td><td>{actionBuffs(action)}</td></tr>; })}</tbody></table></div><div className="pagination"><button type="button" disabled={page === 0} onClick={() => setSequencePage(page - 1)}>{t("resultsPage.previousPage")}</button><span>{t("resultsPage.pageStatus", { page: page + 1, pages: sequencePages, count: matchingSequence.length })}</span><button type="button" disabled={page + 1 >= sequencePages} onClick={() => setSequencePage(page + 1)}>{t("resultsPage.nextPage")}</button></div>{selected ? <article className="action-snapshot"><h3>{t("resultsPage.actionSnapshot", { name: selected.name })}</h3><dl><div><dt>{t("resultsPage.time")}</dt><dd>{number(selected.time_seconds)}s</dd></div><div><dt>{t("resultsPage.target")}</dt><dd>{selected.target}</dd></div><div><dt>{t("resultsPage.resourceState")}</dt><dd>{resourceSnapshot(selected.resources, selected.resource_max)}</dd></div><div><dt>{t("resultsPage.resourceChange")}</dt><dd>{Object.keys(selected.resources).length ? Object.entries(selected.resources).map(([name, value]) => { const previous = previousSelected?.resources[name]; const delta = previous === undefined ? null : value - previous; const capped = selected.resource_max[name] !== undefined && value >= selected.resource_max[name]; return `${name}: ${delta === null ? t("resultsPage.noPreviousSample") : `${delta >= 0 ? "+" : ""}${number(delta)}`}${capped ? ` · ${t("resultsPage.atCap")}` : ""}`; }).join(" · ") : t("resultsPage.none")}</dd></div><div><dt>{t("resultsPage.activeBuffs")}</dt><dd>{selected.buffs.length ? selected.buffs.map((buff) => `${buff.name} ×${number(buff.stacks)}`).join(" · ") : t("resultsPage.none")}</dd></div><div><dt>{t("resultsPage.actionAmount")}</dt><dd>{t("resultsPage.notInReport")}</dd></div></dl><p className="muted">{t("resultsPage.resourceChangeCaution")}</p></article> : null}</> : <p className="muted">{t("resultsPage.noApl")}</p>}
      </section>
      <section className="identity-card"><h2>{t("resultsPage.executionIdentity")}</h2><p>{t("resultsPage.profileIdentity", { name: normalized.player.name, spec: normalized.player.specialization, role: normalized.player.role })}</p><p>{t("resultsPage.scenarioIdentity", { style: normalized.options.fight_style, targets: normalized.options.desired_targets, seconds: number(normalized.options.max_time_seconds) })}</p><p>{t("resultsPage.precisionIdentity", { target: number(normalized.options.target_error ?? 0), confidence: percent((normalized.options.confidence ?? .95) * 100) })}</p><p>{t("resultsPage.simc", { version: normalized.runtime.simc_version, revision: normalized.runtime.git_revision })}</p><p>{t("resultsPage.game", { version: normalized.runtime.game_version, build: normalized.runtime.game_build })}</p><p>{t("resultsPage.iterations", { count: normalized.options.iterations.toLocaleString(i18n.language), threads: normalized.options.threads })}</p><p>{t("resultsPage.aplIdentity")}</p><p className="status-warning">{t("resultsPage.comparisonWarning")}</p></section>
      <section className="artifact-section">
        <h2>{t("resultsPage.artifacts")}</h2><p className="muted">{t("resultsPage.artifactPath", { path: result.artifactDirectory })}</p>
        <div className="button-row"><button className="secondary-button" disabled={exporting} type="button" onClick={() => {
          setExporting(true); setExportError(null); setExportPath(null);
          void quickExport(result.jobId).then((value) => setExportPath(value.directory)).catch((reason) => setExportError(String(reason))).finally(() => setExporting(false));
        }}><Download aria-hidden="true" size={18} />{exporting ? t("resultsPage.exporting") : t("resultsPage.export")}</button></div>
        {exportPath ? <p className="safe-note" role="status">{t("resultsPage.exported", { path: exportPath })}</p> : null}
        {exportError ? <div className="inline-error" role="alert"><strong>{t("resultsPage.exportError")}</strong><code>{exportError}</code></div> : null}
        <Tabs.Root className="artifact-tabs" defaultValue="input">
          <Tabs.List aria-label={t("resultsPage.artifacts")}><Tabs.Trigger value="input">{t("resultsPage.input")}</Tabs.Trigger><Tabs.Trigger value="json">{t("resultsPage.json")}</Tabs.Trigger><Tabs.Trigger value="html">{t("resultsPage.html")}</Tabs.Trigger><Tabs.Trigger value="logs">{t("resultsPage.logs")}</Tabs.Trigger></Tabs.List>
          <Tabs.Content value="input"><pre tabIndex={0}>{result.generatedInput}</pre></Tabs.Content>
          <Tabs.Content value="json"><pre tabIndex={0}>{result.rawJson}</pre></Tabs.Content>
          <Tabs.Content value="html"><p className="safe-note">{t("resultsPage.htmlSafety")}</p><pre tabIndex={0}>{result.rawHtml}</pre></Tabs.Content>
          <Tabs.Content value="logs"><h3>{t("resultsPage.stdout")}</h3>{result.stdoutTruncated ? <p className="status-warning">{t("resultsPage.truncated")}</p> : null}<pre tabIndex={0}>{result.stdout}</pre><h3>{t("resultsPage.stderr")}</h3>{result.stderrTruncated ? <p className="status-warning">{t("resultsPage.truncated")}</p> : null}<pre tabIndex={0}>{result.stderr}</pre></Tabs.Content>
        </Tabs.Root>
      </section>
    </div>
  );
}

function DistributionSummary({ metric, number }: { metric: StatisticalMetric; number: (value: number) => string }) {
  const { t } = useTranslation();
  const span = Math.max(metric.maximum - metric.minimum, 1);
  const position = (value: number) => `${Math.max(0, Math.min(100, ((value - metric.minimum) / span) * 100))}%`;
  return <section className="distribution-summary" aria-labelledby="distribution-heading">
    <h2 id="distribution-heading">{t("resultsPage.distribution")}</h2>
    <p className="muted">{t("resultsPage.distributionEvidence")}</p>
    <div className="distribution-track" aria-hidden="true"><span className="distribution-range" /><span className="distribution-marker median" style={{ left: position(metric.median) }} /><span className="distribution-marker mean" style={{ left: position(metric.mean) }} /></div>
    <div className="table-scroll"><table><thead><tr><th scope="col">{t("resultsPage.minimum")}</th><th scope="col">{t("resultsPage.median")}</th><th scope="col">{t("resultsPage.mean")}</th><th scope="col">{t("resultsPage.maximum")}</th><th scope="col">{t("resultsPage.standardDeviation")}</th><th scope="col">{t("resultsPage.meanError")}</th></tr></thead><tbody><tr><td>{number(metric.minimum)}</td><td>{number(metric.median)}</td><td>{number(metric.mean)}</td><td>{number(metric.maximum)}</td><td>{number(metric.standard_deviation)}</td><td>± {number(metric.mean_error)}</td></tr></tbody></table></div>
  </section>;
}

function EvidenceTimeline({ timeline, number }: { timeline: ResultTimeline; number: (value: number) => string }) {
  const { t } = useTranslation();
  const chart = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!chart.current || !timeline.samples.length || navigator.userAgent.includes("jsdom")) return;
    let dispose: (() => void) | undefined;
    let canceled = false;
    void Promise.all([import("echarts/core"), import("echarts/charts"), import("echarts/components"), import("echarts/renderers")]).then(([echarts, charts, components, renderers]) => {
      if (canceled || !chart.current) return;
      echarts.use([charts.LineChart, components.GridComponent, components.TooltipComponent, renderers.CanvasRenderer]);
      const instance = echarts.init(chart.current);
      instance.setOption({ animation: false, backgroundColor: "transparent", grid: { left: 58, right: 18, top: 16, bottom: 30 }, xAxis: { type: "category", name: t("resultsPage.sample"), data: timeline.samples.map((_, index) => index + 1), axisLabel: { color: "#9ca9ba", hideOverlap: true } }, yAxis: { type: "value", name: timeline.unit, axisLabel: { color: "#9ca9ba" } }, series: [{ type: "line", showSymbol: false, data: timeline.samples, lineStyle: { color: "#68d5c3" }, areaStyle: { color: "rgba(104, 213, 195, .12)" } }], tooltip: { trigger: "axis" } });
      const resize = () => instance.resize();
      window.addEventListener("resize", resize);
      dispose = () => { window.removeEventListener("resize", resize); instance.dispose(); };
    });
    return () => { canceled = true; dispose?.(); };
  }, [t, timeline]);
  const tableSamples = timeline.samples.filter((_, index) => index % Math.max(1, Math.ceil(timeline.samples.length / 12)) === 0).slice(0, 12);
  return <details className="timeline-card"><summary><strong>{timeline.name}</strong><span>{t("resultsPage.timelineSummary", { mean: number(timeline.mean), min: number(timeline.minimum), max: number(timeline.maximum), count: timeline.source_sample_count })}</span></summary><div className="evidence-chart" ref={chart} aria-hidden="true" /><div className="table-scroll"><table><caption>{t("resultsPage.timelineTable", { name: timeline.name })}</caption><thead><tr>{tableSamples.map((_, index) => <th scope="col" key={index}>{t("resultsPage.sampleNumber", { number: index + 1 })}</th>)}</tr></thead><tbody><tr>{tableSamples.map((value, index) => <td key={index}>{number(value)}</td>)}</tr></tbody></table></div></details>;
}
