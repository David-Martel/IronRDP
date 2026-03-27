//! PDUs specific to the [USB Device][1] interface.
//!
//! This interface is used by the server to communicate with the client about USB device operations.
//! It has no default interface ID; the ID is allocated dynamically during the lifetime of a USB
//! redirection channel.
//!
//! [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/034257d7-f7a8-4fe1-b8c2-87ac8dc4f50e

use alloc::format;

use ironrdp_core::{
    DecodeError, DecodeOwned as _, DecodeResult, Encode, EncodeResult, ReadCursor, WriteCursor,
    ensure_fixed_part_size, ensure_size, unsupported_value_err,
};
use ironrdp_pdu::utils::strict_sum;
use ironrdp_str::prefixed::Cch32String;

use crate::ensure_payload_size;
use crate::pdu::header::{InterfaceId, SharedMsgHeader};
use crate::pdu::utils::{HResult, RequestId, RequestIdIoctl};

/// Sent from the server to the client to cancel an outstanding IO request.
///
/// * [MS-RDPEUSB § 2.2.6.1 Cancel Request Message (CANCEL_REQUEST)][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/93912b05-1fc8-4a43-8abd-78d9aab65d71
#[doc(alias = "CANCEL_REQUEST")]
#[derive(Debug)]
pub struct CancelRequest {
    pub header: SharedMsgHeader,
    /// ID of the outstanding IO request to cancel.
    ///
    /// Must match a request previously sent via `IO_CONTROL`, `INTERNAL_IO_CONTROL`,
    /// `TRANSFER_IN_REQUEST`, or `TRANSFER_OUT_REQUEST`.
    pub request_id: RequestId,
}

impl CancelRequest {
    const PAYLOAD_SIZE: usize = size_of::<RequestId>();
    const FIXED_PART_SIZE: usize = Self::PAYLOAD_SIZE + SharedMsgHeader::SIZE_WHEN_NOT_RSP;

    /// Decodes this PDU from `src`, given an already-decoded `header`.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        ensure_payload_size!(in: src);
        let request_id = src.read_u32();
        Ok(Self { header, request_id })
    }
}

impl Encode for CancelRequest {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);
        self.header.encode(dst)?;
        dst.write_u32(self.request_id);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "CANCEL_REQUEST"
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

/// Sent from the server to the client to assign an interface ID for the Request Completion
/// interface.
///
/// * [MS-RDPEUSB § 2.2.6.2 Register Request Callback Message (REGISTER_REQUEST_CALLBACK)][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/8693de72-5e87-4b64-a252-101e865311a5
#[doc(alias = "REGISTER_REQUEST_CALLBACK")]
#[derive(Debug)]
pub struct RegisterRequestCallback {
    pub header: SharedMsgHeader,
    /// The interface ID to be used for all Request Completion messages.
    ///
    /// `None` if the server is unregistering the callback (`NumRequestCompletion == 0`).
    pub request_completion: Option<InterfaceId>,
}

impl RegisterRequestCallback {
    /// Decodes this PDU from `src`, given an already-decoded `header`.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        ensure_size!(in: src, size: size_of::<u32>()); // NumRequestCompletion
        let request_completion = if src.read_u32() == 0 {
            None
        } else {
            ensure_size!(in: src, size: InterfaceId::FIXED_PART_SIZE);
            Some(InterfaceId::from(src.read_u32()))
        };
        Ok(Self {
            header,
            request_completion,
        })
    }
}

impl Encode for RegisterRequestCallback {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_size!(in: dst, size: self.size());
        self.header.encode(dst)?;
        if let Some(request_completion) = self.request_completion {
            dst.write_u32(0x1); // NumRequestCompletion
            dst.write_u32(request_completion.into());
        } else {
            dst.write_u32(0x0); // NumRequestCompletion
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "REGISTER_REQUEST_CALLBACK"
    }

    fn size(&self) -> usize {
        const NUM_REQUEST_COMPLETION_SIZE: usize = size_of::<u32>();
        let request_completion_size = match self.request_completion {
            Some(_) => InterfaceId::FIXED_PART_SIZE,
            None => 0,
        };
        strict_sum(&[
            SharedMsgHeader::SIZE_WHEN_NOT_RSP + NUM_REQUEST_COMPLETION_SIZE + request_completion_size,
        ])
    }
}

/// USB Internal I/O control codes understood by the URBDRC protocol.
///
/// * [MS-RDPEUSB § 2.2.12 USB IO Control Code][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/4f4574f0-9368-4708-8f98-06aa2f44e198
#[repr(u32)]
#[non_exhaustive]
#[doc(alias = "IOCTL_INTERNAL_USB")]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UsbIoctlCode {
    #[doc(alias = "IOCTL_INTERNAL_USB_RESET_PORT")]
    ResetPort = 0x220_007,
    #[doc(alias = "IOCTL_INTERNAL_USB_GET_PORT_STATUS")]
    GetPortStatus = 0x220_013,
    #[doc(alias = "IOCTL_INTERNAL_USB_GET_HUB_COUNT")]
    GetHubCount = 0x220_01B,
    #[doc(alias = "IOCTL_INTERNAL_USB_CYCLE_PORT")]
    CyclePort = 0x220_01F,
    #[doc(alias = "IOCTL_INTERNAL_USB_GET_HUB_NAME")]
    GetHubName = 0x220_020,
    #[doc(alias = "IOCTL_INTERNAL_USB_GET_BUS_INFO")]
    GetBusInfo = 0x220_420,
    #[doc(alias = "IOCTL_INTERNAL_USB_GET_CONTROLLER_NAME")]
    GetControllerName = 0x220_424,
}

impl UsbIoctlCode {
    /// Wire size of an encoded `UsbIoctlCode`.
    pub const FIXED_PART_SIZE: usize = size_of::<u32>();
}

impl TryFrom<u32> for UsbIoctlCode {
    type Error = DecodeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        use UsbIoctlCode::*;
        match value {
            0x220_007 => Ok(ResetPort),
            0x220_013 => Ok(GetPortStatus),
            0x220_01B => Ok(GetHubCount),
            0x220_01F => Ok(CyclePort),
            0x220_020 => Ok(GetHubName),
            0x220_420 => Ok(GetBusInfo),
            0x220_424 => Ok(GetControllerName),
            _ => Err(unsupported_value_err!(
                "IoControlCode",
                format!(
                    "is: {value:#X}; expected one of: \
IOCTL_INTERNAL_USB_RESET_PORT (0x00220007), \
IOCTL_INTERNAL_USB_GET_PORT_STATUS (0x00220013), \
IOCTL_INTERNAL_USB_GET_HUB_COUNT (0x0022001B), \
IOCTL_INTERNAL_USB_CYCLE_PORT (0x0022001F), \
IOCTL_INTERNAL_USB_GET_HUB_NAME (0x00220020), \
IOCTL_INTERNAL_USB_GET_BUS_INFO (0x00220420), \
IOCTL_INTERNAL_USB_GET_CONTROLLER_NAME (0x00220424)"
                )
            )),
        }
    }
}

/// Sent from the server to the client to perform a USB I/O control request.
///
/// Note: As of MS-RDPEUSB v20240423, all USB IO Control Codes require `InputBufferSize == 0`.
///
/// * [MS-RDPEUSB § 2.2.6.5 IO Control Message (IO_CONTROL)][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/4f4574f0-9368-4708-8f98-06aa2f44e198
#[doc(alias = "IO_CONTROL")]
#[derive(Debug)]
pub struct IoCtl {
    pub header: SharedMsgHeader,
    pub ioctl_code: UsbIoctlCode,
    pub output_buffer_size: u32,
    pub request_id: RequestIdIoctl,
}

impl IoCtl {
    #[expect(clippy::identity_op, reason = "zero-size InputBuffer documented for clarity")]
    /// Size in bytes of the payload fields (excluding the header).
    pub const PAYLOAD_SIZE: usize = UsbIoctlCode::FIXED_PART_SIZE
        + size_of::<u32>() // InputBufferSize (always 0)
        + 0 // InputBuffer (empty because InputBufferSize == 0)
        + size_of::<u32>() // OutputBufferSize
        + size_of::<RequestIdIoctl>(); // RequestId

    /// Total encoded size of this PDU.
    pub const FIXED_PART_SIZE: usize = Self::PAYLOAD_SIZE + SharedMsgHeader::SIZE_WHEN_NOT_RSP;

    /// Decodes this PDU from `src`, given an already-decoded `header`.
    ///
    /// # Errors
    ///
    /// Returns an error if `InputBufferSize != 0` or if `OutputBufferSize` is inconsistent with
    /// `IoControlCode` per MS-RDPEUSB § 2.2.6.5.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        ensure_payload_size!(in: src);

        let ioctl_code = UsbIoctlCode::try_from(src.read_u32())?;

        let input_buffer_size = src.read_u32();
        if input_buffer_size != 0 {
            return Err(unsupported_value_err!(
                "IO_CONTROL::InputBufferSize",
                format!("is: {input_buffer_size:#X}; should be: 0x0")
            ));
        }

        let output_buffer_size = {
            let size = src.read_u32();
            use UsbIoctlCode::*;
            match ioctl_code {
                ResetPort | CyclePort if size != 0x0 => {
                    return Err(unsupported_value_err!(
                        "IO_CONTROL::OutputBufferSize",
                        format!("is: {size:#X}; should be: 0x0")
                    ));
                }
                GetPortStatus | GetHubCount if size != 0x4 => {
                    return Err(unsupported_value_err!(
                        "IO_CONTROL::OutputBufferSize",
                        format!("is: {size:#X}; should be: 0x4")
                    ));
                }
                _ => size,
            }
        };

        let request_id = src.read_u32();

        Ok(Self {
            header,
            ioctl_code,
            output_buffer_size,
            request_id,
        })
    }
}

impl Encode for IoCtl {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);
        self.header.encode(dst)?;
        #[expect(clippy::as_conversions, reason = "cast repr(u32) enum discriminant to u32")]
        dst.write_u32(self.ioctl_code as u32);
        dst.write_u32(0x0); // InputBufferSize (always 0)
        dst.write_u32(self.output_buffer_size);
        dst.write_u32(self.request_id);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "IO_CONTROL"
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

/// The only USB internal I/O control code used in the URBDRC protocol.
const IOCTL_TSUSBGD_IOCTL_USBDI_QUERY_BUS_TIME: u32 = 0x0022_4000;

/// Sent from the server to the client to perform a USB internal I/O control request.
///
/// Note: As of MS-RDPEUSB v20240423, only `IOCTL_TSUSBGD_IOCTL_USBDI_QUERY_BUS_TIME` is used,
/// and both `InputBufferSize` and `OutputBufferSize` have fixed values (0 and 4 respectively).
///
/// * [MS-RDPEUSB § 2.2.6.6 Internal IO Control Message (INTERNAL_IO_CONTROL)][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/55d1cd44-eda3-4cba-931c-c3cb8b3c3c92
#[doc(alias = "INTERNAL_IO_CONTROL")]
#[derive(Debug)]
pub struct InternalIoCtl {
    pub header: SharedMsgHeader,
    pub request_id: RequestIdIoctl,
}

impl InternalIoCtl {
    #[expect(clippy::identity_op, reason = "zero-size InputBuffer documented for clarity")]
    /// Size in bytes of the payload fields (excluding the header).
    pub const PAYLOAD_SIZE: usize = size_of::<u32>() // IoControlCode (fixed)
        + size_of::<u32>() // InputBufferSize (always 0)
        + 0 // InputBuffer (empty)
        + size_of::<u32>() // OutputBufferSize (always 4)
        + size_of::<RequestIdIoctl>();

    /// Total encoded size of this PDU.
    pub const FIXED_PART_SIZE: usize = Self::PAYLOAD_SIZE + SharedMsgHeader::SIZE_WHEN_NOT_RSP;

    /// Decodes this PDU from `src`, given an already-decoded `header`.
    ///
    /// # Errors
    ///
    /// Returns an error if `IoControlCode`, `InputBufferSize`, or `OutputBufferSize` differ from
    /// their expected fixed values.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        ensure_payload_size!(in: src);

        let code = src.read_u32();
        if code != IOCTL_TSUSBGD_IOCTL_USBDI_QUERY_BUS_TIME {
            return Err(unsupported_value_err!(
                "INTERNAL_IO_CONTROL::IoControlCode",
                format!(
                    "is: {code:#X}; should be: {IOCTL_TSUSBGD_IOCTL_USBDI_QUERY_BUS_TIME:#X}"
                )
            ));
        }
        let input_size = src.read_u32();
        if input_size != 0x0 {
            return Err(unsupported_value_err!(
                "INTERNAL_IO_CONTROL::InputBufferSize",
                format!("is: {input_size:#X}; should be: 0x0")
            ));
        }
        let output_size = src.read_u32();
        if output_size != 0x4 {
            return Err(unsupported_value_err!(
                "INTERNAL_IO_CONTROL::OutputBufferSize",
                format!("is: {output_size:#X}; should be: 0x4")
            ));
        }
        let request_id = src.read_u32();

        Ok(Self { header, request_id })
    }
}

impl Encode for InternalIoCtl {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);
        self.header.encode(dst)?;
        dst.write_u32(IOCTL_TSUSBGD_IOCTL_USBDI_QUERY_BUS_TIME);
        dst.write_u32(0x0); // InputBufferSize
        dst.write_u32(0x4); // OutputBufferSize
        dst.write_u32(self.request_id);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "INTERNAL_IO_CONTROL"
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

/// Sent from the server to the client to query human-readable device text.
///
/// * [MS-RDPEUSB § 2.2.6.7 Query Device Text Message (QUERY_DEVICE_TEXT)][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/3db78d7e-1d2a-4d06-9bde-81d8ebbd32e6
#[doc(alias = "QUERY_DEVICE_TEXT")]
#[derive(Debug)]
pub struct QueryDeviceText {
    pub header: SharedMsgHeader,
    pub text_type: u32,
    pub locale_id: u32,
}

impl QueryDeviceText {
    /// Size in bytes of the payload fields (excluding the header).
    pub const PAYLOAD_SIZE: usize = size_of::<u32>() + size_of::<u32>();

    /// Total encoded size of this PDU.
    pub const FIXED_PART_SIZE: usize = Self::PAYLOAD_SIZE + SharedMsgHeader::SIZE_WHEN_NOT_RSP;

    /// Decodes this PDU from `src`, given an already-decoded `header`.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        ensure_payload_size!(in: src);
        let text_type = src.read_u32();
        let locale_id = src.read_u32();
        Ok(Self {
            header,
            text_type,
            locale_id,
        })
    }
}

impl Encode for QueryDeviceText {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);
        self.header.encode(dst)?;
        dst.write_u32(self.text_type);
        dst.write_u32(self.locale_id);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "QUERY_DEVICE_TEXT"
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

/// Sent from the client to the server in response to [`QueryDeviceText`].
///
/// * [MS-RDPEUSB § 2.2.6.8 Query Device Text Response (QUERY_DEVICE_TEXT_RSP)][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/3db78d7e-1d2a-4d06-9bde-81d8ebbd32e6
#[doc(alias = "QUERY_DEVICE_TEXT_RSP")]
#[derive(Debug)]
pub struct QueryDeviceTextRsp {
    pub header: SharedMsgHeader,
    pub device_description: Cch32String,
    pub hresult: HResult,
}

impl QueryDeviceTextRsp {
    /// Decodes this PDU from `src`, given an already-decoded `header`.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        let device_description = Cch32String::decode_owned(src)?;
        ensure_size!(in: src, size: size_of::<HResult>());
        let hresult = src.read_u32();
        Ok(Self {
            header,
            device_description,
            hresult,
        })
    }
}

impl Encode for QueryDeviceTextRsp {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_size!(in: dst, size: self.size());
        self.header.encode(dst)?;
        self.device_description.encode(dst)?;
        dst.write_u32(self.hresult);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "QUERY_DEVICE_TEXT_RSP"
    }

    fn size(&self) -> usize {
        strict_sum(&[
            SharedMsgHeader::SIZE_WHEN_RSP
                + self.device_description.size()
                + const { size_of::<HResult>() },
        ])
    }
}
