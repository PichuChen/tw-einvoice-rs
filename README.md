# tw-einvoice-rs

Open-source Rust building blocks for Taiwan e-Invoice interoperability.

## Scope

This project aims to implement, from public specifications and independently observed interoperability behavior:

- MIG 4.1 data models and validation
- invoice lifecycle primitives
- invoice-envelope batching and routing
- CMS signing abstractions
- Ministry of Finance gateway/SFTP transport abstractions
- compatibility adapters for existing Turnkey-based deployments

The project does **not** redistribute proprietary Turnkey binaries or decompiled source code. Public implementation work should be reproducible from public specifications, public documentation, and normalized interoperability test vectors.

## Current compatibility model

The workspace currently contains:

- `tw-einvoice-core`: common MIG/domain primitives and submission state
- `tw-einvoice-mig`: MIG document metadata and validation boundary
- `tw-einvoice-envelope`: routing and 1..1000 message batching model
- `tw-einvoice-signing`: attached CMS/SignedData profile and Turnkey-compatible Base64 armor
- `tw-einvoice-transport`: Turnkey filename grammar and PFS001 gateway request/response model
- `einvoice-cli`: future operator/developer CLI

Recovered transport details are treated as compatibility behavior, not as an API design to copy blindly. In particular, quirks such as extensionless ZIP uploads and pre-compression `size` reporting will be isolated behind explicit compatibility policies as the native transport is implemented.

## Status

Early research / bootstrap stage. **Do not use for production invoice transmission yet.**

The first target is a synthetic end-to-end path:

```text
MIG 4.1 -> InvoiceEnvelope -> CMS signer -> SFTP /in -> PFS001 -> /out reconciliation
```

No production credentials, certificates, or taxpayer data belong in this repository.

## License

Apache-2.0.
