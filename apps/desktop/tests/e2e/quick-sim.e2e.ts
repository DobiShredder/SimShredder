describe("supported desktop shell", () => {
  it("opens the real Tauri app and prepares an exact Quick Sim preview", async () => {
    const autoInstall = process.env.SIMSHREDDER_E2E_AUTO_INSTALL === "1";
    const mode = process.env.SIMSHREDDER_E2E_SIMC || autoInstall ? "live" : "offline";
    const languageControl = await $(".topbar select");
    await languageControl.waitForExist({ timeout: 20_000 });
    await languageControl.waitForDisplayed({ timeout: 20_000 });
    await browser.execute((locale: string) => {
      const select = document.querySelector<HTMLSelectElement>(".topbar select");
      if (!select) throw new Error("language control is missing");
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set;
      setter?.call(select, locale);
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }, "en");

    const keyboardContract = await browser.execute(() => {
      const controls = [...document.querySelectorAll<HTMLElement>("a[href], button:not([disabled]), select, input, textarea")]
        .filter((control) => getComputedStyle(control).display !== "none");
      return controls.slice(0, 3).map((control) => ({
        tag: control.tagName,
        name: control.getAttribute("aria-label") ?? control.textContent?.trim(),
      }));
    });
    expect(keyboardContract).toEqual([
      { tag: "A", name: "Skip to content" },
      { tag: "BUTTON", name: "Home" },
      { tag: "BUTTON", name: "Import" },
    ]);
    await (await $('button[aria-label="Import"]')).click();
    await expect($("h1")).toHaveText("Bring your character into focus.");
    await (await $("button=Home")).click();
    await expect($("h1")).toHaveText("Simulate and compare your WoW gear.");
    expect(await browser.checkScreen(`home-en-${mode}`, { ignoreAntialiasing: true })).toBeLessThan(2);

    await (await $("button=Settings")).click();
    await expect($("h1")).toHaveText("Prepare SimulationCraft");
    const runtimeCheck = await $(".runtime-progress");
    if (await runtimeCheck.isExisting()) {
      await runtimeCheck.waitForExist({ reverse: true, timeout: 20_000 });
    }
    if (autoInstall) {
      const install = await $("button=Download and install");
      await install.waitForDisplayed({ timeout: 20_000 });
      await install.click();
      await expect($(".indeterminate")).toBeDisplayed();
      await browser.waitUntil(async () => {
        const diagnostic = await $(".inline-error code");
        if (await diagnostic.isExisting()) throw new Error(`Automatic SimC install failed: ${await diagnostic.getText()}`);
        const status = await $(".runtime-pill span");
        return await status.isExisting() && await status.getText() === "Ready";
      }, { timeout: 660_000, interval: 1_000 });
    }
    await (await $("button[aria-label*='Appearance']")).click();
    expect(await browser.checkScreen(`settings-en-light-${mode}`, { ignoreAntialiasing: true })).toBeLessThan(2);

    if (mode === "offline") {
      await (await $("h2=Storage locations")).waitForDisplayed();
      await browser.execute(() => {
        const form = document.querySelector<HTMLElement>(".storage-form");
        if (form) window.scrollBy({ top: form.getBoundingClientRect().top - 121, behavior: "auto" });
      });
      const storageConfig = await $(".storage-form").getText();
      expect(storageConfig).not.toContain("dev.simshredder.desktop");
      const exportsInput = await $("#storage-exports");
      const defaultExport = await exportsInput.getValue();
      await exportsInput.setValue(`${defaultExport}-custom`);
      await (await $("button=Save locations")).click();
      await expect($("p=Storage locations saved.")).toBeDisplayed();
      const restoreDefaults = await $("button=Restore defaults");
      await restoreDefaults.waitForEnabled({ timeout: 120_000 });
      await restoreDefaults.click();
      await browser.waitUntil(async () => await $("#storage-exports").getValue() === defaultExport, { timeout: 20_000 });
      await restoreDefaults.waitForEnabled({ timeout: 120_000 });
      await browser.execute(() => {
        const form = document.querySelector<HTMLElement>(".storage-form");
        if (form) window.scrollBy({ top: form.getBoundingClientRect().top - 121, behavior: "auto" });
      });
      await browser.waitUntil(async () => browser.execute(() => {
        const button = document.querySelector<HTMLButtonElement>(".storage-browse-button");
        return button ? getComputedStyle(button).backgroundColor === "rgb(255, 255, 255)" : false;
      }), { timeout: 5_000, interval: 50 });
      const storageVisualContract = await browser.execute(() => {
        const button = document.querySelector<HTMLButtonElement>(".storage-browse-button");
        return {
          theme: document.documentElement.dataset.theme,
          disabled: button?.disabled,
          background: button ? getComputedStyle(button).backgroundColor : "",
        };
      });
      expect(storageVisualContract).toEqual({ theme: "light", disabled: false, background: "rgb(255, 255, 255)" });
      expect(await browser.checkScreen(`storage-en-light-${mode}`, { ignoreAntialiasing: true })).toBeLessThan(2);
    }

    await (await $("button=Import")).click();
    await expect($("h1")).toHaveText("Bring your character into focus.");
    await browser.execute(() => {
      (document.activeElement as HTMLElement | null)?.blur();
      window.scrollTo({ top: 0, left: 0, behavior: "auto" });
    });
    expect(await browser.checkScreen(`import-en-light-${mode}`, { ignoreAntialiasing: true })).toBeLessThan(2);
    await (await $("button[aria-label*='Appearance']")).click();
    await (await $("label*=.simc file")).click();
    await (await $("textarea")).setValue(
      "warrior=DesktopE2E\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nclass_talents=all\nspec_talents=all\nhero_talents=1\nload_default_gear=1\nhead=,id=154029\n\n### Gear from Bags\n# Candidate Helm\n# head=,id=154029,bonus_id=100/200\n",
    );
    await (await $("button=Review profile")).click();
    const reviewHeading = await $("h1=Review every input before the run.");
    await reviewHeading.waitForDisplayed({ timeout: 60_000 });
    await expect($("pre")).toHaveText(expect.stringContaining('warrior="DesktopE2E"'));
    await expect($("pre")).toHaveText(expect.stringContaining("iterations=10000"));
    const talentTooltipContract = await browser.execute(() => {
      const trigger = document.querySelector<HTMLButtonElement>(".semantic-icon-talent");
      const panel = trigger?.nextElementSibling as HTMLElement | null;
      return {
        focusable: trigger?.tabIndex === 0,
        text: panel?.textContent ?? "",
        externalAction: Boolean(panel?.querySelector(".tooltip-link")),
      };
    });
    expect(talentTooltipContract).toEqual(expect.objectContaining({ focusable: true, externalAction: false }));
    expect(talentTooltipContract.text).toContain("Class talents");
    expect(talentTooltipContract.text).toContain("Hero talents");

    if (mode === "live") {
      const numericInputs = await $$("input[type='number']");
      await numericInputs[0].setValue("100");
      await numericInputs[1].setValue("30");
      await (await $("button=Update preview")).click();
      await expect($("pre")).toHaveText(expect.stringContaining("iterations=100\n"));
      await expect($("pre")).toHaveText(expect.stringContaining("max_time=30\n"));
    }

    await browser.execute((locale: string) => {
      const select = document.querySelector<HTMLSelectElement>(".topbar select");
      if (!select) throw new Error("language control is missing");
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set;
      setter?.call(select, locale);
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }, "ko");
    await expect($("h1")).toHaveText("실행 전 모든 입력을 확인하세요.");
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    expect(await browser.checkScreen(`quick-ko-${mode}`, { ignoreAntialiasing: true })).toBeLessThan(2);

    await browser.execute(() => {
      document.documentElement.style.fontSize = "200%";
    });
    await expect($("button=빠른 심크 시작")).toBeDisplayed();
    const layout = await browser.execute(() => ({
      documentWidth: document.documentElement.scrollWidth,
      viewportWidth: document.documentElement.clientWidth,
      generatedPanelWidth: document.querySelector<HTMLElement>(".generated-panel")?.getBoundingClientRect().width ?? 0,
    }));
    expect(layout.documentWidth).toBeLessThanOrEqual(layout.viewportWidth);
    expect(layout.generatedPanelWidth).toBeGreaterThanOrEqual(320);
    await browser.execute(() => window.scrollTo({ top: 0, left: 0, behavior: "auto" }));
    expect(await browser.checkScreen(`quick-ko-200-percent-${mode}`, { ignoreAntialiasing: true })).toBeLessThan(2);

    await browser.execute(() => { document.documentElement.style.fontSize = "100%"; });
    if (mode === "live") {
      await (await $("button=빠른 심크 시작")).click();
      await browser.waitUntil(async () => {
        const diagnostic = await $(".inline-error code");
        if (await diagnostic.isExisting()) throw new Error(`Quick Sim failed: ${await diagnostic.getText()}`);
        const state = await $(".job-title strong");
        return await state.isExisting() && await state.getText() === "완료";
      }, { timeout: autoInstall ? 300_000 : 120_000 });
      await (await $("button=결과 보기")).click();
      const resultHeading = await $("h1*=DesktopE2E");
      await resultHeading.waitForDisplayed({ timeout: 60_000 });
      await expect(resultHeading).toHaveText(expect.stringContaining("DesktopE2E"));
      await expect($("h2=피해 및 치유 내역")).toBeDisplayed();
      await expect($("h2=자원")).toBeDisplayed();
      await expect($("h2=버프")).toBeDisplayed();
      await expect($("h2=행동 순서 표본")).toBeDisplayed();
      const spellTooltipContract = await browser.execute(() => {
        const trigger = document.querySelector<HTMLButtonElement>(".semantic-icon-spell");
        const panel = trigger?.nextElementSibling as HTMLElement | null;
        return { focusable: trigger?.tabIndex === 0, text: panel?.textContent ?? "" };
      });
      expect(spellTooltipContract.focusable).toBe(true);
      expect(spellTooltipContract.text).toContain("Wowhead에서 자세히 보기");
      await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
      await expect($("h2=실행 산출물")).toBeDisplayed();
      expect(await browser.checkScreen("result-ko-dark", {
        ignoreAntialiasing: true,
        ignore: [$(".metric-grid"), $(".result-chart")],
      })).toBeLessThan(3);
      await (await $("button=검증된 산출물 내보내기")).click();
      await expect($("p*=파일 5개를 내보냈습니다")).toBeDisplayed();
    }

    await (await $("button=최고 장비")).click();
    const topGearHeading = await $("h1=강화 하나까지 근거를 갖고 선택하세요.");
    await topGearHeading.waitForDisplayed({ timeout: 60_000 });
    await expect($("h2=정확한 실행 미리보기")).toBeDisplayed();
    const itemTooltipContract = await browser.execute(() => {
      const trigger = document.querySelector<HTMLButtonElement>(".semantic-icon-item");
      const panel = trigger?.nextElementSibling as HTMLElement | null;
      return { focusable: trigger?.tabIndex === 0, text: panel?.textContent ?? "" };
    });
    expect(itemTooltipContract.focusable).toBe(true);
    expect(itemTooltipContract.text).toContain("장비");
    expect(itemTooltipContract.text).toContain("Wowhead에서 자세히 보기");
    await browser.execute(() => {
      (document.activeElement as HTMLElement | null)?.blur();
      window.scrollTo({ top: 0, left: 0, behavior: "auto" });
    });
    expect(await browser.checkScreen(`top-gear-ko-${mode}`, { ignoreAntialiasing: true })).toBeLessThan(3);

    if (mode === "live") {
      await (await $("//label[contains(.,'넓은 탐색 반복 횟수')]//input")).setValue("100");
      await (await $("//label[contains(.,'최종 후보 반복 횟수')]//input")).setValue("100");
      await (await $("//label[contains(.,'고정밀 최종 후보 수')]//input")).setValue("2");
      await (await $("button=미리보기 갱신")).click();
      const startTopGear = await $("button=최고 장비 시작");
      await startTopGear.waitForEnabled({ timeout: 20_000 });
      await startTopGear.click();
      await browser.waitUntil(async () => {
        const diagnostic = await $(".inline-error code");
        if (await diagnostic.isExisting()) throw new Error(`Top Gear start failed: ${await diagnostic.getText()}`);
        return await $(".job-card").isExisting();
      }, { timeout: 90_000 });
      const continueStage = await $("button=다음 검증 단계 계속");
      await continueStage.waitForDisplayed({ timeout: autoInstall ? 300_000 : 120_000 });
      await continueStage.waitForEnabled({ timeout: 20_000 });
      await continueStage.click();
      await browser.waitUntil(async () => {
        const stage = await $(".job-title strong").getText();
        const diagnostic = await $(".inline-error code");
        if (await diagnostic.isExisting()) throw new Error(`Top Gear advance failed: ${await diagnostic.getText()}`);
        return stage === "고정밀 최종 후보 검증";
      }, { timeout: 90_000 });
      const finalizeStage = await $("button=다음 검증 단계 계속");
      await finalizeStage.waitForDisplayed({ timeout: autoInstall ? 300_000 : 120_000 });
      await finalizeStage.waitForEnabled({ timeout: 20_000 });
      await finalizeStage.click();
      const finalInput = await $("h2=최종 검증 .simc 입력");
      await finalInput.waitForDisplayed({ timeout: autoInstall ? 300_000 : 180_000 });
      await expect($("h2=검증된 장비 조합 순위")).toBeDisplayed();
      await (await $("button=검증된 최고 장비 산출물 내보내기")).click();
      await expect($("p*=파일 5개를 내보냈습니다")).toBeDisplayed();
    }
  });
});
