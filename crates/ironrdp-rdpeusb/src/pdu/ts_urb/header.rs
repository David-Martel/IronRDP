//! Header shared by all `TS_URB_*` structures defined in [MS-RDPEUSB].
//!
//! [MS-RDPEUSB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/a1004d0e-99e9-4968-894b-0b924ef2f125

use alloc::format;

use ironrdp_core::{
    Decode, DecodeError, DecodeResult, Encode, EncodeResult, ReadCursor, WriteCursor, ensure_fixed_part_size,
    unsupported_value_err,
};

use crate::pdu::utils::RequestIdTsUrb;

/// Numeric code identifying the requested operation for a USB Request Block (URB).
///
/// This corresponds to the `Function` field of the Windows `_URB_HEADER` structure.
///
/// * [WDK: USB: _URB_HEADER][1]
/// * [USB request blocks (URBs)][2]
///
/// [1]: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/usb/ns-usb-_urb_header
/// [2]: https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/communicating-with-a-usb-device
#[repr(u16)]
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum UrbFunction {
    #[doc(alias = "URB_FUNCTION_SELECT_CONFIGURATION")]
    SelectConfiguration = 0,
    #[doc(alias = "URB_FUNCTION_SELECT_INTERFACE")]
    SelectInterface = 1,
    #[doc(alias = "URB_FUNCTION_ABORT_PIPE")]
    AbortPipe = 2,
    #[doc(alias = "URB_FUNCTION_GET_CURRENT_FRAME_NUMBER")]
    GetCurrentFrameNumber = 7,
    #[doc(alias = "URB_FUNCTION_CONTROL_TRANSFER")]
    ControlTransfer = 8,
    #[doc(alias = "URB_FUNCTION_BULK_OR_INTERRUPT_TRANSFER")]
    BulkOrInterruptTransfer = 9,
    #[doc(alias = "URB_FUNCTION_ISOCH_TRANSFER")]
    IsochTransfer = 10,
    #[doc(alias = "URB_FUNCTION_GET_DESCRIPTOR_FROM_DEVICE")]
    GetDescriptorFromDevice = 11,
    #[doc(alias = "URB_FUNCTION_SET_DESCRIPTOR_TO_DEVICE")]
    SetDescriptorToDevice = 12,
    #[doc(alias = "URB_FUNCTION_SET_FEATURE_TO_DEVICE")]
    SetFeatureToDevice = 13,
    #[doc(alias = "URB_FUNCTION_SET_FEATURE_TO_INTERFACE")]
    SetFeatureToInterface = 14,
    #[doc(alias = "URB_FUNCTION_SET_FEATURE_TO_ENDPOINT")]
    SetFeatureToEndpoint = 15,
    #[doc(alias = "URB_FUNCTION_CLEAR_FEATURE_TO_DEVICE")]
    ClearFeatureToDevice = 16,
    #[doc(alias = "URB_FUNCTION_CLEAR_FEATURE_TO_INTERFACE")]
    ClearFeatureToInterface = 17,
    #[doc(alias = "URB_FUNCTION_CLEAR_FEATURE_TO_ENDPOINT")]
    ClearFeatureToEndpoint = 18,
    #[doc(alias = "URB_FUNCTION_GET_STATUS_FROM_DEVICE")]
    GetStatusFromDevice = 19,
    #[doc(alias = "URB_FUNCTION_GET_STATUS_FROM_INTERFACE")]
    GetStatusFromInterface = 20,
    #[doc(alias = "URB_FUNCTION_GET_STATUS_FROM_ENDPOINT")]
    GetStatusFromEndpoint = 21,
    #[doc(alias = "URB_FUNCTION_VENDOR_DEVICE")]
    VendorDevice = 23,
    #[doc(alias = "URB_FUNCTION_VENDOR_INTERFACE")]
    VendorInterface = 24,
    #[doc(alias = "URB_FUNCTION_VENDOR_ENDPOINT")]
    VendorEndpoint = 25,
    #[doc(alias = "URB_FUNCTION_CLASS_DEVICE")]
    ClassDevice = 26,
    #[doc(alias = "URB_FUNCTION_CLASS_INTERFACE")]
    ClassInterface = 27,
    #[doc(alias = "URB_FUNCTION_CLASS_ENDPOINT")]
    ClassEndpoint = 28,
    #[doc(alias = "URB_FUNCTION_SYNC_RESET_PIPE_AND_CLEAR_STALL")]
    SyncResetPipeAndClearStall = 30,
    #[doc(alias = "URB_FUNCTION_CLASS_OTHER")]
    ClassOther = 31,
    #[doc(alias = "URB_FUNCTION_VENDOR_OTHER")]
    VendorOther = 32,
    #[doc(alias = "URB_FUNCTION_GET_STATUS_FROM_OTHER")]
    GetStatusFromOther = 33,
    #[doc(alias = "URB_FUNCTION_CLEAR_FEATURE_TO_OTHER")]
    ClearFeatureToOther = 34,
    #[doc(alias = "URB_FUNCTION_SET_FEATURE_TO_OTHER")]
    SetFeatureToOther = 35,
    #[doc(alias = "URB_FUNCTION_GET_DESCRIPTOR_FROM_ENDPOINT")]
    GetDescriptorFromEndpoint = 36,
    #[doc(alias = "URB_FUNCTION_SET_DESCRIPTOR_TO_ENDPOINT")]
    SetDescriptorToEndpoint = 37,
    #[doc(alias = "URB_FUNCTION_GET_CONFIGURATION")]
    GetConfiguration = 38,
    #[doc(alias = "URB_FUNCTION_GET_INTERFACE")]
    GetInterface = 39,
    #[doc(alias = "URB_FUNCTION_GET_DESCRIPTOR_FROM_INTERFACE")]
    GetDescriptorFromInterface = 40,
    #[doc(alias = "URB_FUNCTION_SET_DESCRIPTOR_TO_INTERFACE")]
    SetDescriptorToInterface = 41,
    #[doc(alias = "URB_FUNCTION_GET_MS_FEATURE_DESCRIPTOR")]
    GetMsFeatureDescriptor = 42,
    #[doc(alias = "URB_FUNCTION_SYNC_RESET_PIPE")]
    SyncResetPipe = 48,
    #[doc(alias = "URB_FUNCTION_SYNC_CLEAR_STALL")]
    SyncClearStall = 49,
    #[doc(alias = "URB_FUNCTION_CONTROL_TRANSFER_EX")]
    ControlTransferEx = 50,
    #[doc(alias = "URB_FUNCTION_CLOSE_STATIC_STREAMS")]
    CloseStaticStreams = 54,
    #[doc(alias = "URB_FUNCTION_BULK_OR_INTERRUPT_TRANSFER_USING_CHAINED_MDL")]
    BulkOrInterruptTransferUsingChainedMdl = 55,
    #[doc(alias = "URB_FUNCTION_ISOCH_TRANSFER_USING_CHAINED_MDL")]
    IsochTransferUsingChainedMdl = 56,
}

impl From<UrbFunction> for u16 {
    #[expect(clippy::as_conversions, reason = "cast repr(u16) enum discriminant to u16")]
    fn from(value: UrbFunction) -> Self {
        value as Self
    }
}

impl TryFrom<u16> for UrbFunction {
    type Error = DecodeError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        use UrbFunction::*;
        match value {
            0 => Ok(SelectConfiguration),
            1 => Ok(SelectInterface),
            2 => Ok(AbortPipe),
            7 => Ok(GetCurrentFrameNumber),
            8 => Ok(ControlTransfer),
            9 => Ok(BulkOrInterruptTransfer),
            10 => Ok(IsochTransfer),
            11 => Ok(GetDescriptorFromDevice),
            12 => Ok(SetDescriptorToDevice),
            13 => Ok(SetFeatureToDevice),
            14 => Ok(SetFeatureToInterface),
            15 => Ok(SetFeatureToEndpoint),
            16 => Ok(ClearFeatureToDevice),
            17 => Ok(ClearFeatureToInterface),
            18 => Ok(ClearFeatureToEndpoint),
            19 => Ok(GetStatusFromDevice),
            20 => Ok(GetStatusFromInterface),
            21 => Ok(GetStatusFromEndpoint),
            23 => Ok(VendorDevice),
            24 => Ok(VendorInterface),
            25 => Ok(VendorEndpoint),
            26 => Ok(ClassDevice),
            27 => Ok(ClassInterface),
            28 => Ok(ClassEndpoint),
            30 => Ok(SyncResetPipeAndClearStall),
            31 => Ok(ClassOther),
            32 => Ok(VendorOther),
            33 => Ok(GetStatusFromOther),
            34 => Ok(ClearFeatureToOther),
            35 => Ok(SetFeatureToOther),
            36 => Ok(GetDescriptorFromEndpoint),
            37 => Ok(SetDescriptorToEndpoint),
            38 => Ok(GetConfiguration),
            39 => Ok(GetInterface),
            40 => Ok(GetDescriptorFromInterface),
            41 => Ok(SetDescriptorToInterface),
            42 => Ok(GetMsFeatureDescriptor),
            48 => Ok(SyncResetPipe),
            49 => Ok(SyncClearStall),
            50 => Ok(ControlTransferEx),
            54 => Ok(CloseStaticStreams),
            55 => Ok(BulkOrInterruptTransferUsingChainedMdl),
            56 => Ok(IsochTransferUsingChainedMdl),
            _ => Err(unsupported_value_err!("UrbFunction", format!("unsupported value: {value}"))),
        }
    }
}

/// Header for every `TS_URB_*` structure, analogous to [`SharedMsgHeader`] for top-level
/// URBDRC messages.
///
/// * [MS-RDPEUSB § 2.2.9.1 TS_URB_HEADER][1]
///
/// [`SharedMsgHeader`]: crate::pdu::header::SharedMsgHeader
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/0b3e5aba-ac93-4b00-b4a2-ce02997e9843
#[doc(alias = "TS_URB_HEADER")]
#[derive(Debug)]
pub struct TsUrbHeader {
    /// Total size in bytes of the enclosing `TS_URB_*` structure.
    pub size: u16,
    /// The function code identifying the `TS_URB_*` structure this header belongs to.
    pub urb_function: UrbFunction,
    /// Unique identifier for a `TRANSFER_IN_REQUEST` or `TRANSFER_OUT_REQUEST` message.
    pub request_id: RequestIdTsUrb,
    /// When `true`, the client must not send a Request Completion message in response.
    ///
    /// Can only be `true` for `TRANSFER_OUT_REQUEST` messages when the device announced
    /// a non-zero `NoAckIsochWriteJitterBufferSizeInMs` in its [`UsbDeviceCaps`].
    ///
    /// [`UsbDeviceCaps`]: crate::pdu::dev_sink::UsbDeviceCaps
    pub no_ack: bool,
}

impl TsUrbHeader {
    const FIXED_PART_SIZE: usize =
        size_of::<u16>() + size_of::<u16>() /* UrbFunction */ + size_of::<u32>() /* RequestId + NoAck bit */;
}

impl Encode for TsUrbHeader {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);
        dst.write_u16(self.size);
        #[expect(clippy::as_conversions, reason = "cast repr(u16) enum discriminant to u16")]
        dst.write_u16(self.urb_function as u16);
        let no_ack = u32::from(self.no_ack) << 31;
        let last32 = u32::from(self.request_id) | no_ack;
        dst.write_u32(last32);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "TS_URB_HEADER"
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

impl Decode<'_> for TsUrbHeader {
    fn decode(src: &mut ReadCursor<'_>) -> DecodeResult<Self> {
        ensure_fixed_part_size!(in: src);
        let size = src.read_u16();
        let urb_function = UrbFunction::try_from(src.read_u16())?;
        let last32 = src.read_u32();
        let request_id = RequestIdTsUrb::from(last32);
        let no_ack = (last32 >> 31) != 0;
        Ok(Self {
            size,
            urb_function,
            request_id,
            no_ack,
        })
    }
}
