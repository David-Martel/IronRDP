//! `TS_URB_*` structures for USB Request Blocks transported over the URBDRC channel.
//!
//! Each `TS_URB_*` structure begins with a [`TsUrbHeader`][header::TsUrbHeader].
//!
//! * [MS-RDPEUSB § 2.2.9 TS_URB Structures][1]
//!
//! [1]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeusb/0b3e5aba-ac93-4b00-b4a2-ce02997e9843

pub mod header;
