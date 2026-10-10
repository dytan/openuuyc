use std::sync::Arc;

pub(super) struct RawRouter;

impl RawRouter {
    pub fn message(&self, _pointer: *const std::ffi::c_void) -> bool {
        false
    }
}

pub(super) fn router() -> &'static Arc<RawRouter> {
    use std::sync::OnceLock;
    static ROUTER: OnceLock<Arc<RawRouter>> = OnceLock::new();
    ROUTER.get_or_init(|| Arc::new(RawRouter))
}
