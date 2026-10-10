//! Host-role signaling on Linux. Linux is a viewer only: the device keeps its
//! room presence, but never advertises or accepts remote control, so every
//! host signaling event is ignored.
use super::*;

pub(crate) enum Event {}

pub(crate) struct Session;

impl Session {
    pub(crate) fn new(_client: crate::session::host_client::HostClient) -> Self {
        Self
    }
    pub(crate) async fn next(&mut self) -> Event {
        std::future::pending().await
    }
    pub(crate) async fn apply(&mut self, event: Event, _signal: &mut SignalSession) -> Result<()> {
        match event {}
    }
    pub(crate) async fn event(
        &mut self,
        event: &str,
        _args: &[Value],
        _binary: &[Vec<u8>],
        _signal: &mut SignalSession,
    ) -> Result<()> {
        tracing::debug!(event, "host signaling event ignored on Linux");
        Ok(())
    }
    pub(crate) async fn signaling_restored(&mut self, _signal: &mut SignalSession) -> Result<()> {
        Ok(())
    }
    pub(crate) async fn close(self) {}
}
