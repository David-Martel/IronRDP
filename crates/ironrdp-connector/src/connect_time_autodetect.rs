//! Connect-time network auto-detection ([MS-RDPBCGR] §1.3.8, §2.2.14).
//!
//! When the client advertises `RNS_UD_CS_SUPPORT_NET_CHAR_AUTODETECT`
//! (`ClientEarlyCapabilityFlags::SUPPORT_NET_CHAR_AUTODETECT`) in the GCC client
//! core data, FreeRDP-based servers (e.g. gnome-remote-desktop) run a
//! connect-time auto-detection exchange *before* the Licensing phase: they send
//! one or more Server Auto-Detect Request PDUs (RTT measure, bandwidth measure)
//! and expect the client to answer the RTT request.
//!
//! Unlike the continuous/in-session auto-detect PDUs (which are carried inside a
//! Share Data Header, [MS-RDPBCGR] §2.2.14 over the I/O channel after the
//! connection is active), the **connect-time** PDUs are framed with only a
//! [`BasicSecurityHeader`] whose flags carry `AUTODETECT_REQ` / `AUTODETECT_RSP`.
//! This module handles that security-header framing so the connector can consume
//! the request and reply without desynchronising the following Licensing PDU.

use ironrdp_core::{Decode as _, DecodeResult, Encode, EncodeResult, ReadCursor, WriteCursor, ensure_size};
use ironrdp_pdu::rdp::autodetect::{AutoDetectRequest, AutoDetectResponse};
use ironrdp_pdu::rdp::headers::{BASIC_SECURITY_HEADER_SIZE, BasicSecurityHeader, BasicSecurityHeaderFlags};

/// A connect-time Auto-Detect Response PDU: a [`BasicSecurityHeader`] carrying the
/// `AUTODETECT_RSP` flag followed by the [`AutoDetectResponse`] body.
///
/// This is the security-header framing used during the connect sequence (before
/// Licensing), as opposed to the Share-Data framing used in-session.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConnectTimeAutoDetectRsp {
    pub(crate) response: AutoDetectResponse,
}

impl ConnectTimeAutoDetectRsp {
    const NAME: &'static str = "ConnectTimeAutoDetectRsp";

    pub(crate) fn new(response: AutoDetectResponse) -> Self {
        Self { response }
    }

    fn security_header() -> BasicSecurityHeader {
        BasicSecurityHeader {
            flags: BasicSecurityHeaderFlags::AUTODETECT_RSP,
        }
    }
}

impl Encode for ConnectTimeAutoDetectRsp {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_size!(in: dst, size: self.size());

        Self::security_header().encode(dst)?;
        self.response.encode(dst)?;

        Ok(())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn size(&self) -> usize {
        BASIC_SECURITY_HEADER_SIZE + self.response.size()
    }
}

/// Classification of a security-header-framed I/O-channel PDU received while the
/// connector is in the connect-time auto-detection phase.
#[derive(Debug)]
pub(crate) enum ConnectTimePdu {
    /// A Server Auto-Detect Request PDU (`AUTODETECT_REQ`).
    AutoDetectRequest(AutoDetectRequest),
    /// Anything else (most importantly the first Licensing PDU): the auto-detect
    /// phase is over and this PDU belongs to the next connector state.
    Other,
}

/// Peek at the [`BasicSecurityHeader`] of a connect-time I/O-channel PDU and, if
/// it carries `AUTODETECT_REQ`, decode the [`AutoDetectRequest`] body.
///
/// `user_data` is the RDP payload of an MCS Send Data Indication (i.e. the bytes
/// after the MCS/X.224 wrapping have been stripped).
pub(crate) fn classify_connect_time_pdu(user_data: &[u8]) -> DecodeResult<ConnectTimePdu> {
    let mut cursor = ReadCursor::new(user_data);
    let header = BasicSecurityHeader::decode(&mut cursor)?;

    if header.flags.contains(BasicSecurityHeaderFlags::AUTODETECT_REQ) {
        let request = AutoDetectRequest::decode(&mut cursor)?;
        Ok(ConnectTimePdu::AutoDetectRequest(request))
    } else {
        Ok(ConnectTimePdu::Other)
    }
}

/// Build the connect-time Auto-Detect Response for a given request, if one is
/// required.
///
/// Per [MS-RDPBCGR] §1.3.8.1 the client MUST answer an RTT Measure Request with
/// an RTT Measure Response carrying the same sequence number. Bandwidth Measure
/// Start/Payload require no immediate reply; a Bandwidth Measure Stop would ask
/// for a Bandwidth Measure Results response, but producing an honest time-delta /
/// byte-count requires a measurement state machine that is out of scope here, so
/// those are acknowledged by continuing to read (returning `None`). The RTT
/// response alone is sufficient for FreeRDP servers to consider the client
/// auto-detect-capable and enable gated features such as audio redirection.
pub(crate) fn response_for_request(request: &AutoDetectRequest) -> Option<AutoDetectResponse> {
    match request {
        AutoDetectRequest::RttRequest { sequence_number, .. } => Some(AutoDetectResponse::RttResponse {
            sequence_number: *sequence_number,
        }),
        AutoDetectRequest::BandwidthMeasureStart { .. }
        | AutoDetectRequest::BandwidthMeasurePayload { .. }
        | AutoDetectRequest::BandwidthMeasureStop { .. }
        | AutoDetectRequest::NetworkCharacteristicsResult { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use ironrdp_core::{WriteBuf, encode_buf};

    use super::*;

    /// Encode a connect-time Auto-Detect *Request* the way a server would, so the
    /// classifier can be exercised against realistic bytes.
    fn encode_server_autodetect_request(request: &AutoDetectRequest) -> Vec<u8> {
        let header = BasicSecurityHeader {
            flags: BasicSecurityHeaderFlags::AUTODETECT_REQ,
        };
        let mut buf = vec![0u8; header.size() + request.size()];
        let mut cursor = WriteCursor::new(&mut buf);
        header.encode(&mut cursor).unwrap();
        request.encode(&mut cursor).unwrap();
        buf
    }

    #[test]
    fn classifies_rtt_request_and_builds_matching_response() {
        let request = AutoDetectRequest::rtt_connect_time(7);
        let bytes = encode_server_autodetect_request(&request);

        let classified = classify_connect_time_pdu(&bytes).expect("classify");
        let ConnectTimePdu::AutoDetectRequest(decoded) = classified else {
            panic!("expected auto-detect request");
        };

        let response = response_for_request(&decoded).expect("rtt requires a response");
        assert_eq!(response, AutoDetectResponse::RttResponse { sequence_number: 7 });
    }

    #[test]
    fn non_autodetect_security_header_is_other() {
        // A licensing PDU carries LICENSE_PKT, not AUTODETECT_REQ.
        let header = BasicSecurityHeader {
            flags: BasicSecurityHeaderFlags::LICENSE_PKT,
        };
        let mut bytes = vec![0u8; header.size()];
        let mut cursor = WriteCursor::new(&mut bytes);
        header.encode(&mut cursor).unwrap();
        // Trailing licensing bytes (unparsed by the classifier).
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

        let classified = classify_connect_time_pdu(&bytes).expect("classify");
        assert!(matches!(classified, ConnectTimePdu::Other));
    }

    #[test]
    fn bandwidth_requests_need_no_response() {
        let start = AutoDetectRequest::bw_start_connect_time(3);
        assert!(response_for_request(&start).is_none());
    }

    #[test]
    fn response_roundtrips_with_autodetect_rsp_flag() {
        let pdu = ConnectTimeAutoDetectRsp::new(AutoDetectResponse::RttResponse { sequence_number: 42 });
        let mut buf = WriteBuf::new();
        encode_buf(&pdu, &mut buf).expect("encode");

        // First two bytes are the BasicSecurityHeader flags (little-endian);
        // AUTODETECT_RSP = 0x2000.
        let flags = u16::from_le_bytes([buf.filled()[0], buf.filled()[1]]);
        assert_eq!(flags, BasicSecurityHeaderFlags::AUTODETECT_RSP.bits());
    }
}
