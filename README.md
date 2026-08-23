# tw-einvoice-rs

Open-source Rust building blocks for Taiwan e-Invoice interoperability.

## Scope

This project aims to implement, from public specifications and independently observed interoperability behavior:

- MIG 4.1 data models and validation
- invoice lifecycle primitives
- durable packaging and submission state
- signing abstractions
- transport abstractions for the Ministry of Finance e-Invoice platform
- compatibility adapters for existing Turnkey-based deployments

The project does **not** redistribute proprietary Turnkey binaries or decompiled source code. Public implementation work should be reproducible from public specifications, public documentation, and clean-room interoperability test vectors.

## Status

Early research / bootstrap stage. **Do not use for production invoice transmission yet.**

## License

Apache-2.0.
