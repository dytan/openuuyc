//! Remote power actions are a host feature; Linux does not host.
use anyhow::Result;

pub(crate) fn probe() -> Result<()> {
    super::unsupported()
}
pub(crate) fn execute(_action: crate::features::host::power::Action) -> Result<()> {
    super::unsupported()
}
