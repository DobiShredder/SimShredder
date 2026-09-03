import { CopyPlus, RotateCcw, Save, Undo2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RunReference } from "../runs";

export function RunManagementControls({ run, displayName, defaultName, canEdit, onRerun, onEditRerun, onRename }: {
  run: RunReference;
  displayName: string;
  defaultName: string;
  canEdit: boolean;
  onRerun: (run: RunReference) => Promise<void>;
  onEditRerun: (run: RunReference) => Promise<void>;
  onRename: (run: RunReference, name: string | null) => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(displayName);
  const [busy, setBusy] = useState<"rerun" | "edit" | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setName(displayName), [displayName, run]);

  const execute = async (kind: "rerun" | "edit") => {
    setBusy(kind); setError(null);
    try { await (kind === "rerun" ? onRerun(run) : onEditRerun(run)); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(null); }
  };

  return <section className="run-management" aria-label={t("runs.manage")}>
    <form onSubmit={(event) => { event.preventDefault(); onRename(run, name === defaultName ? null : name); }}>
      <label>{t("runs.name")}<input maxLength={80} value={name} onChange={(event) => setName(event.target.value)} /></label>
      <button className="secondary-button" disabled={!name.trim()} type="submit"><Save aria-hidden="true" size={17} />{t("runs.saveName")}</button>
      <button className="secondary-button" disabled={name === defaultName} type="button" onClick={() => { setName(defaultName); onRename(run, null); }}><Undo2 aria-hidden="true" size={17} />{t("runs.resetName")}</button>
    </form>
    <div className="button-row">
      <button className="secondary-button" disabled={busy !== null} type="button" onClick={() => void execute("rerun")}><RotateCcw aria-hidden="true" size={17} />{busy === "rerun" ? t("runs.starting") : t("runs.rerunExact")}</button>
      <button className="primary-button" disabled={busy !== null || !canEdit} title={canEdit ? undefined : t("runs.editUnavailable")} type="button" onClick={() => void execute("edit")}><CopyPlus aria-hidden="true" size={17} />{busy === "edit" ? t("runs.openingEditor") : t("runs.duplicateEdit")}</button>
    </div>
    {!canEdit ? <p className="muted">{t("runs.editUnavailable")}</p> : null}
    {error ? <div className="inline-error" role="alert"><code>{error}</code></div> : null}
  </section>;
}
