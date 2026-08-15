use ironrdp_connector::connection_activation::ConnectionActivationSequence;
use ironrdp_connector::legacy::SendDataIndicationCtx;
use ironrdp_core::{Encode, WriteBuf};
use ironrdp_dvc::{DrdynvcClient, DvcProcessor, DynamicVirtualChannel};
use ironrdp_pdu::mcs::{DisconnectProviderUltimatum, DisconnectReason, McsMessage};
use ironrdp_pdu::rdp::autodetect::AutoDetectResponse;
use ironrdp_pdu::rdp::headers::ShareDataPdu;
use ironrdp_pdu::rdp::multitransport::MultitransportRequestPdu;
use ironrdp_pdu::rdp::server_error_info::{ErrorInfo, ProtocolIndependentCode, ServerSetErrorInfoPdu};
use ironrdp_pdu::rdp::session_info::{InfoData, ServerAutoReconnect};
use ironrdp_pdu::x224::X224;
use ironrdp_svc::{StaticChannelSet, SvcMessage, SvcProcessor, SvcProcessorMessages, client_encode_svc_messages};
use tracing::{debug, warn};

use crate::{SessionError, SessionErrorExt as _, SessionResult, reason_err};

/// X224 Processor output
#[derive(Debug, Clone)]
pub enum ProcessorOutput {
    /// A buffer with encoded data to send to the server.
    ResponseFrame(Vec<u8>),
    /// A graceful disconnect notification. Client should close the connection upon receiving this.
    Disconnect(DisconnectDescription),
    /// Received a [`ironrdp_pdu::rdp::headers::ServerDeactivateAll`] PDU. Client should execute the
    /// [Deactivation-Reactivation Sequence].
    ///
    /// [Deactivation-Reactivation Sequence]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/dfc234ce-481a-4674-9a5d-2a7bafb14432
    DeactivateAll(Box<ConnectionActivationSequence>),
    /// Server Initiate Multitransport Request. The application should establish a
    /// sideband UDP transport using the request ID and security cookie, then send
    /// a [`MultitransportResponsePdu`] back on the IO channel.
    ///
    /// See [\[MS-RDPBCGR\] 2.2.15.1].
    ///
    /// [\[MS-RDPBCGR\] 2.2.15.1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/de783158-8b01-4818-8fb0-62523a5b3490
    /// [`MultitransportResponsePdu`]: ironrdp_pdu::rdp::multitransport::MultitransportResponsePdu
    MultitransportRequest(MultitransportRequestPdu),
    /// Received a Server Redirection PDU. The client should tear down the current
    /// connection and reconnect to the target session, sending the load-balance
    /// routing token in the reconnect's X.224 Connection Request.
    ///
    /// See [\[MS-RDPBCGR\] 2.2.13.1.1].
    ///
    /// [\[MS-RDPBCGR\] 2.2.13.1.1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/1cf18d97-9c1e-4a83-95a2-df3a04c30850
    Redirect(Box<ironrdp_pdu::rdp::headers::ServerRedirectionPdu>),
}

#[derive(Debug, Clone)]
pub enum DisconnectDescription {
    /// Includes the reason from the MCS Disconnect Provider Ultimatum.
    /// This is the least-specific disconnect reason and is only used
    /// when a more specific disconnect code is not available.
    McsDisconnect(DisconnectReason),

    /// Includes the error information sent by the RDP server when there
    /// is a connection or disconnection failure.
    ErrorInfo(ErrorInfo),
}

pub struct Processor {
    static_channels: StaticChannelSet,
    user_channel_id: u16,
    io_channel_id: u16,
    share_id: u32,
    connection_activation: ConnectionActivationSequence,
    /// Most recent server-issued auto-reconnect cookie captured from a
    /// Save Session Info PDU (Logon Info Extended → `ARC_SC_PRIVATE_PACKET`).
    /// Used to build the client auto-reconnect cookie on a reconnect attempt.
    reconnect_cookie: Option<ServerAutoReconnect>,
}

impl Processor {
    pub fn new(
        static_channels: StaticChannelSet,
        user_channel_id: u16,
        io_channel_id: u16,
        share_id: u32,
        connection_activation: ConnectionActivationSequence,
    ) -> Self {
        Self {
            static_channels,
            user_channel_id,
            io_channel_id,
            share_id,
            connection_activation,
            reconnect_cookie: None,
        }
    }

    pub fn set_share_id(&mut self, share_id: u32) {
        self.share_id = share_id;
    }

    /// Returns the most recent server auto-reconnect cookie captured from a
    /// Save Session Info PDU, if any.
    pub fn reconnect_cookie(&self) -> Option<&ServerAutoReconnect> {
        self.reconnect_cookie.as_ref()
    }

    pub fn get_svc_processor<T: SvcProcessor + 'static>(&self) -> Option<&T> {
        self.static_channels
            .get_by_type::<T>()
            .and_then(|svc| svc.channel_processor_downcast_ref())
    }

    pub fn get_svc_processor_mut<T: SvcProcessor + 'static>(&mut self) -> Option<&mut T> {
        self.static_channels
            .get_by_type_mut::<T>()
            .and_then(|svc| svc.channel_processor_downcast_mut())
    }

    /// Completes user's SVC request with data, required to sent it over the network and returns
    /// a buffer with encoded data.
    pub fn process_svc_processor_messages<C: SvcProcessor + 'static>(
        &self,
        messages: SvcProcessorMessages<C>,
    ) -> SessionResult<Vec<u8>> {
        let channel_id = self
            .static_channels
            .get_channel_id_by_type::<C>()
            .ok_or_else(|| reason_err!("SVC", "channel not found"))?;

        process_svc_messages(messages.into(), channel_id, self.user_channel_id)
    }

    pub fn get_dvc<T: DvcProcessor + 'static>(&self) -> Option<&DynamicVirtualChannel> {
        self.get_svc_processor::<DrdynvcClient>()?.get_dvc_by_type_id::<T>()
    }

    pub fn get_dvc_by_channel_id(&self, channel_id: u32) -> Option<&DynamicVirtualChannel> {
        self.get_svc_processor::<DrdynvcClient>()?
            .get_dvc_by_channel_id(channel_id)
    }

    /// Processes a received PDU. Returns a vector of [`ProcessorOutput`] that must be processed
    /// in the returned order.
    pub fn process(&mut self, frame: &[u8]) -> SessionResult<Vec<ProcessorOutput>> {
        let data_ctx: SendDataIndicationCtx<'_> =
            ironrdp_connector::legacy::decode_send_data_indication(frame).map_err(crate::legacy::map_error)?;
        let channel_id = data_ctx.channel_id;
        tracing::debug!(
            channel_id,
            io_channel_id = self.io_channel_id,
            user_data_len = data_ctx.user_data.len(),
            head = ?&data_ctx.user_data[..core::cmp::min(24, data_ctx.user_data.len())],
            "x224 process: routing SendDataIndication"
        );

        if channel_id == self.io_channel_id {
            self.process_io_channel(data_ctx)
        } else if let Some(svc) = self.static_channels.get_by_channel_id_mut(channel_id) {
            let response_pdus = svc.process(data_ctx.user_data).map_err(SessionError::pdu)?;
            process_svc_messages(response_pdus, channel_id, data_ctx.initiator_id)
                .map(|data| vec![ProcessorOutput::ResponseFrame(data)])
        } else {
            self.process_unrouted_channel(channel_id, data_ctx.user_data)
        }
    }

    /// Handle a Send Data Indication that targets neither the I/O channel nor a
    /// registered static virtual channel.
    ///
    /// gnome-remote-desktop / FreeRDP run *continuous* network auto-detection
    /// after the connection is active (when the client advertised
    /// `RNS_UD_CS_SUPPORT_NET_CHAR_AUTODETECT`, i.e. `--network-autodetect`).
    /// Those Server Auto-Detect Request PDUs are sent on the MCS message channel,
    /// which the IronRDP client never joins, so they surface here on an
    /// unrecognized channel (observed as channel `0`) framed with a
    /// [`BasicSecurityHeader`] carrying `AUTODETECT_REQ` — *not* as a Share Data
    /// PDU. Answer them with a security-header-framed Auto-Detect Response, reusing
    /// the connector's connect-time responder so the framing stays identical.
    ///
    /// Any other traffic on the message channel (`0`) is tolerated (logged and
    /// dropped) rather than fatally aborting an otherwise-healthy session; a
    /// genuinely unexpected *non-zero* channel keeps the hard error so real
    /// routing bugs still surface.
    ///
    /// [`BasicSecurityHeader`]: ironrdp_pdu::rdp::headers::BasicSecurityHeader
    fn process_unrouted_channel(&self, channel_id: u16, user_data: &[u8]) -> SessionResult<Vec<ProcessorOutput>> {
        match ironrdp_connector::connect_time_autodetect::in_session_autodetect_response(user_data) {
            Ok(Some(rsp)) => {
                debug!(channel_id, "Answering in-session (message-channel) Auto-Detect Request");
                let mut buf = WriteBuf::new();
                self.encode_io_channel(&mut buf, &rsp)?;
                Ok(vec![ProcessorOutput::ResponseFrame(buf.filled().to_vec())])
            }
            Ok(None) if channel_id == 0 => {
                warn!(channel_id, "Ignoring non-auto-detect PDU on MCS message channel");
                Ok(Vec::new())
            }
            Err(_) if channel_id == 0 => {
                warn!(channel_id, "Ignoring undecodable PDU on MCS message channel");
                Ok(Vec::new())
            }
            Ok(None) | Err(_) => Err(reason_err!("X224", "unexpected channel received: ID {channel_id}")),
        }
    }

    fn process_io_channel(&mut self, data_ctx: SendDataIndicationCtx<'_>) -> SessionResult<Vec<ProcessorOutput>> {
        debug_assert_eq!(data_ctx.channel_id, self.io_channel_id);

        let io_channel = ironrdp_connector::legacy::decode_io_channel(data_ctx).map_err(crate::legacy::map_error)?;

        match io_channel {
            ironrdp_connector::legacy::IoChannelPdu::Data(ctx) => {
                match ctx.pdu {
                    ShareDataPdu::SaveSessionInfo(session_info) => {
                        debug!("Got Session Save Info PDU: {session_info:?}");
                        // Capture the auto-reconnect cookie (ARC_SC_PRIVATE_PACKET) if the
                        // server sent Logon Info Extended with one. It is later used to derive
                        // the client auto-reconnect cookie on a reconnect attempt
                        // ([MS-RDPBCGR] 2.2.4). Output is unchanged, so this is behaviourally
                        // transparent to callers that do not opt into auto-reconnect.
                        if let InfoData::LogonExtended(extended) = &session_info.info_data
                            && let Some(auto_reconnect) = &extended.auto_reconnect
                        {
                            debug!(
                                logon_id = auto_reconnect.logon_id,
                                "Captured server auto-reconnect cookie"
                            );
                            self.reconnect_cookie = Some(auto_reconnect.clone());
                        }
                        Ok(Vec::new())
                    }
                    // FIXME: workaround fix to not terminate the session on "unhandled PDU: Set Keyboard Indicators PDU"
                    ShareDataPdu::SetKeyboardIndicators(data) => {
                        debug!("Got Keyboard Indicators PDU: {data:?}");
                        Ok(Vec::new())
                    }
                    ShareDataPdu::ServerSetErrorInfo(ServerSetErrorInfoPdu(ErrorInfo::ProtocolIndependentCode(
                        ProtocolIndependentCode::None,
                    ))) => {
                        debug!("Received None server error");
                        Ok(Vec::new())
                    }
                    ShareDataPdu::ServerSetErrorInfo(ServerSetErrorInfoPdu(e)) => {
                        // This is a part of server-side graceful disconnect procedure defined
                        // in [MS-RDPBCGR].
                        //
                        // [MS-RDPBCGR]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/149070b0-ecec-4c20-af03-934bbc48adb8
                        let desc = DisconnectDescription::ErrorInfo(e);
                        Ok(vec![ProcessorOutput::Disconnect(desc)])
                    }
                    ShareDataPdu::ShutdownDenied => {
                        debug!("ShutdownDenied received, session will be closed");

                        // As defined in [MS-RDPBCGR], when `ShareDataPdu::ShutdownDenied` is received, we
                        // need to send a disconnect ultimatum to the server if we want to proceed with the
                        // session shutdown.
                        //
                        // [MS-RDPBCGR]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/27915739-8f77-487e-9927-55008af7fd68
                        let ultimatum = McsMessage::DisconnectProviderUltimatum(
                            DisconnectProviderUltimatum::from_reason(DisconnectReason::UserRequested),
                        );

                        let encoded_pdu = ironrdp_core::encode_vec(&X224(ultimatum)).map_err(SessionError::encode);

                        Ok(vec![
                            ProcessorOutput::ResponseFrame(encoded_pdu?),
                            ProcessorOutput::Disconnect(DisconnectDescription::McsDisconnect(
                                DisconnectReason::UserRequested,
                            )),
                        ])
                    }

                    // Server-initiated Auto-Detect Request PDU ([MS-RDPBCGR] §2.2.14).
                    ShareDataPdu::AutoDetectReq(req) => {
                        use ironrdp_pdu::rdp::autodetect::AutoDetectRequest;

                        match req {
                            AutoDetectRequest::RttRequest {
                                sequence_number,
                                request_type,
                            } => {
                                // Respond immediately with an RTT Measure Response carrying the
                                // same sequence number as the request.
                                //
                                // [MS-RDPBCGR] §2.2.14.2.1
                                debug!(
                                    sequence_number,
                                    request_type, "Received Auto-Detect RTT Request; sending RTT Response"
                                );

                                let rsp = AutoDetectResponse::RttResponse { sequence_number };

                                let mut buf = WriteBuf::new();
                                self.encode_static(&mut buf, ShareDataPdu::AutoDetectRsp(rsp))?;
                                Ok(vec![ProcessorOutput::ResponseFrame(buf.filled().to_vec())])
                            }

                            AutoDetectRequest::BandwidthMeasureStart {
                                sequence_number,
                                request_type,
                            } => {
                                // A bandwidth measurement window is opening.  Tracking start time
                                // and byte counts for a full BW-measurement state machine is out of
                                // scope for this pass; log and continue.
                                debug!(
                                    sequence_number,
                                    request_type, "Received Auto-Detect Bandwidth Measure Start"
                                );
                                Ok(Vec::new())
                            }

                            AutoDetectRequest::BandwidthMeasurePayload {
                                sequence_number,
                                payload,
                            } => {
                                // Payload-only PDU sent during connect-time BW detection; no
                                // response required.
                                debug!(
                                    sequence_number,
                                    payload_len = payload.len(),
                                    "Received Auto-Detect Bandwidth Measure Payload"
                                );
                                Ok(Vec::new())
                            }

                            AutoDetectRequest::BandwidthMeasureStop {
                                sequence_number,
                                request_type,
                                ..
                            } => {
                                // A full BW-results response (BandwidthMeasureResults) requires
                                // accurate timeDelta and byteCount from the measurement window.
                                // Without that state machine the best we can do is log.
                                warn!(
                                    sequence_number,
                                    request_type,
                                    "Received Auto-Detect Bandwidth Measure Stop; \
                                     BW measurement response not yet implemented"
                                );
                                Ok(Vec::new())
                            }

                            AutoDetectRequest::NetworkCharacteristicsResult {
                                sequence_number,
                                request_type,
                                ..
                            } => {
                                // The server is informing us of its network view; no response
                                // required per [MS-RDPBCGR] §2.2.14.1.5.
                                debug!(
                                    sequence_number,
                                    request_type, "Received Auto-Detect Network Characteristics Result from server"
                                );
                                Ok(Vec::new())
                            }
                        }
                    }

                    _ => Err(reason_err!(
                        "IO channel",
                        "unhandled PDU: {:?}",
                        ctx.pdu.as_short_name()
                    )),
                }
            }
            ironrdp_connector::legacy::IoChannelPdu::MultitransportRequest(pdu) => {
                debug!(
                    "Received Initiate Multitransport Request: request_id={}",
                    pdu.request_id
                );
                Ok(vec![ProcessorOutput::MultitransportRequest(pdu)])
            }
            ironrdp_connector::legacy::IoChannelPdu::DeactivateAll(_) => Ok(vec![ProcessorOutput::DeactivateAll(
                Box::new(self.connection_activation.reset_clone()),
            )]),
            ironrdp_connector::legacy::IoChannelPdu::Redirection(redirection) => {
                debug!(
                    redir_flags = format_args!("{:#010x}", redirection.redir_flags),
                    has_load_balance_info = redirection.load_balance_info.is_some(),
                    "Received Server Redirection PDU"
                );
                Ok(vec![ProcessorOutput::Redirect(Box::new(redirection))])
            }
        }
    }

    /// Send a pdu on the static global channel. Typically used to send input events
    pub fn encode_static(&self, output: &mut WriteBuf, pdu: ShareDataPdu) -> SessionResult<usize> {
        let written = ironrdp_connector::legacy::encode_share_data(
            self.user_channel_id,
            self.io_channel_id,
            self.share_id,
            pdu,
            output,
        )
        .map_err(crate::legacy::map_error)?;
        Ok(written)
    }

    /// Encodes a raw IO-channel PDU such as a multitransport response.
    pub fn encode_io_channel<T: Encode>(&self, output: &mut WriteBuf, pdu: &T) -> SessionResult<usize> {
        encode_io_channel_pdu(self.user_channel_id, self.io_channel_id, output, pdu)
    }
}

/// Processes a vector of [`SvcMessage`] in preparation for sending them to the server on the `channel_id` channel.
///
/// This includes chunkifying the messages, adding MCS, x224, and tpkt headers, and encoding them into a buffer.
/// The messages returned here are ready to be sent to the server.
///
/// The caller is responsible for ensuring that the `channel_id` corresponds to the correct channel.
fn process_svc_messages(messages: Vec<SvcMessage>, channel_id: u16, initiator_id: u16) -> SessionResult<Vec<u8>> {
    client_encode_svc_messages(messages, channel_id, initiator_id).map_err(SessionError::encode)
}

fn encode_io_channel_pdu<T: Encode>(
    initiator_id: u16,
    channel_id: u16,
    output: &mut WriteBuf,
    pdu: &T,
) -> SessionResult<usize> {
    ironrdp_connector::legacy::encode_send_data_request(initiator_id, channel_id, pdu, output)
        .map_err(crate::legacy::map_error)
}

#[cfg(test)]
mod tests {
    use ironrdp_connector::legacy::decode_send_data_indication;
    use ironrdp_core::{WriteBuf, decode};
    use ironrdp_pdu::rdp::headers::{BasicSecurityHeader, BasicSecurityHeaderFlags};
    use ironrdp_pdu::rdp::multitransport::{MultitransportRequestPdu, MultitransportResponsePdu, RequestedProtocol};

    use super::encode_io_channel_pdu;

    #[test]
    fn encode_io_channel_pdu_wraps_multitransport_response_on_global_channel() {
        let mut output = WriteBuf::new();
        let response = MultitransportResponsePdu::abort(42);

        let written =
            encode_io_channel_pdu(1003, 1001, &mut output, &response).expect("encode multitransport response");
        assert!(written > 0);

        let data_ctx = decode_send_data_indication(output.filled()).expect("decode send-data indication");
        assert_eq!(data_ctx.initiator_id, 1003);
        assert_eq!(data_ctx.channel_id, 1001);

        let decoded = decode::<MultitransportResponsePdu>(data_ctx.user_data).expect("decode multitransport response");
        assert_eq!(decoded.request_id, 42);
        assert_eq!(decoded.hr_response, MultitransportResponsePdu::E_ABORT);
    }

    #[test]
    fn encode_io_channel_pdu_preserves_multitransport_request_payload() {
        let mut output = WriteBuf::new();
        let request = MultitransportRequestPdu {
            security_header: BasicSecurityHeader {
                flags: BasicSecurityHeaderFlags::TRANSPORT_REQ,
            },
            request_id: 7,
            requested_protocol: RequestedProtocol::UdpFecL,
            security_cookie: [0xAB; 16],
        };

        let written = encode_io_channel_pdu(1003, 1001, &mut output, &request).expect("encode multitransport request");
        assert!(written > 0);

        let data_ctx = decode_send_data_indication(output.filled()).expect("decode send-data indication");
        assert_eq!(data_ctx.initiator_id, 1003);
        assert_eq!(data_ctx.channel_id, 1001);

        let decoded = decode::<MultitransportRequestPdu>(data_ctx.user_data).expect("decode multitransport request");
        assert_eq!(decoded.request_id, 7);
        assert_eq!(decoded.requested_protocol, RequestedProtocol::UdpFecL);
        assert_eq!(decoded.security_cookie, [0xAB; 16]);
    }
}
