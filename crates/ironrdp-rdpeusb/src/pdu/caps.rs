//! Capability exchange PDUs for [MS-RDPEUSB § 2.2.3].
//!
//! The server sends [`RimExchangeCapabilityRequest`] to advertise its capability version; the
//! client replies with [`RimExchangeCapabilityResponse`].
//!
//! [MS-RDPEUSB § 2.2.3]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/6aee4e70-9d3b-49d7-a9b9-3c437cb27c8e

use alloc::borrow::ToOwned as _;

use ironrdp_core::{
    DecodeError, DecodeResult, Encode, EncodeResult, ReadCursor, WriteCursor, ensure_fixed_part_size,
    unsupported_value_err,
};

use crate::ensure_payload_size;
use crate::pdu::header::SharedMsgHeader;
use crate::pdu::utils::HResult;

/// Interface manipulation capability version identifier.
///
/// * [MS-RDPEUSB § 2.2.3.1][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/6aee4e70-9d3b-49d7-a9b9-3c437cb27c8e
#[repr(u32)]
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum Capability {
    #[doc(alias = "RIM_CAPABILITY_VERSION_01")]
    RimCapabilityVersion01 = 0x1,
}

impl Capability {
    /// Wire size of an encoded `Capability` field.
    pub const FIXED_PART_SIZE: usize = size_of::<u32>();
}

impl TryFrom<u32> for Capability {
    type Error = DecodeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 0x1 {
            Ok(Self::RimCapabilityVersion01)
        } else {
            Err(unsupported_value_err!(
                "CapabilityValue",
                alloc::format!("{value:#x}")
            ))
        }
    }
}

/// Sent by the server to announce its capability version.
///
/// * [MS-RDPEUSB § 2.2.3.1 RIM_EXCHANGE_CAPABILITY_REQUEST][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/6aee4e70-9d3b-49d7-a9b9-3c437cb27c8e
#[doc(alias = "RIM_EXCHANGE_CAPABILITY_REQUEST")]
#[derive(Debug)]
pub struct RimExchangeCapabilityRequest {
    pub header: SharedMsgHeader,
    pub capability: Capability,
}

impl RimExchangeCapabilityRequest {
    /// Size in bytes of the payload (everything after the header).
    pub const PAYLOAD_SIZE: usize = Capability::FIXED_PART_SIZE;

    /// Total encoded size of this PDU.
    pub const FIXED_PART_SIZE: usize = Self::PAYLOAD_SIZE + SharedMsgHeader::SIZE_WHEN_NOT_RSP;

    /// Decodes this PDU from `src`, given an already-decoded `header`.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        ensure_payload_size!(in: src);
        let capability = Capability::try_from(src.read_u32())?;
        Ok(Self { header, capability })
    }
}

impl Encode for RimExchangeCapabilityRequest {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);
        self.header.encode(dst)?;
        #[expect(clippy::as_conversions, reason = "cast repr(u32) enum discriminant to u32")]
        dst.write_u32(self.capability as u32);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "RIM_EXCHANGE_CAPABILITY_REQUEST"
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

/// Sent by the client in response to [`RimExchangeCapabilityRequest`].
///
/// * [MS-RDPEUSB § 2.2.3.2 RIM_EXCHANGE_CAPABILITY_RESPONSE][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/6aee4e70-9d3b-49d7-a9b9-3c437cb27c8e
#[doc(alias = "RIM_EXCHANGE_CAPABILITY_RESPONSE")]
#[derive(Debug)]
pub struct RimExchangeCapabilityResponse {
    pub header: SharedMsgHeader,
    pub capability: Capability,
    pub result: HResult,
}

impl RimExchangeCapabilityResponse {
    /// Size in bytes of the payload (everything after the header).
    pub const PAYLOAD_SIZE: usize = Capability::FIXED_PART_SIZE + size_of::<HResult>();

    /// Total encoded size of this PDU.
    pub const FIXED_PART_SIZE: usize = Self::PAYLOAD_SIZE + SharedMsgHeader::SIZE_WHEN_RSP;

    /// Decodes this PDU from `src`, given an already-decoded `header`.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        ensure_payload_size!(in: src);
        let capability = Capability::try_from(src.read_u32())?;
        let result = src.read_u32();
        Ok(Self {
            header,
            capability,
            result,
        })
    }
}

impl Encode for RimExchangeCapabilityResponse {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);
        self.header.encode(dst)?;
        #[expect(clippy::as_conversions, reason = "cast repr(u32) enum discriminant to u32")]
        dst.write_u32(self.capability as u32);
        dst.write_u32(self.result);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "RIM_EXCHANGE_CAPABILITY_RESPONSE"
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}
