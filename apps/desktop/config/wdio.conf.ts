import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const windows = process.platform === "win32";
const application = path.resolve(`../../target/debug/simshredder-desktop${windows ? ".exe" : ""}`);
const testAppData = path.resolve("../../target/wdio-app-data");
const autoInstall = process.env.SIMSHREDDER_E2E_AUTO_INSTALL === "1";
const baselineVariant = windows ? "windows-ci" : process.env.CI ? "macos-ci" : "macos-retina";
const baselineFolder = path.resolve("tests/e2e/baseline", baselineVariant);
const e2eWindow = { width: 1024, height: 674 };

if (process.env.SIMSHREDDER_E2E_ACCEPT_BASELINE === "1" && process.env.CI) {
  throw new Error("baseline acceptance is only allowed during an intentional local review");
}

function prepareLiveRuntime() {
  process.env.SIMSHREDDER_TEST_APP_DATA = testAppData;
  fs.rmSync(testAppData, { recursive: true, force: true });
  if (autoInstall) {
    const artifact = process.env.SIMSHREDDER_E2E_RUNTIME_ARTIFACT;
    if (artifact) {
      if (!fs.statSync(artifact).isFile()) throw new Error(`runtime artifact is not a file: ${artifact}`);
      const downloads = path.join(testAppData, "simulationcraft", "downloads");
      fs.mkdirSync(downloads, { recursive: true, mode: 0o700 });
      fs.copyFileSync(artifact, path.join(downloads, path.basename(artifact)));
    }
    return;
  }
  const source = process.env.SIMSHREDDER_E2E_SIMC;
  if (!source) return;
  const runtimeBuild = process.env.SIMSHREDDER_E2E_SIMC_BUILD ?? "02b39ce";
  const runtimeGameVersion = process.env.SIMSHREDDER_E2E_SIMC_GAME_VERSION ?? "12.1.0.69497";
  const id = `1210-01-${runtimeBuild}`;
  const runtimeRoot = path.join(testAppData, "simulationcraft");
  const installRoot = path.join(runtimeRoot, "runtimes", id);
  fs.mkdirSync(installRoot, { recursive: true, mode: 0o700 });
  const executable = path.join(installRoot, windows ? "simc.exe" : "simc");
  try { fs.linkSync(source, executable); } catch { fs.copyFileSync(source, executable); }
  if (!windows) fs.chmodSync(executable, 0o700);
  const digest = crypto.createHash("sha256").update(fs.readFileSync(executable)).digest("hex");
  const state = {
    schema_version: 1,
    active_id: id,
    previous_id: null,
    runtimes: [{
      id,
      simcVersion: "1210-01",
      build: runtimeBuild,
      gameVersion: runtimeGameVersion,
      channel: "live",
      executableSha256: digest,
      installedAtUnixSeconds: 1,
    }],
  };
  fs.writeFileSync(path.join(runtimeRoot, "runtime-state.json"), JSON.stringify(state));
  if (!windows) fs.chmodSync(path.join(runtimeRoot, "runtime-state.json"), 0o600);
}

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["../tests/e2e/**/*.e2e.ts"],
  maxInstances: 1,
  services: [
    [
      "tauri",
      {
        appBinaryPath: application,
        driverProvider: "embedded",
        embeddedPort: 4445,
        env: { SIMSHREDDER_TEST_APP_DATA: testAppData, RUST_BACKTRACE: "1" },
      },
    ],
    [
      "visual",
      {
        baselineFolder,
        screenshotPath: path.resolve("../../target/wdio-visual"),
        formatImageName: "{tag}-{width}x{height}",
        autoSaveBaseline: process.env.SIMSHREDDER_E2E_ACCEPT_BASELINE === "1",
        alwaysSaveActualImage: true,
        compareOptions: { scaleImagesToSameSize: true },
      },
    ],
  ],
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": { application },
    },
  ],
  logLevel: "warn",
  bail: 0,
  waitforTimeout: 15_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 2,
  framework: "mocha",
  reporters: ["spec"],
  onPrepare: prepareLiveRuntime,
  before: async () => {
    const scaleFactor = await browser.execute(() => window.devicePixelRatio || 1);
    await browser.setWindowSize(
      Math.round(e2eWindow.width * scaleFactor),
      Math.round(e2eWindow.height * scaleFactor),
    );
  },
  mochaOpts: {
    ui: "bdd",
    timeout: autoInstall ? 1_200_000 : process.env.SIMSHREDDER_E2E_SIMC ? 600_000 : 240_000,
  },
};
