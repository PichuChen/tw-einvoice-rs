# M12: MOF TEST-only F0401 acceptance runbook

Tracking: Issue #40. Depends on M11 #39 (3.2.2 offline comparison). This document is a preparation procedure, not evidence of live acceptance.

## Non-negotiable boundary

- **TEST ONLY.** SFTP: tsftp.einvoice.nat.gov.tw:2222 (in/out). Gateway: https://tgw.einvoice.nat.gov.tw/gateway/api with PFS001 /pfs001i/uploadInvoiceMessage.
- Never use production endpoints, production PFX, customer invoices, or credentials in this phase. Never run a network acceptance test on GitHub-hosted CI.
- The organization has an allowlisted test egress IP, but the machine actually running the test **must** use that same NAT/public egress IP. A CI or assistant host is not presumed allowlisted.
- Authenticate the test SFTP host key against a trusted independent source and use a pinned dedicated known_hosts. TLS verification is mandatory; do not permit TOFU or accept-all.
- Use a test-registered business, authorized test invoice number/format, registered test signing PFX and separate transport credentials. M10 dummy BANs and disposable self-signed PFX cannot be submitted to MOF.
- A PFS001 statusCode 0 means **queued**, not invoice accepted. The result must be reconciled from signed out-folder results.

## Stage 0: operator readiness (no MOF traffic)

1. Confirm official test registration and certificate authorization, authorized invoice number/range, test transport identity, and test on-boarding/self-inspection requirements.
2. On the approved host, verify source egress IP against the approved TEST allowlist using the operator's trusted network evidence (not a GitHub runner).
3. Provision test-only secrets from a secret manager and a verified test known_hosts file; do not paste values into GitHub, shell history or logs.
4. Confirm NTP, independent storage locations for test artifacts and durable inbox, and a single-use F0401 UUID/common name.
5. Explicitly approve one valid test invoice and preserve its correlation key.

## Stage 1: local preparation (offline)

The public CLI accepts prepare, submit, and receive. Both submit and receive require an explicit --environment flag and have separate test/production endpoint mappings.

Example **offline preparation only** (all shell variables refer to operator-supplied authorized TEST values):

~~~sh
cargo run --locked -p tw-einvoice-cli -- prepare \
  --filename "$TEST_LOCAL_PACK_FILENAME" \
  --mig "$TEST_F0401_XML" \
  --from-vac "$TEST_FROM_VAC" --to-vac "$TEST_TO_VAC" \
  --action C0401 --service store --zip plain \
  --pfx "$TEST_SIGNER_PFX" \
  --api-version 3.1.5 --turnkey-version 3.2.2 \
  --output "$TEST_ARTIFACT_PATH"
~~~

Verify MIG, exact Envelope bytes, Base64 wrapping, CMS/OpenSSL signature, filename, amount and PFS001 metadata before any network action. Treat the reported 3.1.5/3.2.2 fields as provisional until M11 and test-platform compatibility are confirmed.

## Stage 2: single TEST submission (only after manual approval)

Run the same authorized configuration via the submit subcommand from the allowlisted host, with these mandatory test-only options:

~~~text
--environment test
--known-hosts /secure/test-known_hosts
--transport-id <authorized-test-transport-id>
~~~

The CLI reads PFX and transport passwords from TW_EINVOICE_PFX_PASSWORD and TW_EINVOICE_TRANSPORT_PASSWORD (or explicitly selected environment-variable names). Never put literal passwords in CLI arguments.

Current submit behavior **prepares and persists a new artifact before SFTP**, rather than loading an earlier prepare output. Before submit, ensure the newly prepared file preserves the approved immutable UUID/common name; do not overwrite earlier evidence or generate a different invoice unknowingly.

Record SFTP object name/size/hash and PFS001 response separately. Do not call a queued package an accepted invoice.

## Stage 3: receive and durable reconciliation

From the same approved host, run receive with --environment test, the verified test known_hosts, transport identity and durable inbox directory. Verify downloaded signed ProcessResult/SummaryResult, correlation UUID, routing tuple, counts, and terminal invoice state. Keep PFS enqueue status separate from terminal acceptance or rejection.

## Stage 4: failure and replay tests (after one successful baseline)

Under operator-controlled conditions, test one at a time: API timeout after SFTP upload, duplicate same-object notification, interrupted receive, replayed out-file, restart windows and deliberate test rejection. Preserve immutable UUID/hash on retry. Unknown API outcome must not become a silent rejection or a new invoice.

## Completion criteria

Archive redacted evidence: public egress confirmation, verified host key fingerprint, CMS and uploaded-file SHA-256, common name/UUID, official/software versions, SFTP/PFS001 acknowledgement, signed out results and local durable state transitions. No secrets or proprietary binaries in evidence.

This stage remains **open** until authorized TEST network acceptance and recovery checks are actually observed. It does not imply authorization to use the production environment.

Official MOF onboarding guidance: https://www.einvoice.nat.gov.tw/static/ptl/ein_upload/html/1692842481285.html
