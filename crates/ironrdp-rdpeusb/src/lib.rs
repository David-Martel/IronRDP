//! [MS-RDPEUSB]: Remote Desktop Protocol: USB Devices Virtual Channel Extension
//!
//! This crate provides PDU definitions for the URBDRC (USB Redirection Virtual Channel)
//! protocol as specified in MS-RDPEUSB.
//!
//! [MS-RDPEUSB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/a1004d0e-99e9-4968-894b-0b924ef2f125

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod pdu;

/// The static virtual channel name for URBDRC as defined in MS-RDPEUSB.
///
/// [MS-RDPEUSB § 1.3]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/63fe6561-6b48-4e94-b1ef-ed33d6d42c1a
pub const CHANNEL_NAME: &str = "URBDRC";
