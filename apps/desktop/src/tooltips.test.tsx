import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "./i18n";
import { EntityTooltip, itemTooltipModel, openWowheadReference, wowheadReferenceKind } from "./tooltips";

const { mockInvoke } = vi.hoisted(() => ({ mockInvoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

describe("local entity tooltips", () => {
  beforeEach(async () => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue(undefined);
    await i18n.changeLanguage("en");
  });

  it("builds an item tooltip only from local verified fields", () => {
    const model = itemTooltipModel({
      id: 154029,
      slot: "head",
      itemLevel: "700",
      rank: 2,
      gemIds: [213455, 213456],
      enchantId: 7414,
      changed: true,
    }, {
      title: "Item 154029",
      category: "Equipment · Head",
      itemLevel: "Item level",
      rank: "Upgrade rank",
      gems: "Gems",
      enchant: "Enchant",
      state: "State",
      candidate: "Candidate",
      worn: "Equipped",
      none: "None",
    });

    expect(model).toEqual({
      kind: "item",
      id: 154029,
      title: "Item 154029",
      category: "Equipment · Head",
      details: [
        { label: "Item level", value: "700" },
        { label: "Upgrade rank", value: "2" },
        { label: "Gems", value: "213455 / 213456" },
        { label: "Enchant", value: "7414" },
        { label: "State", value: "Candidate" },
      ],
    });
  });

  it("maps talent and buff references to spell pages without remote tooltip requests", async () => {
    expect(wowheadReferenceKind("item")).toBe("item");
    expect(wowheadReferenceKind("spell")).toBe("spell");
    expect(wowheadReferenceKind("talent")).toBe("spell");
    expect(wowheadReferenceKind("buff")).toBe("spell");

    await openWowheadReference({ kind: "talent", id: 184367 });
    expect(mockInvoke).toHaveBeenCalledWith("open_wowhead_reference", { kind: "spell", id: 184367 });
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    await expect(openWowheadReference({ kind: "item", id: 0 })).rejects.toThrow("invalid external reference ID");
  });

  it("shows a keyboard-focusable semantic placeholder and opens only on explicit action", async () => {
    const user = userEvent.setup();
    const model = itemTooltipModel({
      id: 154029,
      slot: "head",
      rank: 0,
      gemIds: [],
      enchantId: null,
      changed: false,
    }, {
      title: "Item 154029",
      category: "Equipment · Head",
      itemLevel: "Item level",
      rank: "Upgrade rank",
      gems: "Gems",
      enchant: "Enchant",
      state: "State",
      candidate: "Candidate",
      worn: "Equipped",
      none: "None",
    });
    render(<EntityTooltip model={model} />);

    const trigger = screen.getByRole("button", { name: "Show details for Item 154029" });
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(mockInvoke).not.toHaveBeenCalled();

    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("dialog")).toHaveTextContent("Equipment · Head · ID 154029");
    await user.click(screen.getByRole("button", { name: "View on Wowhead" }));
    expect(mockInvoke).toHaveBeenCalledWith("open_wowhead_reference", { kind: "item", id: 154029 });
  });

  it("renders an unidentified talent configuration locally without inventing an external reference", async () => {
    const user = userEvent.setup();
    render(<EntityTooltip model={{
      kind: "talent",
      id: null,
      title: "Talent configuration",
      category: "Talent",
      details: [{ label: "Talent loadout", value: "CgEAAAAAAAA" }],
    }} />);

    const trigger = screen.getByRole("button", { name: "Show details for Talent configuration" });
    expect(trigger).toBeVisible();
    await user.click(trigger);
    expect(screen.getByRole("dialog")).toHaveTextContent("TalentTalent loadoutCgEAAAAAAAA");
    expect(screen.queryByRole("button", { name: "View on Wowhead" })).not.toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
  });
});
