# Releasing

1. Update the version in `Cargo.toml` (`[workspace.package]`) and
   `apps/desktop/src-tauri/tauri.conf.json`, and move the "Unreleased" notes
   in `CHANGELOG.md` under the new version.
2. Commit, then tag and push: `git tag v0.1.0 && git push origin v0.1.0`.
3. The **Release** workflow builds installers for Windows (.msi, .exe),
   macOS (.dmg, Apple Silicon and Intel) and Linux (.AppImage, .deb, .rpm),
   plus standalone `meshmind-mcp` binaries, and attaches them to a **draft**
   GitHub release.
4. Check the draft, paste the changelog section, and publish.

## Code signing (optional, recommended)

Unsigned builds work, but macOS Gatekeeper and Windows SmartScreen warn users.
The workflow signs automatically when these repository secrets exist:

**macOS** (Apple Developer Program membership needed):

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of your "Developer ID Application" .p12 |
| `APPLE_CERTIFICATE_PASSWORD` | Password of that .p12 |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | For notarization (`APPLE_PASSWORD` is an app-specific password) |

**Windows**: set `WINDOWS_CERTIFICATE` (base64 .pfx) and
`WINDOWS_CERTIFICATE_PASSWORD`. Azure Trusted Signing also works; see the
[Tauri signing guide](https://v2.tauri.app/distribute/sign/windows/).

Without the secrets, the workflow still produces unsigned installers.
