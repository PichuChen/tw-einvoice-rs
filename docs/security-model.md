# Security model

The project is intended for security-sensitive financial infrastructure. Source availability is useful only when the build, key boundaries and operational state are also auditable.

## Trust boundaries

### Invoice input

ERP/POS input is untrusted until MIG schema and semantic validation complete. Parsing and validation code must not perform network access or load external XML entities.

### Signing key

Private signing keys are never part of the invoice domain model. A signer receives immutable envelope bytes and exposes only a signing operation. Software-PFX, PKCS#11/HSM and future signer backends should implement the same boundary.

### Transport credentials

SFTP/Web API credentials are transport secrets and must not be serialized into invoice payloads, logs, metrics, panic messages or durable test fixtures.

### Remote outcome

Network errors do not imply invoice rejection. Durable state must distinguish a definite remote response from an unknown outcome.

## Supply-chain baseline

Release automation should eventually enforce:

- `cargo fmt`, `cargo clippy`, unit/integration tests;
- dependency/advisory review (`cargo audit` or equivalent);
- license/source policy (`cargo deny` or equivalent);
- SBOM generation (CycloneDX and/or SPDX);
- container vulnerability scanning for distributed images;
- provenance/signing for release artifacts;
- pinned/reviewed CI actions;
- no bundled Ministry binaries or production secrets.

## Cryptographic testing

Never commit a real fiscal signing private key. Automated interoperability fixtures must generate or use clearly synthetic test CAs/certificates whose keys have no production value.

Cryptographic tests should validate structure and semantics rather than require byte equality where an algorithm is intentionally randomized (for example ECDSA signatures).

## XML

XML implementations must disable external entity expansion and network schema resolution by default. MIG schemas used for validation should be vendored from sources whose redistribution status is known, or fetched/verified in a controlled build step when redistribution is not appropriate.

## Logging and privacy

Logs should default to message UUID, stage, status and stable error code rather than full invoice payloads. Business identifiers, invoice numbers and buyer/carrier data may be sensitive operational data and should be treated accordingly.
