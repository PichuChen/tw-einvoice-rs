# Container deployment

The container image is a Linux operator/runtime packaging of the native Rust
pipeline. It does not contain Ministry credentials, software certificates,
`known_hosts`, MIG payloads, signed artifacts, or received `/out` objects.

> Status: pre-production. Complete Ministry test-environment interoperability
> before enabling production transmission.

## Build

```sh
docker build -t tw-einvoice-rs:local .
docker run --rm tw-einvoice-rs:local --help
```

The final stage runs as numeric UID/GID `65532:65532` and has no shell entrypoint
or privileged requirement.

## Filesystem boundary

Use read-only mounts for identity/trust material and a writable working volume
for generated artifacts and the durable receive inbox.

Example host layout:

```text
secrets/
  identity.pfx
  known_hosts
work/
  mig/
  signed/
  inbox/
```

The OpenSSH `known_hosts` file must contain the Ministry SFTP host key. Because
the service uses port 2222, OpenSSH commonly records the host as
`[hostname]:2222`. The Rust client deliberately has no host-key bypass mode.

## Secret injection

Passwords are never accepted as CLI arguments. Set them in the invoking
environment and pass only the variable name/value through the orchestrator's
secret mechanism.

For a local Docker invocation, `--env NAME` copies an already-exported host
variable without placing the secret value in the command line:

```sh
export TW_EINVOICE_PFX_PASSWORD='...'
export TW_EINVOICE_TRANSPORT_PASSWORD='...'
```

Do not bake either variable into the image or a checked-in environment file.

## Prepare without network I/O

```sh
docker run --rm \
  --env TW_EINVOICE_PFX_PASSWORD \
  --mount type=bind,src="$PWD/secrets",dst=/run/einvoice/secrets,readonly \
  --mount type=bind,src="$PWD/work",dst=/work \
  tw-einvoice-rs:local prepare \
  --filename 'source.xml_12345678_PLATFORM_4.1-F0401-20261022-210000000-550e8400-e29b-41d4-a716-446655440000_1' \
  --mig /work/mig/invoice.xml \
  --from-vac FROM-ROUTE \
  --to-vac TO-ROUTE \
  --action C0401 \
  --service store \
  --pfx /run/einvoice/secrets/identity.pfx \
  --output /work/signed/message.cms
```

`prepare` performs validation, envelope construction and strict CMS signing,
then persists the exact Base64-armored artifact. It performs no SFTP or HTTP
operation.

## Submit to the Ministry test environment

```sh
docker run --rm \
  --env TW_EINVOICE_PFX_PASSWORD \
  --env TW_EINVOICE_TRANSPORT_PASSWORD \
  --mount type=bind,src="$PWD/secrets",dst=/run/einvoice/secrets,readonly \
  --mount type=bind,src="$PWD/work",dst=/work \
  tw-einvoice-rs:local submit \
  --environment test \
  --known-hosts /run/einvoice/secrets/known_hosts \
  --transport-id YOUR_TRANSPORT_ID \
  --filename 'source.xml_12345678_PLATFORM_4.1-F0401-20261022-210000000-550e8400-e29b-41d4-a716-446655440000_1' \
  --mig /work/mig/invoice.xml \
  --from-vac FROM-ROUTE \
  --to-vac TO-ROUTE \
  --action C0401 \
  --service store \
  --pfx /run/einvoice/secrets/identity.pfx \
  --output /work/signed/message.cms
```

The command persists and fsyncs the signed artifact before any SFTP or PFS001
network operation. Use `--environment production` only after test-environment
interoperability has been completed and operational approval is in place.

## Receive and reconcile `/out`

```sh
docker run --rm \
  --env TW_EINVOICE_TRANSPORT_PASSWORD \
  --mount type=bind,src="$PWD/secrets",dst=/run/einvoice/secrets,readonly \
  --mount type=bind,src="$PWD/work",dst=/work \
  tw-einvoice-rs:local receive \
  --environment test \
  --known-hosts /run/einvoice/secrets/known_hosts \
  --transport-id YOUR_TRANSPORT_ID \
  --inbox-dir /work/inbox
```

Each remote object is downloaded into a same-directory temporary file, flushed
and fsynced, length-checked, atomically published without overwrite, and only
then deleted from SFTP. ProcessResult/SummaryResult parsing occurs after that
durability boundary.

## Network boundary

The Ministry platform requires an approved/fixed source address for Turnkey
traffic. In Docker/Kubernetes, give the workload a deterministic egress path
(NAT gateway, egress gateway, or equivalent) and register that public address
with the platform. Containerization does not remove that requirement.
