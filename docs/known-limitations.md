# Known limitations

This project is pre-release. The message-specific validation layer is currently a fast preflight validator, not a replacement for XML Schema validation.

## M0 / F0401

- Full MIG 4.1 XSD validation is not implemented yet.
- Cross-field accounting/business rules are not implemented yet.
- The current `xsd:time` preflight parser validates timezone hour bounds but does not yet reject minute values greater than 59 for offsets below `14:00` (for example, `+08:99`). Full XSD validation will reject such a value. This must be corrected before M0 is merged/released.
- Currency validation currently checks the three-uppercase-letter wire shape; the complete MIG enumeration belongs in the schema/domain layer.

These limitations are intentionally explicit so callers do not mistake preflight validation for platform acceptance.
