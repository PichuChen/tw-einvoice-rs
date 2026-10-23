# Security policy

This project handles security-sensitive financial messaging. It is currently
pre-production and has no supported production release line yet.

Please do not publish vulnerabilities that expose real credentials,
certificates, taxpayer data, or production invoice payloads. Use a private
GitHub security advisory when available.

## Current automated controls

Every change is checked with formatting, strict Clippy, workspace tests, an
explicit Rust 1.88 minimum-version build, locked dependency resolution, RustSec
advisory scanning, CycloneDX SBOM generation, and a clean container build/start
smoke test. Dependency updates are monitored through Dependabot.

See [docs/supply-chain.md](docs/supply-chain.md) and
[docs/security-model.md](docs/security-model.md) for the detailed boundaries.

## Security-sensitive design choices

- SFTP host keys are mandatory and checked against an explicit OpenSSH
  `known_hosts` file.
- PFX and transport passwords are not accepted as literal CLI arguments.
- Signed outbound artifacts are fsynced before network submission.
- Inbound `/out` objects are fsynced, size-checked, and atomically published
  before the remote copy is deleted.
- Secret/key formats and runtime inbox/outbox data are excluded from the Docker
  build context.
- Public compatibility code does not include proprietary Turnkey binaries or
  decompiled source.

## Remaining pre-production work

Live Ministry test-environment interoperability, independent implementation
review, container/OS vulnerability scanning, release provenance/attestation,
and deployment-specific operational controls are still required before a
production release is declared supported.
