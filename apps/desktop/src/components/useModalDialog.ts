import { useEffect, useRef } from "react";

const focusable = "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex='-1'])";

export function useModalDialog(open: boolean, onClose: () => void, restoreFocus?: () => HTMLElement | null) {
  const dialog = useRef<HTMLDialogElement>(null);
  const close = useRef(onClose);
  const previousFocus = useRef<HTMLElement | null>(null);
  close.current = onClose;
  useEffect(() => {
    if (!open) {
      const rememberFocus = (event: FocusEvent) => {
        if (event.target instanceof HTMLElement && !dialog.current?.contains(event.target)) previousFocus.current = event.target;
      };
      if (document.activeElement instanceof HTMLElement) previousFocus.current = document.activeElement;
      document.addEventListener("focusin", rememberFocus);
      return () => document.removeEventListener("focusin", rememberFocus);
    }
    const previous = restoreFocus?.() ?? previousFocus.current;
    const element = dialog.current;
    if (!element) return;
    const controls = () => [...element.querySelectorAll<HTMLElement>(focusable)];
    (element.querySelector<HTMLElement>("[data-modal-initial-focus]") ?? controls()[0] ?? element).focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); close.current(); return; }
      if (event.key !== "Tab") return;
      const items = controls();
      if (!items.length) { event.preventDefault(); element.focus(); return; }
      const first = items[0]; const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    element.addEventListener("keydown", keydown);
    return () => { element.removeEventListener("keydown", keydown); previous?.focus(); };
  }, [open]);
  return dialog;
}
