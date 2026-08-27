# Security policy

## Supported releases

Security fixes are provided for the latest published SimShredder release on Apple Silicon macOS 26 or later and Windows x64 versions supported by the pinned official SimulationCraft release. Pre-release source snapshots and unsupported operating systems do not receive a separate security support promise.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability or leaked credential. Use the repository's **Security → Report a vulnerability** private advisory flow. Include the affected version, operating system, reproduction steps, expected impact, and whether the report involves a SimulationCraft artifact or catalog signature.

Do not include real World of Warcraft profile data, local paths, tokens, certificates, or private keys unless the maintainers explicitly request a minimal encrypted sample.

## Trust boundaries

- SimulationCraft is not bundled. The app accepts only a signed, non-expired, monotonically increasing catalog and an exact HTTP or HTTPS artifact from the official SimulationCraft download host. Redirects are forbidden, and the signed size plus SHA-256 authenticate the public artifact when the upstream HTTPS certificate is unusable.
- Runtime downloads must match the catalog size and SHA-256 before extraction. Installation stays inside the current user's application-data directory and requires no administrator access.
- Imported profiles, SimulationCraft output, SQLite state, icons and exports remain local. SimShredder has no telemetry or crash-upload endpoint.
- The application does not execute imported content through a shell and does not inject raw SimulationCraft HTML into the privileged application DOM.

If a release-signing certificate or runtime-catalog key may be compromised, maintainers must stop releases, remove the affected secret from automation, rotate trust using a previously trusted key when possible, and publish a security advisory before resuming distribution.
