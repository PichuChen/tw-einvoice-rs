# tw-einvoice-rs

Open-source Rust implementation of Taiwan e-Invoice MIG, signing, transport,
and Turnkey-compatible workflows.

## Scope

This project implements, from public specifications and independently observed
interoperability behavior:

- MIG 4.1 domain/validation primitives
- invoice-envelope batching and routing
- strict Turnkey-compatible CMS/SignedData signing
- Turnkey filename and packaging behavior
- Ministry SFTP `/in` upload and `/out` durable receive
- PFS001 upload notification
- ProcessResult/SummaryResult reconciliation
- validated end-to-end submission orchestration
- operator CLI and Linux container packaging

The project does **not** redistribute proprietary Turnkey binaries or decompiled
source code. Public implementation work should remain reproducible from public
specifications, public documentation, normalized interoperability observations,
and synthetic test vectors.

## Workspace

- `tw-einvoice-core`: common MIG/domain primitives and submission state
- `tw-einvoice-mig`: MIG document metadata and validation boundary
- `tw-einvoice-envelope`: Turnkey-compatible envelope serialization/batching
- `tw-einvoice-signing`: strict attached CMS signer and PFX backend
- `tw-einvoice-pack`: envelope -> CMS -> armored signed artifact
- `tw-einvoice-results`: ProcessResult/SummaryResult parsing and validation
- `tw-einvoice-state`: durable transport state, terminal disposition, and optimistic persistence boundary
- `tw-einvoice-transport`: filename, ZIP, SFTP, PFS001 and reconciliation
- `tw-einvoice-client`: cross-layer validated prepare/submit facade
- `tw-einvoice-cli`: `prepare`, `submit`, and `receive` operator commands

## Implemented path

```text
canonical MIG 4.1
    -> InvoiceEnvelope
    -> strict CMS/PFX signer
    -> Base64 signed artifact
    -> optional Turnkey-compatible ZIP
    -> verified SFTP /in
    -> PFS001
    -> verified SFTP /out
    -> durable local receive
    -> ProcessResult / SummaryResult
```

SFTP host keys are verified through OpenSSH `known_hosts`; the native
implementation deliberately does not reproduce the official client's observed
`StrictHostKeyChecking=no` behavior.

## Container

A non-root multi-stage Linux image is included. See
[`docs/container.md`](docs/container.md) for build, mounts, secret handling,
test/production selection, and durable inbox guidance. Supply-chain controls,
locked builds, RustSec scanning, and SBOM artifacts are documented in
[`docs/supply-chain.md`](docs/supply-chain.md). Durable reconciliation state and
the future PostgreSQL persistence boundary are documented in
[`docs/state-persistence.md`](docs/state-persistence.md).

```sh
docker build -t tw-einvoice-rs:local .
docker run --rm tw-einvoice-rs:local --help
```

## Status

**Pre-production. Do not use for production invoice transmission yet.**

Synthetic and local interoperability tests cover the native pipeline, but live
Ministry **test-environment** interoperability still needs to be completed with
authorized test credentials, an approved source IP, a test PFX identity, and
the Ministry SFTP host key before production enablement.

No production credentials, certificates, taxpayer data, or proprietary Turnkey
artifacts belong in this repository.

## License

Apache-2.0.


## MIG 4.1

The specification-driven F0401 model, validation layers, and deterministic XML
fixture are documented in [`docs/mig41-f0401.md`](docs/mig41-f0401.md).
