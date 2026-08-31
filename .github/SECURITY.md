# Security policy

## Supported releases

Security fixes are provided for the latest published SimShredder release on Apple Silicon macOS 26 or later and Windows x64 versions supported by the pinned official SimulationCraft release. Pre-release source snapshots and unsupported operating systems do not receive a separate security support promise.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability or leaked credential. Use the repository's **Security → Report a vulnerability** private advisory flow. Include the affected version, operating system, reproduction steps, expected impact, and whether the report involves a SimulationCraft download.

Do not include real World of Warcraft profile data, local paths, tokens, certificates, or private keys unless the maintainers explicitly request a minimal encrypted sample.

## Trust boundaries

- SimulationCraft is not bundled. The app reads the bounded official nightly listing, accepts only the exact supported filename for the current OS, forbids redirects, and downloads only from `downloads.simulationcraft.org/nightly/`.
- Runtime downloads must match the official server's bounded `Content-Length`. SimShredder records a local SHA-256, rejects unsafe archive contents, and validates the extracted executable architecture and Retail Live identity before activation. Installation stays inside the current user's application-data directory and requires no administrator access.
- Imported profiles, SimulationCraft output, SQLite state, icons and exports remain local. SimShredder has no telemetry or crash-upload endpoint.
- The application does not execute imported content through a shell and does not inject raw SimulationCraft HTML into the privileged application DOM.

If a release-signing certificate may be compromised, maintainers must stop releases, remove the affected secret from automation, and publish a security advisory before resuming distribution.
