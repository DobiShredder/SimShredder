import { invoke } from "@tauri-apps/api/core";
import { Box, ExternalLink, Sparkles } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
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
  const headingId = useId();
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState(false);
  const [visible, setVisible] = useState(false);
  const [position, setPosition] = useState({ left: 12, top: 12, width: 304 });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLSpanElement>(null);
  const closeTimer = useRef<number | null>(null);
  const suppressRestoredFocus = useRef(false);
  const Icon = model.kind === "item" ? Box : Sparkles;

  const cancelClose = () => {
    if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    closeTimer.current = null;
  };
  const show = () => {
    cancelClose();
    setVisible(true);
  };
  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = window.setTimeout(() => setVisible(false), 120);
  };
  const openForAction = () => {
    show();
    window.setTimeout(() => panelRef.current?.querySelector<HTMLButtonElement>(".tooltip-link")?.focus(), 0);
  };

  useLayoutEffect(() => {
    if (!visible) return;
    const place = () => {
      const trigger = triggerRef.current?.getBoundingClientRect();
      if (!trigger) return;
      const margin = 12;
      const gap = 6;
      const width = Math.min(304, Math.max(180, window.innerWidth - margin * 2));
      const height = panelRef.current?.getBoundingClientRect().height ?? 0;
      const left = Math.min(Math.max(margin, trigger.left), Math.max(margin, window.innerWidth - width - margin));
      const top = trigger.top - height - gap >= margin
        ? trigger.top - height - gap
        : Math.min(trigger.bottom + gap, Math.max(margin, window.innerHeight - height - margin));
      setPosition({ left, top, width });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [model, visible]);

  useEffect(() => {
    if (!visible) return;
    const dismiss = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!triggerRef.current?.contains(target) && !panelRef.current?.contains(target)) setVisible(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setVisible(false);
        suppressRestoredFocus.current = true;
        triggerRef.current?.focus();
      }
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", escape);
    };
  }, [visible]);

  useEffect(() => () => cancelClose(), []);

  const panel = visible ? createPortal(
    <span
      aria-labelledby={headingId}
      className="entity-tooltip-panel"
      id={tooltipId}
      onPointerEnter={cancelClose}
      onPointerLeave={scheduleClose}
      ref={panelRef}
      role="dialog"
      style={position}
    >
      <span className="entity-tooltip-heading" id={headingId}>
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
    </span>,
    document.body,
  ) : null;

  return (
    <span className="entity-tooltip" onPointerEnter={show} onPointerLeave={scheduleClose}>
      <button
        aria-controls={tooltipId}
        aria-expanded={visible}
        aria-haspopup="dialog"
        aria-label={t("tooltip.show", { title: model.title })}
        className={`semantic-icon semantic-icon-${model.kind}`}
        onClick={openForAction}
        onFocus={() => {
          if (suppressRestoredFocus.current) {
            suppressRestoredFocus.current = false;
            return;
          }
          show();
        }}
        ref={triggerRef}
        type="button"
      >
        <Icon aria-hidden="true" size={18} />
      </button>
      {panel}
    </span>
  );
}
