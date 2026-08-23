# Protocol boundaries

`tw-einvoice-rs` deliberately separates public MIG semantics from transport interoperability so each layer can be tested and audited independently.

```text
MIG message model
    -> deterministic XML bytes
    -> InvoiceEnvelope
    -> exact envelope bytes
    -> CMS signer
    -> optional one-entry ZIP
    -> SFTP object
    -> gateway enqueue notification
    -> ProcessResult reconciliation
```

## Byte ownership

Every transition that changes byte representation is explicit:

1. A MIG message owns its deterministic XML serialization.
2. The envelope layer embeds the already-serialized message and owns the exact envelope bytes.
3. The signer accepts bytes and returns an opaque signed package. It must not parse or reserialize XML.
4. Compression, when enabled, wraps the immutable signed package without changing its logical submission identity.
5. The transport layer submits immutable package bytes and metadata; it does not modify invoice content.

This makes hashes meaningful at every durable boundary and prevents accidental XML reserialization after signing.

## State ownership

Local stage success and platform acceptance are different concepts. The gateway should represent them separately.

Suggested high-level submission states:

```text
Created
Validated
Packaged
Signed
SftpUploaded
ApiNotifyPending
ApiOutcomeUnknown
PlatformQueued
Accepted
Rejected
```

An HTTP timeout after SFTP upload must never be represented as `Rejected`: the remote outcome is unknown until retry/reconciliation or a later ProcessResult resolves it.

## Clean-room rule

Public code and documentation describe protocol/domain behavior and independently implemented algorithms. Proprietary Turnkey binaries, source reconstructions, credentials and production material do not belong in this repository.

Interoperability evidence may be produced using disposable synthetic inputs, then normalized into tests and protocol requirements before implementation here.
