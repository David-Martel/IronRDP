//! Shared utility types and macros for MS-RDPEUSB PDUs.

/// An integer value that indicates the result or status of an operation.
///
/// * [MS-ERREF § 2.1 HRESULT][1]
///
/// [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/0642cb2f-2075-4469-918c-4441e69c548a
pub type HResult = u32;

/// The [`CancelRequest::request_id`] field.
///
/// Represents the ID of a request previously sent via IO_CONTROL, INTERNAL_IO_CONTROL,
/// TRANSFER_IN_REQUEST, or TRANSFER_OUT_REQUEST message. Think of this like an "umbrella" type
/// for [`RequestIdIoctl`] and [`RequestIdTsUrb`].
///
/// [`CancelRequest::request_id`]: crate::pdu::usb_dev::CancelRequest::request_id
pub type RequestId = u32;

/// Represents a request ID that uniquely identifies an `IO_CONTROL` or `INTERNAL_IO_CONTROL`
/// message.
pub type RequestIdIoctl = u32;

/// Represents a request ID that uniquely identifies a `TRANSFER_IN_REQUEST` or
/// `TRANSFER_OUT_REQUEST` message. Uses only 31 bits.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct RequestIdTsUrb(u32);

impl From<u32> for RequestIdTsUrb {
    /// Constructs a request ID for `TRANSFER_IN_REQUEST` or `TRANSFER_OUT_REQUEST`.
    ///
    /// Discards the highest bit so the value fits in 31 bits.
    fn from(value: u32) -> Self {
        Self(value & 0x7F_FF_FF_FF)
    }
}

impl From<RequestIdTsUrb> for u32 {
    fn from(value: RequestIdTsUrb) -> Self {
        value.0
    }
}

/// Ensures that a buffer has at least the `PAYLOAD_SIZE` of the current struct.
///
/// This macro is a specialised version of [`ironrdp_core::ensure_size`] that uses the
/// `PAYLOAD_SIZE` constant of the enclosing `impl` block.
///
/// # Examples
///
/// ```
/// use ironrdp_rdpeusb::ensure_payload_size;
///
/// struct MyStruct;
///
/// impl MyStruct {
///     const PAYLOAD_SIZE: usize = 20;
///
///     fn parse(src: &mut ironrdp_core::ReadCursor<'_>) -> ironrdp_core::DecodeResult<Self> {
///         ensure_payload_size!(in: src);
///         // ... parsing logic
///         Ok(MyStruct)
///     }
/// }
/// ```
///
/// # Note
///
/// This macro requires that the current `impl` block defines a `PAYLOAD_SIZE: usize` constant.
#[macro_export]
macro_rules! ensure_payload_size {
    (in: $buf:ident) => {{
        ironrdp_core::ensure_size!(ctx: ironrdp_core::function!(), in: $buf, size: Self::PAYLOAD_SIZE)
    }};
}
