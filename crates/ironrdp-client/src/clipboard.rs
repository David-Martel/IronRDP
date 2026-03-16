use ironrdp::cliprdr::backend::{ClipboardMessage, ClipboardMessageProxy};
use tokio::sync::mpsc;
use tracing::{error, trace};

use crate::rdp::RdpInputEvent;

/// Shim for sending and receiving CLIPRDR events as `RdpInputEvent`
#[derive(Clone, Debug)]
pub struct ClientClipboardMessageProxy {
    tx: mpsc::UnboundedSender<RdpInputEvent>,
}

impl ClientClipboardMessageProxy {
    pub fn new(tx: mpsc::UnboundedSender<RdpInputEvent>) -> Self {
        Self { tx }
    }
}

impl ClipboardMessageProxy for ClientClipboardMessageProxy {
    fn send_clipboard_message(&self, message: ClipboardMessage) {
        trace!(
            message_kind = clipboard_message_kind(&message),
            "Forwarding local clipboard event"
        );
        if self.tx.send(RdpInputEvent::Clipboard(message)).is_err() {
            error!("Failed to send os clipboard message, receiver is closed");
        }
    }
}

fn clipboard_message_kind(message: &ClipboardMessage) -> &'static str {
    match message {
        ClipboardMessage::SendInitiateCopy(_) => "SendInitiateCopy",
        ClipboardMessage::SendFormatData(_) => "SendFormatData",
        ClipboardMessage::SendInitiatePaste(_) => "SendInitiatePaste",
        ClipboardMessage::SendLockClipboard { .. } => "SendLockClipboard",
        ClipboardMessage::SendUnlockClipboard { .. } => "SendUnlockClipboard",
        ClipboardMessage::SendFileContentsRequest(_) => "SendFileContentsRequest",
        ClipboardMessage::SendFileContentsResponse(_) => "SendFileContentsResponse",
        ClipboardMessage::Error(_) => "Error",
    }
}
