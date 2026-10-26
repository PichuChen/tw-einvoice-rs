# Full-chain F0401 interoperability harness

Issue #35 / M10 adds public, offline coverage in
`crates/einvoice-client/tests/full_chain.rs`:

1. Construct and validate a synthetic MIG 4.1 `F0401Invoice`, including Unicode
   and XML escaping, then serialize it through the domain model.
2. Preserve that XML inside a one-message `InvoiceEnvelope` with coherent
   seller, routing, message type and version metadata.
3. Generate an ephemeral RSA-2048 identity and self-signed certificate, encode
   them as a password-protected PKCS#12/PFX, and load the real `PfxSigner`.
4. Run `prepare_submission` and `PreparedSubmission::submit`, exercising the
   strict CMS/armor/Pack path and the typed `PackArtifactSubmission` boundary.
5. Capture plain and ZIP upload bytes with in-memory adapters. Assert one upload
   precedes one notification. Encode the real PFS001 JSON body and check every
   field, then decode a synthetic accepted gateway response.
6. Decode the captured object (including ZIP extraction), verify its attached
   CMS signature using OpenSSL, and compare the recovered Envelope byte for
   byte. A same-length mutation of the embedded invoice must remain parseable
   but fail signature verification.

## Pinned boundaries

- MIG XML remains intact in the signed Envelope, including UTF-8 and escaped text.
- Envelope `InvoicePack` uses `version="v41"`; filename and PFS001 use `4.1`.
- The source basename is derived from the generated invoice number. The common
  name is created once as submission metadata and preserved through preparation,
  upload, PFS001 and receipt. Neither F0401 nor Envelope has a common-filename
  field, so the harness does not add one to the XML.
- ZIP contains exactly one Deflated entry named with the full local pack filename.
  The remote object and PFS001 use the common name without a `.zip` suffix.
- PFS001 `size` is the pre-ZIP armored CMS byte length, not the Envelope, binary
  CMS or compressed object length.
- Armor uses 64 columns, LF and a final LF without PEM markers. CMS begins with
  the strict BER indefinite-length framing.

## Run

```sh
cargo test --locked -p tw-einvoice-client --test full_chain
```

The existing `cargo test --locked --workspace --all-targets` CI step also runs
both cases; no separate service, secret or ignored/manual test is needed.

Only the upload and notification adapters are mocked. The harness never creates
an SFTP/HTTP client, contacts MOF, or reads external certificates or credentials.
The generated private key/PFX exists in process memory only. OpenSSL's `NOVERIFY`
option skips trust-chain validation for the disposable self-signed certificate;
content/signature verification stays enabled and is checked by the tamper case.

This establishes offline cross-layer consistency and cryptographic validity.
It does not establish official-client byte equivalence or MOF acceptance. The
official 5420 differential runner belongs in the private research repository,
after this public CI gate is green. Proprietary binaries and decompiled source
must remain outside this repository.
