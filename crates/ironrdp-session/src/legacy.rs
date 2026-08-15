use crate::SessionError;

// FIXME: code should be fixed so that we never need this conversion
// For that, some code from this ironrdp_session::legacy and ironrdp_connector::legacy modules should be moved to ironrdp_pdu itself
impl From<ironrdp_connector::ConnectorErrorKind> for crate::SessionErrorKind {
    fn from(value: ironrdp_connector::ConnectorErrorKind) -> Self {
        // Preserve the underlying decode/encode/reason detail instead of collapsing every
        // connector error to `General`. Collapsing was actively harmful: it discarded the
        // inner `DecodeError` (which is stored in the kind, not the `source` field), so a
        // failed PDU decode surfaced only as an opaque "general error" with no cause. That
        // is what previously masked GRD's Server Redirection PDU behind a misleading error.
        match value {
            ironrdp_connector::ConnectorErrorKind::Decode(e) => crate::SessionErrorKind::Decode(e),
            ironrdp_connector::ConnectorErrorKind::Encode(e) => crate::SessionErrorKind::Encode(e),
            ironrdp_connector::ConnectorErrorKind::Reason(description) => crate::SessionErrorKind::Reason(description),
            ironrdp_connector::ConnectorErrorKind::Custom | ironrdp_connector::ConnectorErrorKind::Credssp(_) => {
                crate::SessionErrorKind::Custom
            }
            _ => crate::SessionErrorKind::General,
        }
    }
}

pub(crate) fn map_error(error: ironrdp_connector::ConnectorError) -> SessionError {
    error.into_other_kind()
}
