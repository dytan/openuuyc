//! OpenUUYC's negotiated native drag (OLE drag source/target on both ends) is
//! Windows-only. On Linux the channel stays inert: peers see no native drag
//! capability, and dropping files onto the viewer goes through the official
//! clipboard file drop (`StreamControlHandle::drop_files`) instead.
pub(crate) mod controller {
    use anyhow::Result;
    use std::sync::Arc;
    use webrtc::data_channel::RTCDataChannel;

    #[derive(Clone, Default)]
    pub(crate) struct Controller;

    impl Controller {
        pub fn bind(&self, _channel: &Arc<RTCDataChannel>) {}
        pub fn enable(&self, _enabled: bool) {}
        pub fn close(&self, _channel: &Arc<RTCDataChannel>) {}
        /// Native drag packets are ignored; the peer never sees our capability.
        pub fn receive(&self, _channel: &Arc<RTCDataChannel>, _bytes: &[u8], _bulk: bool) -> Result<()> {
            Ok(())
        }
        pub fn handoff_pending(&self) -> bool {
            false
        }
        pub fn interactive(&self) -> bool {
            false
        }
        pub fn cancel_gesture(&self, _preparing_only: bool) {}
        pub fn is_native(&self) -> bool {
            false
        }
        pub fn available(&self) -> bool {
            false
        }
    }
}
