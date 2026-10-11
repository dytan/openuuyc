//! Linux input backend: always the local uinput engine. There is no privileged
//! input service (that is the Windows session-0 broker), so `require_service`
//! requests are refused rather than silently downgraded.
use super::{
    engine::{Engine, Geometry},
    wire::Event,
};
use anyhow::{Result, bail};

pub(super) struct Backend {
    engine: Engine,
    healthy: bool,
}

impl Backend {
    pub fn new(
        policy: super::wire::Policy,
        require_service: bool,
        _permitted: impl Fn() -> bool,
    ) -> Result<Self> {
        if require_service {
            bail!("Linux 被控没有特权输入服务");
        }
        Ok(Self {
            engine: Engine::new(false, policy, 0, false)?,
            healthy: true,
        })
    }
    pub fn service(&self) -> bool {
        false
    }
    pub fn healthy(&self) -> bool {
        self.healthy
    }
    pub fn backend(&self) -> &str {
        self.engine.backend()
    }
    pub fn apply(
        &mut self,
        events: Vec<Event>,
        geometry: Geometry,
        permitted: impl Fn() -> bool,
    ) -> Result<()> {
        for event in events {
            if !permitted() {
                break;
            }
            if let Err(error) = self.engine.apply(event, geometry.clone(), &permitted) {
                self.healthy = false;
                return Err(error);
            }
        }
        Ok(())
    }
    pub fn release(&mut self) -> Result<()> {
        self.engine.release()
    }
    pub fn synchronize(&mut self, geometry: Geometry) -> Result<()> {
        self.engine.synchronize(geometry)
    }
    pub fn configure(&mut self, configuration: super::config::Configuration) -> Result<()> {
        self.engine.configure(configuration)
    }
    pub fn mouse_policy(&self) -> super::config::MousePolicy {
        self.engine.mouse_policy()
    }
    pub fn tick(&mut self) -> Result<()> {
        self.engine.tick()
    }
}
