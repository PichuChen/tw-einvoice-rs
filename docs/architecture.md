# Architecture notes

The Ministry of Finance training material describes Turnkey as a pipeline around a file/DB boundary and the platform's SFTP + Web API + queue processing path.

The publicly documented upload path is conceptually:

1. ERP emits a MIG document.
2. UpCast validates MIG data and groups records, assigning a UUID.
3. Pack adds envelope/routing data and signs the package.
4. SendFile uploads to SFTP and calls the platform Web API.
5. The platform queues and processes the job.
6. Turnkey retrieves `ProcessResult` and reconciles state by UUID.

The download side is described as ReceiveFile -> Unpack -> DownCast.

This repository therefore separates concerns into domain/MIG, package/signing, durable lifecycle state, and transport adapters rather than treating Turnkey as a monolith.

## Planned adapters

- `official-turnkey-spool`: write/read the documented Turnkey filesystem boundary while existing installations remain in service.
- `mof-native`: direct platform transport, only after the wire protocol and authorization requirements are sufficiently specified and interoperability-tested.
- third-party provider adapters may be added independently of the core MIG model.

## Primary public references

- Ministry of Finance e-Invoice platform: https://www.einvoice.nat.gov.tw/
- New Turnkey training material: https://www.einvoice.nat.gov.tw/static/ptl/ein_upload/attachments/1680146243169_0.pdf

Public documentation is treated as specification input, not copied wholesale into this repository.
