import { Save, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { QuickSimRequest } from "../quick";
import type { TopGearRequest } from "../topGear";
import { deletePreset, listPresets, savePreset, type WorkflowKind } from "../workflowPreferences";
import { useModalDialog } from "./useModalDialog";

type Request = QuickSimRequest | TopGearRequest;

export function PresetControls({ profileId, kind, request, builtIns, onApply }: {
  profileId: string;
  kind: WorkflowKind;
  request: Request;
  builtIns: Array<{ id: string; name: string; request: Request; summary: string }>;
  onApply: (request: Request) => void;
}) {
  const { t } = useTranslation();
  const [presets, setPresets] = useState(() => listPresets(profileId, kind));
  const [selected, setSelected] = useState("");
  const [name, setName] = useState("");
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  const deleteDialog = useModalDialog(Boolean(pendingDelete), () => setPendingDelete(null));
  useEffect(() => {
    setPresets(listPresets(profileId, kind));
    setSelected("");
    setName("");
  }, [kind, profileId]);

  const select = (id: string) => {
    setSelected(id);
    const builtIn = builtIns.find((candidate) => candidate.id === id);
    const custom = presets.find((candidate) => candidate.id === id);
    if (builtIn) {
      setName("");
      onApply(builtIn.request);
    } else if (custom) {
      setName(custom.name);
      onApply(custom.request);
    }
  };
  const save = () => {
    const existing = presets.find((preset) => preset.id === selected);
    const saved = savePreset(profileId, { id: existing?.id, kind, name, request });
    setPresets(listPresets(profileId, kind));
    setSelected(saved.id);
    setName(saved.name);
  };
  const confirmDelete = () => {
    if (!pendingDelete) return;
    deletePreset(profileId, pendingDelete);
    setPresets(listPresets(profileId, kind));
    setSelected("");
    setName("");
    setPendingDelete(null);
  };

  return <section className="preset-controls" aria-labelledby={`${kind}-preset-heading`}>
    <div>
      <h2 id={`${kind}-preset-heading`}>{t("presets.title")}</h2>
      <p>{t("presets.help")}</p>
    </div>
    <label>{t("presets.choose")}<select value={selected} onChange={(event) => select(event.target.value)}>
      <option value="">{t("presets.current")}</option>
      <optgroup label={t("presets.builtIn")}>{builtIns.map((preset) => <option value={preset.id} key={preset.id}>{preset.name} — {preset.summary}</option>)}</optgroup>
      {presets.length ? <optgroup label={t("presets.saved")}>{presets.map((preset) => <option value={preset.id} key={preset.id}>{preset.name}</option>)}</optgroup> : null}
    </select></label>
    <label>{t("presets.name")}<input maxLength={80} value={name} onChange={(event) => setName(event.target.value)} placeholder={t("presets.namePlaceholder")} /></label>
    <div className="button-row">
      <button className="secondary-button" disabled={!name.trim()} type="button" onClick={save}><Save aria-hidden="true" size={17} />{t(presets.some((preset) => preset.id === selected) ? "presets.update" : "presets.save")}</button>
      <button className="secondary-button danger-button" disabled={!presets.some((preset) => preset.id === selected)} type="button" onClick={() => setPendingDelete(selected)}><Trash2 aria-hidden="true" size={17} />{t("presets.delete")}</button>
    </div>
    {pendingDelete ? <div className="modal-backdrop"><dialog ref={deleteDialog} className="confirmation-dialog" open aria-labelledby="preset-delete-title"><h2 id="preset-delete-title">{t("presets.deleteTitle")}</h2><p>{t("presets.deleteBody", { name: presets.find((preset) => preset.id === pendingDelete)?.name })}</p><div className="button-row"><button className="secondary-button" data-modal-initial-focus type="button" onClick={() => setPendingDelete(null)}>{t("presets.cancel")}</button><button className="danger-button" type="button" onClick={confirmDelete}>{t("presets.delete")}</button></div></dialog></div> : null}
  </section>;
}
