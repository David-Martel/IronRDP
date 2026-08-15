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
use ironrdp_pdu::rdp::autodetect::{AutoDetectRequest, AutoDetectResponse, BW_RESULTS_CONNECT_TIME};
use ironrdp_pdu::rdp::headers::{BASIC_SECURITY_HEADER_SIZE, BasicSecurityHeader, BasicSecurityHeaderFlags};

/// A connect-time Auto-Detect Response PDU: a [`BasicSecurityHeader`] carrying the
/// `AUTODETECT_RSP` flag followed by the [`AutoDetectResponse`] body.
///
/// This is the security-header framing used during the connect sequence (before
/// Licensing), as opposed to the Share-Data framing used in-session.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectTimeAutoDetectRsp {
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
/// Per [MS-RDPBCGR] §1.3.8:
/// - an RTT Measure Request is answered with an RTT Measure Response carrying the
///   same sequence number (§2.2.14.2.1);
/// - a Bandwidth Measure **Stop** is answered with a Bandwidth Measure Results
///   response (§2.2.14.2.2) — this is mandatory: gnome-remote-desktop performs a
///   *bandwidth*-based connect-time detection (Start → Stop) and blocks the
///   handshake (never advancing to Licensing) until it receives the Results, so
///   omitting it deadlocks the connection;
/// - Bandwidth Measure Start / Payload and Network Characteristics Result require
///   no immediate reply.
///
/// The Results `byte_count` is taken from the connect-time Stop payload (the data
/// whose transfer is being measured) and a nominal 1 ms `time_delta` is reported.
/// An exact measurement would require timing across the whole Start→Stop window,
/// but the value only informs the server's codec sizing; what matters for gating
/// (e.g. audio redirection) is that the client answered, marking itself
/// auto-detect-capable.
/// Build the response for an in-session (continuous) auto-detect request that
/// arrived framed with a [`BasicSecurityHeader`] on the MCS message channel.
///
/// gnome-remote-desktop / FreeRDP keep running network auto-detection *after* the
/// connection is active, sending Server Auto-Detect Request PDUs on the MCS
/// message channel (which the IronRDP client does not join, so they surface in
/// the session layer on an unrecognized channel — commonly decoded as channel 0).
/// Unlike the in-session auto-detect PDUs carried inside a Share Data Header
/// ([MS-RDPBCGR] §2.2.14 over the I/O channel), these use the *same*
/// security-header framing as the connect-time exchange, so the same classifier
/// and responder apply.
///
/// Returns `Ok(Some(rsp))` with an encodable response for requests that require a
/// reply (RTT Measure, Bandwidth Measure Stop), `Ok(None)` for requests that need
/// no reply or payloads that are not auto-detect requests, and `Err` only if the
/// leading [`BasicSecurityHeader`] / request body cannot be decoded.
pub fn in_session_autodetect_response(user_data: &[u8]) -> DecodeResult<Option<ConnectTimeAutoDetectRsp>> {
    match classify_connect_time_pdu(user_data)? {
        ConnectTimePdu::AutoDetectRequest(request) => {
            Ok(response_for_request(&request).map(ConnectTimeAutoDetectRsp::new))
        }
        ConnectTimePdu::Other => Ok(None),
    }
}

pub(crate) fn response_for_request(request: &AutoDetectRequest) -> Option<AutoDetectResponse> {
    match request {
        AutoDetectRequest::RttRequest { sequence_number, .. } => Some(AutoDetectResponse::RttResponse {
            sequence_number: *sequence_number,
        }),
        AutoDetectRequest::BandwidthMeasureStop {
            sequence_number,
            payload,
            ..
        } => {
            let byte_count = payload
                .as_ref()
                .map_or(0, |p| u32::try_from(p.len()).unwrap_or(u32::MAX));
            Some(AutoDetectResponse::BandwidthMeasureResults {
                sequence_number: *sequence_number,
                response_type: BW_RESULTS_CONNECT_TIME,
                time_delta_ms: 1,
                byte_count,
            })
        }
        AutoDetectRequest::BandwidthMeasureStart { .. }
        | AutoDetectRequest::BandwidthMeasurePayload { .. }
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
    fn bandwidth_start_needs_no_response() {
        let start = AutoDetectRequest::bw_start_connect_time(3);
        assert!(response_for_request(&start).is_none());
    }

    #[test]
    fn bandwidth_stop_produces_results_with_payload_byte_count() {
        let stop = AutoDetectRequest::BandwidthMeasureStop {
            sequence_number: 9,
            request_type: ironrdp_pdu::rdp::autodetect::BW_STOP_CONNECT_TIME,
            payload: Some(vec![0u8; 1600]),
        };
        let response = response_for_request(&stop).expect("connect-time BW stop must be answered");
        assert_eq!(
            response,
            AutoDetectResponse::BandwidthMeasureResults {
                sequence_number: 9,
                response_type: BW_RESULTS_CONNECT_TIME,
                time_delta_ms: 1,
                byte_count: 1600,
            }
        );
    }

    #[test]
    fn in_session_rtt_request_is_answered() {
        // Same security-header framing as connect-time, but exercised through the
        // public in-session entry point used by the session layer for
        // message-channel auto-detect requests.
        let request = AutoDetectRequest::rtt_connect_time(11);
        let bytes = encode_server_autodetect_request(&request);

        let rsp = in_session_autodetect_response(&bytes)
            .expect("classify")
            .expect("rtt requires a response");
        assert_eq!(rsp.response, AutoDetectResponse::RttResponse { sequence_number: 11 });
    }

    #[test]
    fn in_session_non_autodetect_is_none() {
        // A licensing PDU (LICENSE_PKT) must not be treated as an auto-detect
        // request, so the session layer falls through to its tolerate/error path.
        let header = BasicSecurityHeader {
            flags: BasicSecurityHeaderFlags::LICENSE_PKT,
        };
        let mut bytes = vec![0u8; header.size()];
        let mut cursor = WriteCursor::new(&mut bytes);
        header.encode(&mut cursor).unwrap();
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

        assert!(in_session_autodetect_response(&bytes).expect("classify").is_none());
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
