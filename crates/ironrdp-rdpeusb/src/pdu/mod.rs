//! PDU definitions for [MS-RDPEUSB]: Remote Desktop Protocol: USB Devices Virtual Channel
//! Extension (URBDRC).
//!
//! # Protocol structure
//!
//! Every URBDRC message starts with a [`SharedMsgHeader`][header::SharedMsgHeader] that carries
//! an [`InterfaceId`][header::InterfaceId], a [`Mask`][header::Mask], a message ID, and — when
//! not a response — a [`FunctionId`][header::FunctionId].
//!
//! The well-known interfaces and their default IDs are:
//!
//! | Interface | Default ID | Direction |
//! |-----------|-----------|-----------|
//! | Exchange Capabilities | `0x0` | bidirectional |
//! | Device Sink | `0x1` | client → server |
//! | Channel Notification | `0x2` / `0x3` | bidirectional |
//! | USB Device | dynamic | server → client |
//! | Request Completion | dynamic | client → server |
//!
//! [MS-RDPEUSB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/a1004d0e-99e9-4968-894b-0b924ef2f125

pub mod caps;
pub mod chan_notify;
pub mod dev_sink;
pub mod header;
pub mod req_complete;
pub mod ts_urb;
pub mod usb_dev;
pub mod utils;

use ironrdp_core::{Encode, EncodeResult, WriteCursor};

use crate::pdu::caps::RimExchangeCapabilityRequest;
use crate::pdu::usb_dev::{CancelRequest, RegisterRequestCallback};

/// Top-level enum covering server-to-client URBDRC PDUs.
///
/// Decoding is left to callers who have already parsed the
/// [`SharedMsgHeader`][header::SharedMsgHeader] and know the interface context, because interface
/// ID assignment is dynamic for the USB Device and Request Completion interfaces.
#[derive(Debug)]
pub enum UrbdrcServerPdu {
    /// Interface Manipulation Exchange Capabilities request from the server.
    Caps(RimExchangeCapabilityRequest),
    /// Cancel an outstanding IO request.
    CancelReq(CancelRequest),
    /// Register (or unregister) the Request Completion interface callback.
    RegReqCallback(RegisterRequestCallback),
}

impl Encode for UrbdrcServerPdu {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        match self {
            Self::Caps(pdu) => pdu.encode(dst),
            Self::CancelReq(pdu) => pdu.encode(dst),
            Self::RegReqCallback(pdu) => pdu.encode(dst),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Caps(pdu) => pdu.name(),
            Self::CancelReq(pdu) => pdu.name(),
            Self::RegReqCallback(pdu) => pdu.name(),
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::Caps(pdu) => pdu.size(),
            Self::CancelReq(pdu) => pdu.size(),
            Self::RegReqCallback(pdu) => pdu.size(),
        }
    }
}
