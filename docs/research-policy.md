# Clean-room research policy

`tw-einvoice-rs` is the public implementation repository. Protocol research that may involve proprietary artifacts is isolated in a separate private workspace.

## Allowed public inputs

- publicly published MIG specifications and schemas, subject to their distribution terms
- Ministry of Finance public manuals and training material
- independently generated test data
- independently recorded black-box behavior and interoperability observations
- protocol descriptions reduced to facts necessary for interoperability

## Do not copy into this repository

- proprietary Turnkey binaries or JARs
- decompiled proprietary source
- vendor credentials, private keys, production certificates, or taxpayer/invoice data
- confidential logs or packet captures containing identifying data

## Handoff rule

A finding from private research should enter the public project as one of:

1. a protocol fact with provenance and no copied implementation expression;
2. a sanitized, independently authored test vector;
3. a behavioral test that can be reproduced against a legitimately operated Turnkey instance.

Implementation should then be written from that normalized description/test, not by transliterating proprietary code.
