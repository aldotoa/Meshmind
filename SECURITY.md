# Security policy

## How MeshMind is exposed

- The MCP server listens on `127.0.0.1` only, never on the network.
- Requests with a browser `Origin` other than localhost are rejected, which
  blocks malicious web pages (DNS rebinding).
- The agent can read and write files through `export_glb`, `save_project` and
  `open_project`, with your user's permissions. Only connect agents you trust.
- MeshMind makes no network requests of its own.

## Reporting a vulnerability

Please report security problems privately through
[GitHub's private vulnerability reporting](https://github.com/aldotoa/meshmind/security/advisories/new),
not in public issues. We aim to reply within a week and will credit you in the
release notes unless you prefer otherwise.

## Supported versions

Security fixes go into the latest release.
