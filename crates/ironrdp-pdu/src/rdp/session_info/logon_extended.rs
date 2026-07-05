use bitflags::bitflags;
use ironrdp_core::{
    Decode, DecodeResult, Encode, EncodeResult, ReadCursor, WriteCursor, cast_length, ensure_fixed_part_size,
    ensure_size, invalid_field_err, read_padding,
};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive as _;

const LOGON_EX_LENGTH_FIELD_SIZE: usize = 2;
const LOGON_EX_FLAGS_FIELD_SIZE: usize = 4;
const LOGON_EX_PADDING_SIZE: usize = 570;
const LOGON_EX_PADDING_BUFFER: [u8; LOGON_EX_PADDING_SIZE] = [0; LOGON_EX_PADDING_SIZE];

const LOGON_INFO_FIELD_DATA_SIZE: usize = 4;
const AUTO_RECONNECT_VERSION_1: u32 = 0x0000_0001;
const AUTO_RECONNECT_PACKET_SIZE: usize = 28;
/// Same value as [`AUTO_RECONNECT_PACKET_SIZE`] (28), typed as the `u32` written on
/// the wire. Declared as its own literal (rather than a fallible `u32::try_from`) so
/// the wire encode/decode paths are infallible. Both constants MUST stay equal to 28.
const AUTO_RECONNECT_PACKET_SIZE_U32: u32 = 28;
const AUTO_RECONNECT_RANDOM_BITS_SIZE: usize = 16;
const LOGON_ERRORS_INFO_SIZE: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogonInfoExtended {
    pub present_fields_flags: LogonExFlags,
    pub auto_reconnect: Option<ServerAutoReconnect>,
    pub errors_info: Option<LogonErrorsInfo>,
}

impl LogonInfoExtended {
    const NAME: &'static str = "LogonInfoExtended";

    const FIXED_PART_SIZE: usize = LOGON_EX_LENGTH_FIELD_SIZE + LOGON_EX_FLAGS_FIELD_SIZE;

    fn get_internal_size(&self) -> usize {
        let reconnect_size = self.auto_reconnect.as_ref().map(|r| r.size()).unwrap_or(0);

        let errors_size = self.errors_info.as_ref().map(|r| r.size()).unwrap_or(0);

        Self::FIXED_PART_SIZE + reconnect_size + errors_size
    }
}

impl Encode for LogonInfoExtended {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_size!(in: dst, size: self.size());

        dst.write_u16(cast_length!("internalSize", self.get_internal_size())?);
        dst.write_u32(self.present_fields_flags.bits());

        if let Some(ref reconnect) = self.auto_reconnect {
            reconnect.encode(dst)?;
        }
        if let Some(ref errors) = self.errors_info {
            errors.encode(dst)?;
        }

        dst.write_slice(LOGON_EX_PADDING_BUFFER.as_ref());

        Ok(())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn size(&self) -> usize {
        self.get_internal_size() + LOGON_EX_PADDING_SIZE
    }
}

impl<'de> Decode<'de> for LogonInfoExtended {
    fn decode(src: &mut ReadCursor<'de>) -> DecodeResult<Self> {
        ensure_fixed_part_size!(in: src);

        let _self_length = src.read_u16();
        let present_fields_flags = LogonExFlags::from_bits_retain(src.read_u32());

        let auto_reconnect = if present_fields_flags.contains(LogonExFlags::AUTO_RECONNECT_COOKIE) {
            Some(ServerAutoReconnect::decode(src)?)
        } else {
            None
        };

        let errors_info = if present_fields_flags.contains(LogonExFlags::LOGON_ERRORS) {
            Some(LogonErrorsInfo::decode(src)?)
        } else {
            None
        };

        ensure_size!(in: src, size: LOGON_EX_PADDING_SIZE);
        read_padding!(src, LOGON_EX_PADDING_SIZE);

        Ok(Self {
            present_fields_flags,
            auto_reconnect,
            errors_info,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAutoReconnect {
    pub logon_id: u32,
    pub random_bits: [u8; AUTO_RECONNECT_RANDOM_BITS_SIZE],
}

impl ServerAutoReconnect {
    const NAME: &'static str = "ServerAutoReconnect";

    const FIXED_PART_SIZE: usize = AUTO_RECONNECT_PACKET_SIZE + LOGON_INFO_FIELD_DATA_SIZE;
}

impl Encode for ServerAutoReconnect {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);

        dst.write_u32(AUTO_RECONNECT_PACKET_SIZE_U32);
        dst.write_u32(AUTO_RECONNECT_PACKET_SIZE_U32);
        dst.write_u32(AUTO_RECONNECT_VERSION_1);
        dst.write_u32(self.logon_id);
        dst.write_slice(self.random_bits.as_ref());

        Ok(())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

impl<'de> Decode<'de> for ServerAutoReconnect {
    fn decode(src: &mut ReadCursor<'de>) -> DecodeResult<Self> {
        ensure_fixed_part_size!(in: src);

        let _data_length = src.read_u32();
        let packet_length = src.read_u32();
        if packet_length != AUTO_RECONNECT_PACKET_SIZE_U32 {
            return Err(invalid_field_err!("packetLen", "invalid auto-reconnect packet size"));
        }

        let version = src.read_u32();
        if version != AUTO_RECONNECT_VERSION_1 {
            return Err(invalid_field_err!("version", "invalid auto-reconnect version"));
        }

        let logon_id = src.read_u32();
        let random_bits = src.read_array();

        Ok(Self { logon_id, random_bits })
    }
}

/// Size in bytes of the serialized client auto-reconnect cookie
/// (`ARC_CS_PRIVATE_PACKET`): cbLen(4) + Version(4) + LogonId(4) +
/// SecurityVerifier(16).
pub const CLIENT_AUTO_RECONNECT_COOKIE_SIZE: usize = 28;

/// The 32-byte ClientRandom value used to derive the auto-reconnect security
/// verifier under Enhanced RDP Security (TLS / CredSSP / NLA).
///
/// Per [MS-RDPBCGR] 5.5 (Automatic Reconnection), the real client random is only
/// fed into the verifier under **Standard RDP Security** (`PROTOCOL_RDP`), where a
/// client random is exchanged during the security-exchange phase. Under Enhanced
/// RDP Security (TLS/CredSSP/NLA) no client random is exchanged, so the value used
/// for the HMAC is a 32-byte array of zeros. (FreeRDP-based clients follow the same
/// rule: the client-random buffer is zero-initialised and only populated with real
/// bytes when the selected protocol is Standard RDP Security.)
///
/// NOTE: only the zero-client-random (Enhanced Security) path is exercised by the
/// unit tests and against real servers here; the Standard-Security branch is
/// correct-by-construction (`from_server` takes the client random as a parameter)
/// but unproven on the wire.
///
/// [MS-RDPBCGR] 5.5: <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/>
pub const ENHANCED_SECURITY_CLIENT_RANDOM: [u8; 32] = [0; 32];

/// Client Auto-Reconnect Packet (`ARC_CS_PRIVATE_PACKET`).
///
/// Sent by the client in the `autoReconnectCookie` field of the extended Client
/// Info PDU on a reconnect attempt so the server can re-attach the existing
/// session instead of starting a new one.
///
/// The [`security_verifier`](Self::security_verifier) is a cryptographic
/// derivation from the server-issued [`ServerAutoReconnect`] cookie — **not** a
/// copy of its random bits:
///
/// `SecurityVerifier = HMAC-MD5(ArcRandomBits, ClientRandom)`
///
/// See [MS-RDPBCGR] 2.2.4.4 (packet layout) and 5.5 (derivation).
///
/// [MS-RDPBCGR]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/1cf7bcc4-f6d1-4924-a37c-7c9975ba9f47
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientAutoReconnect {
    pub logon_id: u32,
    pub security_verifier: [u8; AUTO_RECONNECT_RANDOM_BITS_SIZE],
}

impl ClientAutoReconnect {
    /// Derives the client auto-reconnect packet from the server's cookie.
    ///
    /// `client_random` is the negotiated client random. Under Enhanced Security
    /// (TLS/CredSSP/NLA) this is [`ENHANCED_SECURITY_CLIENT_RANDOM`] (32 zero
    /// bytes); prefer [`from_server_enhanced_security`](Self::from_server_enhanced_security)
    /// for that common case.
    pub fn from_server(server: &ServerAutoReconnect, client_random: &[u8]) -> Self {
        Self {
            logon_id: server.logon_id,
            security_verifier: hmac_md5(&server.random_bits, client_random),
        }
    }

    /// Derives the packet for an Enhanced-Security (TLS/CredSSP/NLA) connection,
    /// where the client random is 32 zero bytes per [MS-RDPBCGR] 5.5.
    ///
    /// [MS-RDPBCGR]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/1d263f84-6153-4a16-b329-8770be364e1b
    pub fn from_server_enhanced_security(server: &ServerAutoReconnect) -> Self {
        Self::from_server(server, &ENHANCED_SECURITY_CLIENT_RANDOM)
    }

    /// Serializes to the 28-byte `ARC_CS_PRIVATE_PACKET` wire form (all fields
    /// little-endian) for the Client Info PDU's auto-reconnect cookie field.
    pub fn to_bytes(&self) -> [u8; CLIENT_AUTO_RECONNECT_COOKIE_SIZE] {
        let mut out = [0u8; CLIENT_AUTO_RECONNECT_COOKIE_SIZE];
        out[0..4].copy_from_slice(&AUTO_RECONNECT_PACKET_SIZE_U32.to_le_bytes());
        out[4..8].copy_from_slice(&AUTO_RECONNECT_VERSION_1.to_le_bytes());
        out[8..12].copy_from_slice(&self.logon_id.to_le_bytes());
        out[12..28].copy_from_slice(&self.security_verifier);
        out
    }
}

/// Computes `HMAC-MD5(key, msg)` (RFC 2104) using MD5 as the underlying hash.
///
/// Implemented directly over the `md-5` primitive already vendored by this crate
/// to avoid pulling in a mismatched `hmac`/`digest` version. MD5 has a 64-byte
/// block; auto-reconnect keys are 16 bytes so the `key.len() > BLOCK` branch is
/// never taken in practice, but it is kept for correctness.
fn hmac_md5(key: &[u8], msg: &[u8]) -> [u8; 16] {
    use md5::{Digest as _, Md5};

    const BLOCK: usize = 64;

    let mut block_key = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut hasher = Md5::new();
        hasher.update(key);
        let digest = hasher.finalize();
        block_key[..digest.len()].copy_from_slice(&digest);
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for ((ip, op), kb) in ipad.iter_mut().zip(opad.iter_mut()).zip(block_key.iter()) {
        *ip ^= *kb;
        *op ^= *kb;
    }

    let mut inner = Md5::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();

    let mut outer = Md5::new();
    outer.update(opad);
    outer.update(inner_digest);

    let mut result = [0u8; 16];
    result.copy_from_slice(&outer.finalize());
    result
}

/// TS_LOGON_ERRORS_INFO
///
/// [Doc](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/845eb789-6edf-453a-8b0e-c976823d1f72)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogonErrorsInfo {
    pub error_type: LogonErrorNotificationType,
    pub error_data: LogonErrorNotificationData,
}

impl LogonErrorsInfo {
    const NAME: &'static str = "LogonErrorsInfo";

    const FIXED_PART_SIZE: usize = LOGON_ERRORS_INFO_SIZE + LOGON_INFO_FIELD_DATA_SIZE;
}

impl Encode for LogonErrorsInfo {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ensure_fixed_part_size!(in: dst);

        dst.write_u32(u32::try_from(LOGON_ERRORS_INFO_SIZE).expect("LOGON_ERRORS_INFO_SIZE fits into u32"));
        dst.write_u32(self.error_type.as_u32());
        dst.write_u32(self.error_data.to_u32());

        Ok(())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn size(&self) -> usize {
        Self::FIXED_PART_SIZE
    }
}

impl<'de> Decode<'de> for LogonErrorsInfo {
    fn decode(src: &mut ReadCursor<'de>) -> DecodeResult<Self> {
        ensure_fixed_part_size!(in: src);

        let _data_length = src.read_u32();
        let error_type = LogonErrorNotificationType::from_u32(src.read_u32())
            .ok_or_else(|| invalid_field_err!("errorType", "invalid logon error type"))?;

        let error_notification_data = src.read_u32();
        let error_data = LogonErrorNotificationDataErrorCode::from_u32(error_notification_data)
            .map(LogonErrorNotificationData::ErrorCode)
            .unwrap_or(LogonErrorNotificationData::SessionId(error_notification_data));

        Ok(Self { error_type, error_data })
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct LogonExFlags: u32 {
        const AUTO_RECONNECT_COOKIE = 0x0000_0001;
        const LOGON_ERRORS = 0x0000_0002;

        const _ = !0;
    }
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum LogonErrorNotificationType {
    SessionBusyOptions = 0xFFFF_FFF8,
    DisconnectRefused = 0xFFFF_FFF9,
    NoPermission = 0xFFFF_FFFA,
    BumpOptions = 0xFFFF_FFFB,
    ReconnectOptions = 0xFFFF_FFFC,
    SessionTerminate = 0xFFFF_FFFD,
    SessionContinue = 0xFFFF_FFFE,
    AccessDenied = 0xFFFF_FFFF,
}

impl LogonErrorNotificationType {
    #[expect(
        clippy::as_conversions,
        reason = "guarantees discriminant layout, and as is the only way to cast enum -> primitive"
    )]
    fn as_u32(self) -> u32 {
        self as u32
    }
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum LogonErrorNotificationDataErrorCode {
    FailedBadPassword = 0x0000_0000,
    FailedUpdatePassword = 0x0000_0001,
    FailedOther = 0x0000_0002,
    Warning = 0x0000_0003,
}

impl LogonErrorNotificationDataErrorCode {
    #[expect(
        clippy::as_conversions,
        reason = "guarantees discriminant layout, and as is the only way to cast enum -> primitive"
    )]
    fn as_u32(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogonErrorNotificationData {
    ErrorCode(LogonErrorNotificationDataErrorCode),
    SessionId(u32),
}

impl LogonErrorNotificationData {
    pub fn to_u32(&self) -> u32 {
        match self {
            LogonErrorNotificationData::ErrorCode(code) => code.as_u32(),
            LogonErrorNotificationData::SessionId(id) => *id,
        }
    }
}

#[cfg(test)]
mod arc_tests {
    use super::*;

    /// RFC 2202 HMAC-MD5 test case 1 (independent known-answer vector):
    /// key = 0x0b × 16, data = "Hi There" → 0x9294727a3638bb1c13f48ef8158bfc9d.
    #[test]
    fn hmac_md5_matches_rfc2202_vector_1() {
        let key = [0x0bu8; 16];
        let data = b"Hi There";
        let expected = [
            0x92, 0x94, 0x72, 0x7a, 0x36, 0x38, 0xbb, 0x1c, 0x13, 0xf4, 0x8e, 0xf8, 0x15, 0x8b, 0xfc, 0x9d,
        ];
        assert_eq!(hmac_md5(&key, data), expected);
    }

    /// RFC 2202 HMAC-MD5 test case 2 (short key, non-block-aligned):
    /// key = "Jefe", data = "what do ya want for nothing?"
    /// → 0x750c783e6ab0b503eaa86e310a5db738.
    #[test]
    fn hmac_md5_matches_rfc2202_vector_2() {
        let expected = [
            0x75, 0x0c, 0x78, 0x3e, 0x6a, 0xb0, 0xb5, 0x03, 0xea, 0xa8, 0x6e, 0x31, 0x0a, 0x5d, 0xb7, 0x38,
        ];
        assert_eq!(hmac_md5(b"Jefe", b"what do ya want for nothing?"), expected);
    }

    /// Known-answer test for the client auto-reconnect derivation under Enhanced
    /// Security. The expected verifier was computed independently:
    /// `python -c "import hmac,hashlib; print(hmac.new(bytes(range(1,17)), bytes(32), hashlib.md5).hexdigest())"`
    /// → 894025a99d64ab966419ec1ef13c261c.
    #[test]
    fn client_auto_reconnect_verifier_known_answer() {
        let server = ServerAutoReconnect {
            logon_id: 0x0201,
            random_bits: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        };
        let expected_verifier = [
            0x89, 0x40, 0x25, 0xa9, 0x9d, 0x64, 0xab, 0x96, 0x64, 0x19, 0xec, 0x1e, 0xf1, 0x3c, 0x26, 0x1c,
        ];

        let client = ClientAutoReconnect::from_server_enhanced_security(&server);
        assert_eq!(client.logon_id, 0x0201);
        assert_eq!(client.security_verifier, expected_verifier);
    }

    /// The derivation must NOT be a copy of the server random bits (regression
    /// guard against the naive `reconnect_cookie = random_bits` mistake).
    #[test]
    fn client_verifier_is_not_a_copy_of_random_bits() {
        let random_bits = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let server = ServerAutoReconnect {
            logon_id: 7,
            random_bits,
        };
        let client = ClientAutoReconnect::from_server_enhanced_security(&server);
        assert_ne!(client.security_verifier, random_bits);
    }

    /// The 28-byte `ARC_CS_PRIVATE_PACKET` wire layout: cbLen(4)=28, Version(4)=1,
    /// LogonId(4), SecurityVerifier(16), all little-endian.
    #[test]
    fn client_auto_reconnect_wire_layout() {
        let server = ServerAutoReconnect {
            logon_id: 0x0403_0201,
            random_bits: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        };
        let client = ClientAutoReconnect::from_server_enhanced_security(&server);
        let bytes = client.to_bytes();

        assert_eq!(bytes.len(), CLIENT_AUTO_RECONNECT_COOKIE_SIZE);
        assert_eq!(&bytes[0..4], &[0x1c, 0x00, 0x00, 0x00]); // cbLen = 28
        assert_eq!(&bytes[4..8], &[0x01, 0x00, 0x00, 0x00]); // Version = 1
        assert_eq!(&bytes[8..12], &[0x01, 0x02, 0x03, 0x04]); // LogonId (LE)
        assert_eq!(&bytes[12..28], &client.security_verifier);
    }
}
