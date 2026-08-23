# Security policy

This project handles security-sensitive financial messaging. It is currently pre-production and has no supported release line yet.

Please do not publish vulnerabilities that expose real credentials, certificates, taxpayer data, or production invoice payloads. Use a private GitHub security advisory when available.

The project intends to maintain:

- `cargo clippy` and tests on every change
- dependency and license review
- secret scanning hygiene
- SBOM/release provenance before production releases
- explicit threat modeling for certificate handling, replay/idempotency, spool integrity, and transport authentication
