use std::{error::Error, fmt, str};

use tw_einvoice_core::MigVersion;

use crate::InvoiceEnvelope;

const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";
const ENVELOPE_NAMESPACE_V41: &str = "urn:GEINV:InvoiceEnvelope:4.1";
const ENVELOPE_SCHEMA: &str = "InvoiceEnvelope.xsd";

/// Serialization error for the strict Linux Turnkey envelope profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeSerializeError {
    /// Turnkey's upstream canonical form is UTF-8 text. A raw payload that is
    /// not UTF-8 cannot be inserted into the XML envelope without changing it.
    PayloadNotUtf8 { index: usize },
    /// An XML declaration cannot appear inside `InvoicePack`; the official
    /// UpCast stage removes it before Pack splices payload text into the
    /// envelope.
    EmbeddedXmlDeclaration { index: usize },
}

impl fmt::Display for EnvelopeSerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadNotUtf8 { index } => {
                write!(f, "MIG payload {index} is not valid UTF-8")
            }
            Self::EmbeddedXmlDeclaration { index } => write!(
                f,
                "MIG payload {index} contains an XML declaration that cannot be embedded in InvoicePack"
            ),
        }
    }
}

impl Error for EnvelopeSerializeError {}

impl InvoiceEnvelope {
    /// Serializes the envelope using the Linux Turnkey 3.2.1 Pack wire profile.
    ///
    /// This deliberately models the observable Pack behavior rather than a
    /// generic XML pretty-printer:
    ///
    /// - the document begins with the leading LF emitted by the official
    ///   `Parser.generateXml(OutputStream, ...)` path;
    /// - the envelope namespace/schema attributes and field ordering match the
    ///   MIG 4.1 `InvoiceEnvelope` XSD and Turnkey JAXB output;
    /// - `InvoicePack` attributes are ordered `count`, `messageType`, `version`;
    /// - already-canonical MIG payload text is spliced into the pack without
    ///   parse/re-serialize round-tripping;
    /// - source line endings are normalized to LF, matching Java
    ///   `BufferedReader.readLine()` + `PrintStream.println()` on Linux;
    /// - the final `</InvoiceEnvelope>` is followed by LF because Pack writes it
    ///   with `println` before signing.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeSerializeError`] when a payload is not UTF-8 or still
    /// contains an XML declaration.
    pub fn to_turnkey_xml(&self) -> Result<Vec<u8>, EnvelopeSerializeError> {
        let (namespace, version_token) = envelope_profile(self.metadata.version);
        let mut output = String::with_capacity(
            self.payloads
                .iter()
                .map(|payload| payload.as_bytes().len())
                .sum::<usize>()
                + 1024,
        );

        // The official no-start-document JAXB/StAX path emits this leading LF,
        // and Pack signs it as part of the envelope bytes.
        output.push('\n');
        output.push_str("<InvoiceEnvelope xmlns=\"");
        push_attribute_escaped(&mut output, namespace);
        output.push_str("\" xmlns:xsi=\"");
        push_attribute_escaped(&mut output, XSI_NAMESPACE);
        output.push_str("\" xsi:schemaLocation=\"");
        push_attribute_escaped(&mut output, namespace);
        output.push(' ');
        push_attribute_escaped(&mut output, ENVELOPE_SCHEMA);
        output.push_str("\">\n");

        push_party(&mut output, "From", &self.routing.from);
        push_routing(&mut output, "FromVAC", &self.routing.from_vac);
        push_party(&mut output, "To", &self.routing.to);
        push_routing(&mut output, "ToVAC", &self.routing.to_vac);

        output.push_str("  <InvoicePack count=\"");
        output.push_str(&self.payloads.len().to_string());
        output.push_str("\" messageType=\"");
        push_attribute_escaped(&mut output, &self.metadata.message_type);
        output.push_str("\" version=\"");
        push_attribute_escaped(&mut output, version_token);
        output.push_str("\">\n");

        for (index, payload) in self.payloads.iter().enumerate() {
            let text = str::from_utf8(payload.as_bytes())
                .map_err(|_| EnvelopeSerializeError::PayloadNotUtf8 { index })?;
            if text.trim_start().starts_with("<?xml") {
                return Err(EnvelopeSerializeError::EmbeddedXmlDeclaration { index });
            }
            append_like_java_read_line(&mut output, text);
        }

        output.push_str("</InvoicePack>\n</InvoiceEnvelope>\n");
        Ok(output.into_bytes())
    }
}

fn envelope_profile(version: MigVersion) -> (&'static str, &'static str) {
    match version {
        MigVersion::V4_1 => (ENVELOPE_NAMESPACE_V41, "v41"),
    }
}

fn push_party(output: &mut String, tag: &str, party: &crate::PartyInfo) {
    output.push_str("  <");
    output.push_str(tag);
    output.push_str(">\n    <PartyId>");
    push_text_escaped(output, &party.identifier);
    output.push_str("</PartyId>\n");
    if let Some(description) = &party.description {
        output.push_str("    <Description>");
        push_text_escaped(output, description);
        output.push_str("</Description>\n");
    }
    output.push_str("  </");
    output.push_str(tag);
    output.push_str(">\n");
}

fn push_routing(output: &mut String, tag: &str, routing: &crate::RoutingInfo) {
    output.push_str("  <");
    output.push_str(tag);
    output.push_str(">\n    <RoutingId>");
    push_text_escaped(output, &routing.identifier);
    output.push_str("</RoutingId>\n");
    if let Some(description) = &routing.description {
        output.push_str("    <Description>");
        push_text_escaped(output, description);
        output.push_str("</Description>\n");
    }
    output.push_str("  </");
    output.push_str(tag);
    output.push_str(">\n");
}

fn append_like_java_read_line(output: &mut String, input: &str) {
    // Java BufferedReader.readLine() recognizes CR, LF, and CRLF and discards
    // the terminator. Turnkey then PrintStream.println()s every returned line.
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.is_empty() {
        output.push('\n');
        return;
    }

    for line in normalized.split_terminator('\n') {
        output.push_str(line);
        output.push('\n');
    }
}

fn push_text_escaped(output: &mut String, input: &str) {
    for character in input.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
}

fn push_attribute_escaped(output: &mut String, input: &str) {
    for character in input.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use tw_einvoice_core::MigVersion;

    use crate::{
        EnvelopeRouting, InvoiceEnvelope, InvoicePackMetadata, MigPayload, PartyInfo, RoutingInfo,
    };

    use super::EnvelopeSerializeError;

    fn routing() -> EnvelopeRouting {
        EnvelopeRouting {
            from: PartyInfo {
                identifier: "12345678".into(),
                description: Some("Sender & Co".into()),
            },
            from_vac: RoutingInfo {
                identifier: "FROM-VAC".into(),
                description: Some("Route <A>".into()),
            },
            to: PartyInfo {
                identifier: "0000000000".into(),
                description: Some("Receiver".into()),
            },
            to_vac: RoutingInfo {
                identifier: "TO-VAC".into(),
                description: None,
            },
        }
    }

    fn metadata() -> InvoicePackMetadata {
        InvoicePackMetadata {
            message_type: "F0401".into(),
            version: MigVersion::V4_1,
        }
    }

    #[test]
    fn matches_recovered_turnkey_envelope_splice_profile() {
        let payload = MigPayload::new(
            br#"<Invoice xmlns="urn:GEINV:eInvoiceMessage:F0401:4.1"><Main/></Invoice>"#.to_vec(),
        )
        .unwrap();
        let envelope = InvoiceEnvelope::new(routing(), metadata(), vec![payload]).unwrap();
        let encoded = envelope.to_turnkey_xml().unwrap();

        let expected = concat!(
            "\n",
            "<InvoiceEnvelope xmlns=\"urn:GEINV:InvoiceEnvelope:4.1\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"urn:GEINV:InvoiceEnvelope:4.1 InvoiceEnvelope.xsd\">\n",
            "  <From>\n",
            "    <PartyId>12345678</PartyId>\n",
            "    <Description>Sender &amp; Co</Description>\n",
            "  </From>\n",
            "  <FromVAC>\n",
            "    <RoutingId>FROM-VAC</RoutingId>\n",
            "    <Description>Route &lt;A&gt;</Description>\n",
            "  </FromVAC>\n",
            "  <To>\n",
            "    <PartyId>0000000000</PartyId>\n",
            "    <Description>Receiver</Description>\n",
            "  </To>\n",
            "  <ToVAC>\n",
            "    <RoutingId>TO-VAC</RoutingId>\n",
            "  </ToVAC>\n",
            "  <InvoicePack count=\"1\" messageType=\"F0401\" version=\"v41\">\n",
            "<Invoice xmlns=\"urn:GEINV:eInvoiceMessage:F0401:4.1\"><Main/></Invoice>\n",
            "</InvoicePack>\n",
            "</InvoiceEnvelope>\n"
        );

        assert_eq!(encoded, expected.as_bytes());
    }

    #[test]
    fn normalizes_payload_crlf_like_linux_pack() {
        let payload =
            MigPayload::new(b"<Invoice>\r\n  <Main/>\r\n</Invoice>\r\n".to_vec()).unwrap();
        let envelope = InvoiceEnvelope::new(routing(), metadata(), vec![payload]).unwrap();
        let encoded = String::from_utf8(envelope.to_turnkey_xml().unwrap()).unwrap();

        assert!(encoded.contains("<Invoice>\n  <Main/>\n</Invoice>\n</InvoicePack>"));
        assert!(!encoded.contains('\r'));
    }

    #[test]
    fn rejects_embedded_xml_declaration() {
        let payload = MigPayload::new(b"<?xml version=\"1.0\"?><Invoice/>".to_vec()).unwrap();
        let envelope = InvoiceEnvelope::new(routing(), metadata(), vec![payload]).unwrap();

        assert_eq!(
            envelope.to_turnkey_xml().unwrap_err(),
            EnvelopeSerializeError::EmbeddedXmlDeclaration { index: 0 }
        );
    }

    #[test]
    fn rejects_non_utf8_payload() {
        let payload = MigPayload::new(vec![0xff, 0xfe]).unwrap();
        let envelope = InvoiceEnvelope::new(routing(), metadata(), vec![payload]).unwrap();

        assert_eq!(
            envelope.to_turnkey_xml().unwrap_err(),
            EnvelopeSerializeError::PayloadNotUtf8 { index: 0 }
        );
    }
}
