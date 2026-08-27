# Third-party notices

SimShredder is licensed under Apache-2.0. The following direct desktop dependencies are distributed under their own licenses. The exact resolved dependency graph is recorded in `pnpm-lock.yaml` and `Cargo.lock`; release packaging will include the complete license texts required by that graph.

| Component | Purpose | License |
|---|---|---|
| Tauri, `@tauri-apps/*` and `tauri-plugin-opener` | Desktop shell, bridge and allowlisted OS-browser handoff | Apache-2.0 OR MIT |
| React and React DOM | User interface | MIT |
| Vite and Vitest | Build and test tooling | MIT |
| TypeScript | Type checking | Apache-2.0 |
| Radix Primitives | Accessible interaction primitives | MIT |
| Apache ECharts | Result charts | Apache-2.0 |
| Lucide | General user-interface icons | ISC |
| i18next and react-i18next | Localization | MIT |
| Testing Library and jsdom | Component testing | MIT |
| esbuild | Build tooling | MIT |
| atomic-write-file | Cross-platform atomic state replacement | BSD-3-Clause |
| base64 | Signed catalog encoding | MIT OR Apache-2.0 |
| ed25519-dalek | Runtime catalog signature verification | BSD-3-Clause |
| reqwest and rustls | Bounded HTTPS catalog and official HTTP/HTTPS artifact retrieval | MIT OR Apache-2.0 |
| serde and serde_json | Versioned data serialization | MIT OR Apache-2.0 |
| SHA-2 | Artifact and catalog integrity digests | MIT OR Apache-2.0 |
| sevenz-rust2 | Bounded selective extraction of official Windows archives | MIT OR Apache-2.0 |
| windows-spawn | Windows process creation and Job Object tree lifetime | MIT OR Apache-2.0 |
| rusqlite and SQLite | Local persistent queue and metadata | MIT; public domain |
| image | Bounded raster icon decoding and validation | MIT OR Apache-2.0 |
| clap | Development contract command-line interface | MIT OR Apache-2.0 |

The SimShredder signal-mark SVG and generated application icons are original project assets. No World of Warcraft, Blizzard, SimulationCraft, Raidbots, or Wowhead image is included in the repository or application bundle.

SimulationCraft is a separate GPL-3.0 project. SimShredder does not bundle or redistribute its executable; the managed runtime flow downloads it from the official SimulationCraft server directly to the user's per-user application data directory.

Release workflows generate complete locked production dependency license reports with cargo-about and `pnpm licenses list`, fail when a Node package has no packaged license text, and compare them byte-for-byte with the checked-in reports. Both reports are bundled inside the application and placed next to each signed installer. These generated reports supplement this concise direct-dependency notice.
