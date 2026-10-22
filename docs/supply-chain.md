# Software supply-chain controls

This project is intended to be inspectable by organizations that cannot accept
a closed binary as the only security assurance for invoice transmission.

## Reproducible dependency graph

`Cargo.lock` is committed for the complete application workspace. CI and the
container build use Cargo's `--locked` mode, so a pull request cannot silently
resolve a different dependency version without changing the lockfile.

The declared minimum supported Rust version is 1.88. CI invokes
`cargo +1.88.0` explicitly so the repository-level stable toolchain override
cannot turn the MSRV check into a false positive.

## Source and dependency checks

Every CI run performs:

- `cargo fmt --check`
- Clippy across all workspace targets with `-D warnings`
- all workspace tests using the locked dependency graph
- an explicit Rust 1.88 locked build check
- RustSec `cargo audit` against the committed `Cargo.lock`
- a clean Docker build and runtime `--help` smoke test

The advisory check is intentionally based on the committed lockfile, not an
independently re-resolved dependency graph.

## SBOM

CI installs OWASP `cargo-cyclonedx` at a pinned tool version and generates a
CycloneDX JSON SBOM from the locked Cargo workspace. The run's Git commit
timestamp is supplied as `SOURCE_DATE_EPOCH` so the generator can use its
reproducible-output mode.

SBOM files are uploaded as the `cyclonedx-sbom` GitHub Actions artifact and
retained for 30 days.

This SBOM describes the Rust/Cargo dependency graph. It is not yet a complete
container/OS-package SBOM; image-level scanning and release attestations are a
separate hardening step.

## Dependency maintenance

Dependabot checks three ecosystems weekly:

- Cargo crates
- the Docker base images
- GitHub Actions

Updates still require normal CI and review. Dependabot does not bypass the
locked build, compatibility fixtures, or protocol tests.

## Secret boundary

No production PFX, password, Ministry account, `known_hosts`, taxpayer data, or
received invoice object is included in source control, the Docker build context,
or CI artifacts.

The container's `.dockerignore` excludes common key/certificate formats,
environment files, `known_hosts`, signed CMS/PKCS#7 artifacts, and runtime
inbox/outbox/work directories.

## Remaining production gates

The controls above provide source/dependency evidence but do not prove Ministry
interoperability or operational readiness. Before production use, complete at
least:

1. authorized Ministry test-environment interoperability;
2. independent review of the clean-room protocol observations and Rust wire
   implementation;
3. container/OS vulnerability scanning and release SBOM/attestation;
4. deployment-specific secret management, egress allowlisting, backup,
   retention, monitoring, and incident procedures;
5. application-unit acceptance tests with representative non-production MIG
   messages and result/retry scenarios.
