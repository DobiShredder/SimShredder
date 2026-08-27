import { invoke } from "@tauri-apps/api/core";
import { Box, ExternalLink, Sparkles } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

export type TooltipKind = "item" | "spell" | "talent" | "buff";
export type TooltipDetail = { label: string; value: string };
export type TooltipModel = {
  kind: TooltipKind;
  id: number | null;
  title: string;
  category: string;
  details: TooltipDetail[];
};

export type ItemTooltipInput = {
  id: number;
  slot: string;
  itemLevel?: string;
  rank: number;
  gemIds: number[];
  enchantId: number | null;
  changed: boolean;
};

export function itemTooltipModel(
  input: ItemTooltipInput,
  text: {
    title: string;
    category: string;
    itemLevel: string;
    rank: string;
    gems: string;
    enchant: string;
    state: string;
    candidate: string;
    worn: string;
    none: string;
  },
): TooltipModel {
  const details: TooltipDetail[] = [];
  if (input.itemLevel) details.push({ label: text.itemLevel, value: input.itemLevel });
  details.push({ label: text.rank, value: String(input.rank) });
  details.push({ label: text.gems, value: input.gemIds.length ? input.gemIds.join(" / ") : text.none });
  details.push({ label: text.enchant, value: input.enchantId ? String(input.enchantId) : text.none });
  details.push({ label: text.state, value: input.changed ? text.candidate : text.worn });
  return { kind: "item", id: input.id, title: text.title, category: text.category, details };
}

export function wowheadReferenceKind(kind: TooltipKind): "item" | "spell" {
  return kind === "item" ? "item" : "spell";
}

export function openWowheadReference(model: Pick<TooltipModel, "kind" | "id">): Promise<void> {
  if (model.id === null || !Number.isSafeInteger(model.id) || model.id <= 0 || model.id > 0xffff_ffff) {
    return Promise.reject(new Error("invalid external reference ID"));
  }
  return invoke("open_wowhead_reference", { kind: wowheadReferenceKind(model.kind), id: model.id });
}

export function EntityTooltip({ model }: { model: TooltipModel }) {
  const { t } = useTranslation();
  const tooltipId = useId();
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState(false);
  const Icon = model.kind === "item" ? Box : Sparkles;

  return (
    <span className="entity-tooltip">
      <button
        aria-describedby={tooltipId}
        aria-label={t("tooltip.show", { title: model.title })}
        className={`semantic-icon semantic-icon-${model.kind}`}
        type="button"
      >
        <Icon aria-hidden="true" size={18} />
      </button>
      <span className="entity-tooltip-panel" id={tooltipId} role="tooltip">
        <span className="entity-tooltip-heading">
          <strong>{model.title}</strong>
          <small>{model.category}{model.id === null ? "" : ` · ID ${model.id}`}</small>
        </span>
        <span className="entity-tooltip-details">
          {model.details.map((detail) => <span key={detail.label}><small>{detail.label}</small><strong>{detail.value}</strong></span>)}
        </span>
        {model.id === null ? null : <button className="tooltip-link" disabled={opening} type="button" onClick={() => {
          setOpening(true);
          setError(false);
          void openWowheadReference(model).catch(() => setError(true)).finally(() => setOpening(false));
        }}>
          <ExternalLink aria-hidden="true" size={14} />
          {opening ? t("tooltip.opening") : t("tooltip.openWowhead")}
        </button>}
        {error ? <span className="tooltip-error" role="alert">{t("tooltip.openError")}</span> : null}
      </span>
    </span>
  );
}
