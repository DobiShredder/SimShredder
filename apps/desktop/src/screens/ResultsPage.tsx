import * as Tabs from "@radix-ui/react-tabs";
import { Download } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { quickExport, type QuickResultView } from "../quick";
import { EntityTooltip, type TooltipKind, type TooltipModel } from "../tooltips";

export function ResultsPage({ result }: { result: QuickResultView | null }) {
  const { t, i18n } = useTranslation();
  const chart = useRef<HTMLDivElement>(null);
  const metric = result?.result.primary_metric;
  const [exporting, setExporting] = useState(false);
  const [exportPath, setExportPath] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  useEffect(() => {
    if (!chart.current || !metric) return;
    let dispose: (() => void) | undefined;
    let canceled = false;
    void Promise.all([
      import("echarts/core"),
      import("echarts/charts"),
      import("echarts/components"),
      import("echarts/renderers"),
    ]).then(([echarts, charts, components, renderers]) => {
      if (canceled || !chart.current) return;
      echarts.use([charts.BarChart, components.GridComponent, components.TooltipComponent, renderers.CanvasRenderer]);
      const instance = echarts.init(chart.current);
      instance.setOption({
        backgroundColor: "transparent",
        grid: { left: 72, right: 28, top: 22, bottom: 36 },
        xAxis: { type: "value", min: Math.floor(metric.minimum * 0.98), axisLabel: { color: "#9ca9ba" } },
        yAxis: { type: "category", data: [metric.name], axisLabel: { color: "#9ca9ba" } },
        series: [{ type: "bar", data: [metric.mean], itemStyle: { color: "#68d5c3", borderRadius: 6 }, label: { show: true, position: "right", formatter: metric.mean.toLocaleString(i18n.language, { maximumFractionDigits: 1 }), color: "#9ca9ba" } }],
        tooltip: { trigger: "axis", valueFormatter: (value: unknown) => Number(value).toLocaleString(i18n.language) },
      });
      const resize = () => instance.resize();
      window.addEventListener("resize", resize);
      dispose = () => { window.removeEventListener("resize", resize); instance.dispose(); };
    });
    return () => { canceled = true; dispose?.(); };
  }, [i18n.language, metric]);

  if (!result || !metric) return <div className="page placeholder-page"><p className="eyebrow">{t("resultsPage.eyebrow")}</p><h1>{t("resultsPage.noResult")}</h1></div>;
  const normalized = result.result;
  const number = (value: number) => value.toLocaleString(i18n.language, { maximumFractionDigits: 2 });
  const percent = (value: number) => `${number(value)}%`;
  const entityModel = (kind: TooltipKind, id: number, name: string, internalName: string, details: TooltipModel["details"]): TooltipModel => ({
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
  return (
    <div className="page results-page">
      <p className="eyebrow">{t("resultsPage.eyebrow")}</p>
      <h1>{t("resultsPage.title", { name: normalized.player.name, spec: normalized.player.specialization })}</h1>
      <section className="metric-grid">
        <article className="primary-metric"><span>{t("resultsPage.primary", { metric: metric.name })}</span><strong>{number(metric.mean)}</strong><small>± {number(metric.mean_error)}</small></article>
        <article><span>{t("resultsPage.error")}</span><strong>{number(metric.mean_error)}</strong></article>
        <article><span>{t("resultsPage.median")}</span><strong>{number(metric.median)}</strong></article>
        <article><span>{t("resultsPage.range")}</span><strong>{number(metric.minimum)}–{number(metric.maximum)}</strong></article>
      </section>
      <section className="result-chart" aria-label={t("resultsPage.chartLabel")}><div ref={chart} /></section>
      <section className="result-detail-section">
        <h2>{t("resultsPage.damageBreakdown")}</h2>
        {normalized.actions.length ? <div className="table-scroll"><table><thead><tr><th scope="col">{t("resultsPage.action")}</th><th scope="col">{t("resultsPage.executes")}</th><th scope="col">{t("resultsPage.perSecond")}</th><th scope="col">{t("resultsPage.share")}</th></tr></thead><tbody>{normalized.actions.map((action) => <tr key={`${action.internal_name}-${action.id ?? "none"}`}><th scope="row">{entityName("spell", action.id, action.name, action.internal_name, [{ label: t("resultsPage.school"), value: action.school }, { label: t("resultsPage.amountPerFight"), value: number(action.amount_per_fight) }])}</th><td>{number(action.executes)}</td><td>{number(action.metric_per_second)}</td><td>{percent(action.share * 100)}</td></tr>)}</tbody></table></div> : <p className="muted">{t("resultsPage.noActions")}</p>}
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
        <h2>{t("resultsPage.aplSequence")}</h2>
        <p className="muted">{t("resultsPage.aplHelp")}</p>
        {normalized.apl_sequence.length ? <div className="table-scroll"><table><thead><tr><th scope="col">{t("resultsPage.time")}</th><th scope="col">{t("resultsPage.action")}</th><th scope="col">{t("resultsPage.target")}</th><th scope="col">{t("resultsPage.resourceState")}</th></tr></thead><tbody>{normalized.apl_sequence.map((action, index) => <tr key={`${action.time_seconds}-${action.internal_name}-${index}`}><td>{number(action.time_seconds)}s</td><th scope="row">{entityName("spell", action.id, action.name, action.internal_name, [])}</th><td>{action.target}</td><td>{resourceSnapshot(action.resources, action.resource_max)}</td></tr>)}</tbody></table></div> : <p className="muted">{t("resultsPage.noApl")}</p>}
      </section>
      <section className="identity-card"><h2>{t("resultsPage.runtime")}</h2><p>{t("resultsPage.simc", { version: normalized.runtime.simc_version, revision: normalized.runtime.git_revision })}</p><p>{t("resultsPage.game", { version: normalized.runtime.game_version, build: normalized.runtime.game_build })}</p><p>{t("resultsPage.iterations", { count: normalized.options.iterations.toLocaleString(i18n.language), threads: normalized.options.threads })}</p></section>
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
