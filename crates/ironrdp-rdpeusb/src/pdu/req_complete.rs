//! PDUs specific to the [Request Completion][1] interface.
//!
//! Used by the client to send the final result for a request previously sent from the server.
//! The unique interface ID for this interface is provided by the server using the
//! [`REGISTER_REQUEST_CALLBACK`][super::usb_dev::RegisterRequestCallback] message, during the
//! lifecycle of a USB redirection channel.
//!
//! [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/c0a146fc-20cf-4897-af27-a3c5474151ac

use alloc::vec::Vec;

use ironrdp_core::{
    DecodeResult, Encode, EncodeResult, ReadCursor, WriteCursor, ensure_size, invalid_field_err, other_err,
};
use ironrdp_pdu::utils::strict_sum;

use crate::pdu::header::SharedMsgHeader;
use crate::pdu::utils::{HResult, RequestIdIoctl};

/// Win32 error code for an insufficient output buffer.
///
/// * [MS-ERREF § 2.2 Win32 Error Codes][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/18d8fbe8-a967-4f1c-ae50-99ca8e491d2d
const ERROR_INSUFFICIENT_BUFFER: u32 = 0x7A;

/// Facility code used to convert Win32 errors to HRESULT values.
///
/// * [MS-ERREF § 2.1.2 HRESULT From WIN32 Error Code Macro][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/0c0bcf55-277e-4120-b5dc-f6115fc8dc38
const FACILITY_WIN32: u32 = 0x7;

/// Converts a Win32 error code to an HRESULT value.
///
/// * [MS-ERREF § 2.1.2 HRESULT From WIN32 Error Code Macro][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/0c0bcf55-277e-4120-b5dc-f6115fc8dc38
macro_rules! HRESULT_FROM_WIN32 {
    ($x:expr) => {{
        #[expect(clippy::cast_possible_wrap, clippy::as_conversions)]
        if ($x as i32) <= 0 {
            $x
        } else {
            $x & 0x0000_FFFF | (FACILITY_WIN32 << 16) | 0x8000_0000
        }
    }};
}

const HRESULT_FROM_WIN32_ERROR_INSUFFICIENT_BUFFER: u32 = HRESULT_FROM_WIN32!(ERROR_INSUFFICIENT_BUFFER);

/// Sent by the client to report the result of an `IO_CONTROL` or `INTERNAL_IO_CONTROL` request.
///
/// * [MS-RDPEUSB § 2.2.7.1 IOCONTROL_COMPLETION][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/b1722374-0658-47ba-8368-87bf9d3db4d4
#[doc(alias = "IOCONTROL_COMPLETION")]
#[derive(Debug)]
pub struct IoctlCompletion {
    pub header: SharedMsgHeader,
    pub request_id: RequestIdIoctl,
    pub hresult: HResult,
    pub information: u32,
    pub output_buffer_size: u32,
    pub output_buffer: Vec<u8>,
}

impl IoctlCompletion {
    /// Decodes this PDU from `src`, given an already-decoded `header`.
    ///
    /// # Errors
    ///
    /// Returns an error if the `Information` and `OutputBufferSize` fields are inconsistent with
    /// the `HResult` value as specified in MS-RDPEUSB § 2.2.7.1.
    pub fn decode(src: &mut ReadCursor<'_>, header: SharedMsgHeader) -> DecodeResult<Self> {
        let fixed_bytes =
            size_of::<RequestIdIoctl>() + size_of::<HResult>() + size_of::<u32>() + size_of::<u32>();
        ensure_size!(in: src, size: fixed_bytes);

        let request_id = src.read_u32();
        let hresult = src.read_u32();
        let information = src.read_u32();
        let output_buffer_size = src.read_u32();

        if hresult == 0 {
            if information != output_buffer_size {
                return Err(invalid_field_err!(
                    "Information",
                    "HResult is 0x0 (success) but Information != OutputBufferSize"
                ));
            }
        } else if hresult != HRESULT_FROM_WIN32_ERROR_INSUFFICIENT_BUFFER && output_buffer_size != 0 {
            // Per MS-RDPEUSB § 2.2.7.1: for any HResult other than SUCCESS or
            // HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER), OutputBufferSize MUST be 0.
            return Err(invalid_field_err!(
                "OutputBufferSize",
                "OutputBufferSize must be 0 when HResult is not 0 or HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER)"
            ));
        }

        let output_buffer = match hresult {
            0 | HRESULT_FROM_WIN32_ERROR_INSUFFICIENT_BUFFER => {
                let n = information.try_into().map_err(|e| other_err!(source: e))?;
                Vec::from(src.read_slice(n))
            }
            _ => Vec::new(),
        };

        Ok(Self {
            header,
            request_id,
            hresult,
            information,
            output_buffer_size,
            output_buffer,
        })
    }
}

impl Encode for IoctlCompletion {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_size!(in: dst, size: self.size());

        self.header.encode(dst)?;
        dst.write_u32(self.request_id);
        dst.write_u32(self.hresult);
        dst.write_u32(self.information);
        dst.write_u32(self.output_buffer_size);
        dst.write_slice(&self.output_buffer);

        Ok(())
    }

    fn name(&self) -> &'static str {
        "IOCONTROL_COMPLETION"
    }

    fn size(&self) -> usize {
        #[expect(
            clippy::as_conversions,
            reason = "information is a u32 read from wire; usize >= u32 on all supported targets"
        )]
        let out_buf_len = if self.hresult == 0 {
            self.output_buffer.len()
        } else if self.hresult == HRESULT_FROM_WIN32!(ERROR_INSUFFICIENT_BUFFER) {
            self.information as usize
        } else {
            0
        };

        strict_sum(&[
            SharedMsgHeader::SIZE_WHEN_NOT_RSP
                + const {
                    size_of::<RequestIdIoctl>()
                        + size_of::<HResult>()
                        + size_of::<u32>() // Information
                        + size_of::<u32>() // OutputBufferSize
                }
                + out_buf_len,
        ])
    }
}
