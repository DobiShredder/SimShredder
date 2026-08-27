# SimShredder privacy and network behavior

SimShredder is a local desktop application. It does not operate an account service, upload character profiles or simulation results, send analytics, or submit crash reports.

## Network requests

The production application may make only these outbound requests:

1. `https://github.com/DobiShredder/SimShredder/releases/download/simc-runtime-catalog/runtime-catalog.json` to check the signed SimulationCraft runtime catalog. The client follows at most one redirect, and only to the HTTPS GitHub release-asset host and path expected for that exact release asset.
2. `http://downloads.simulationcraft.org/nightly/<pinned-file>` (or HTTPS after upstream certificate repair) to download the exact official SimulationCraft artifact selected by that catalog.
3. Only after the user selects **View on Wowhead**, the operating system's default browser opens `https://www.wowhead.com/item=<numeric-id>` or `https://www.wowhead.com/spell=<numeric-id>`. SimShredder does not fetch that page inside the application.

The SimulationCraft artifact client rejects redirects; the catalog client permits only the single GitHub asset redirect described above. Both clients bound response sizes and timeouts. The catalog must pass Ed25519 signature, expiry, monotonic sequence, target, URL, size, and SHA-256 validation. Because the official nightly host currently presents an invalid HTTPS certificate, SimShredder does not bypass TLS verification and instead permits the exact official HTTP artifact named by the signed catalog. The public artifact must match the signed size and SHA-256 before installation. SimulationCraft is separate software and is downloaded directly from its official server, not redistributed by SimShredder.

All `0.x` test versions disable remote WoW icon providers and do not display WoW artwork. Built-in semantic placeholders and locally rendered details keep all simulation flows available offline. There is no Blizzard API, advertising, telemetry, analytics, or third-party tooltip request in `0.x`. The optional Wowhead browser action sends only the selected numeric item or spell ID in the page path; it does not include a character name, realm, profile, result, or tracking parameter. A future `1.0.0` icon provider requires a separate privacy and release audit before activation.

The operating system and contacted servers necessarily receive ordinary connection metadata such as IP address, time, available TLS information, and HTTP headers. An HTTP artifact request and response are not encrypted, so the requested public filename and downloaded bytes can be observed in transit; the signed hash detects modification but does not provide confidentiality. SimShredder does not add a character name, realm, profile, result, or installation identifier to these requests.

## Local data

Profiles, generated `.simc` input, job state, logs, JSON, HTML, results, managed SimulationCraft versions, signed catalog state, and icon cache metadata stay on the current user's device. The default control root is `~/Library/Application Support/SimShredder` on macOS and `%LOCALAPPDATA%\SimShredder` on Windows. Default exports use the user's `Documents/SimShredder Exports` directory.

Settings lets users independently choose the workspace/history, managed SimulationCraft, icon cache, and export directories by typing an absolute path or using the operating system's native folder picker. The small `storage-settings.json` preference file remains in the fixed control root so SimShredder can find those choices on its next launch. Custom directories must be separate, non-nested regular directories. Changing a location does not copy, move, or delete existing data; selecting the previous directory restores access to it.

Runtime and job writes use staged or atomic replacement where their integrity matters. SimShredder does not require administrator access, a system service, system-wide PATH changes, or machine-wide registry changes.

Users can clear the icon cache in Settings. Removing only the fixed control root deletes preferences and any data still stored under its default subdirectories, but it does not delete data in custom directories. Custom workspace, SimulationCraft, icon-cache, and export directories must be removed separately. Uninstalling the application does not silently delete user-created data or exports.

## Changes

Any future endpoint, telemetry, cloud execution, Blizzard API, or remote icon provider is outside the `0.x` contract and requires documentation and release audit before activation. The planned `1.0.0` icon design uses a scheduled, signed static catalog rather than an always-on credential broker; its exact public URLs, expiry behavior and request metadata will be documented before release.
