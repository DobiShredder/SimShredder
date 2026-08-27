import { FileInput, Upload } from "lucide-react";
import { useRef, useState, type DragEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  defaultQuickRequest,
  quickPrepare,
  type PreparedQuickSim,
  type QuickSimRequest,
  type SourceFormat,
} from "../quick";

export function ImportPage({ onPrepared }: {
  onPrepared: (request: QuickSimRequest, preview: PreparedQuickSim) => void;
}) {
  const { t } = useTranslation();
  const [source, setSource] = useState("");
  const [format, setFormat] = useState<SourceFormat>("addonExport");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const loadFile = async (file: File) => {
    if (file.size > 2 * 1024 * 1024) {
      setError(t("importPage.fileTooLarge"));
      return;
    }
    setSource(await file.text());
    setFormat("simcFile");
    setError(null);
  };
  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const file = event.dataTransfer.files.item(0);
    if (file) void loadFile(file);
  };
  const review = async () => {
    setBusy(true);
    setError(null);
    const request = defaultQuickRequest(source, format);
    try {
      onPrepared(request, await quickPrepare(request));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page import-page">
      <p className="eyebrow">{t("importPage.eyebrow")}</p>
      <h1>{t("importPage.title")}</h1>
      <p className="settings-lead">{t("importPage.body")}</p>
      <fieldset className="format-choice">
        <legend>{t("importPage.sourceLabel")}</legend>
        {(["addonExport", "simcFile"] as const).map((value) => (
          <label key={value}>
            <input checked={format === value} name="source-format" onChange={() => setFormat(value)} type="radio" />
            <span>{t(value === "addonExport" ? "importPage.addon" : "importPage.simc")}</span>
          </label>
        ))}
      </fieldset>
      <div className="source-drop" onDragOver={(event) => event.preventDefault()} onDrop={handleDrop}>
        <textarea aria-label={t("importPage.sourceLabel")} onChange={(event) => setSource(event.target.value)} placeholder={t("importPage.sourcePlaceholder")} spellCheck={false} value={source} />
        <div className="source-actions">
          <span><Upload aria-hidden="true" size={16} />{t("importPage.dropHint")}</span>
          <button className="secondary-button" type="button" onClick={() => fileInput.current?.click()}><FileInput aria-hidden="true" size={17} />{t("importPage.chooseFile")}</button>
          <input ref={fileInput} accept=".simc,text/plain" aria-label={t("importPage.chooseFile")} className="sr-only" onChange={(event) => { const file = event.target.files?.item(0); if (file) void loadFile(file); }} type="file" />
        </div>
      </div>
      {error ? <div className="inline-error" role="alert"><strong>{t("importPage.errorTitle")}</strong><code>{error}</code></div> : null}
      <div className="button-row">
        <button className="primary-button" disabled={busy || !source.trim()} type="button" onClick={() => void review()}><FileInput aria-hidden="true" size={18} />{busy ? t("importPage.reviewing") : t("importPage.review")}</button>
      </div>
    </div>
  );
}
