use anyhow::Result;

pub(super) fn remove_unused_raw_keyboard() -> Result<()> {
    Ok(())
}

pub(super) fn message(_pointer: *const std::ffi::c_void) -> bool {
    false
}

pub(super) struct KeyboardHook;

impl KeyboardHook {
    pub fn install() -> Result<Self> {
        Ok(Self)
    }
}
