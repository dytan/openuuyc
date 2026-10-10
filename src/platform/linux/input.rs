//! Input helpers. Linux has no injected-input marker; the deadline timer is a
//! portable thread-backed replacement for the Windows threadpool timer.
pub(crate) mod system {
    pub(crate) const INPUT_MARKER: usize = 0x4f55_5549;
    pub(crate) fn own_message() -> bool {
        false
    }
}

pub(crate) mod deadline {
    //! On-demand wakeup at a deadline for the remote-input pacing loop.
    use anyhow::Result;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Instant;
    use tokio::sync::Notify;

    struct Shared {
        deadline: Mutex<(Option<Instant>, bool)>,
        changed: Condvar,
    }
    pub(crate) struct Timer {
        shared: Arc<Shared>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Timer {
        pub fn new(wake: Arc<Notify>) -> Result<Self> {
            let shared = Arc::new(Shared {
                deadline: Mutex::new((None, false)),
                changed: Condvar::new(),
            });
            let worker = shared.clone();
            let thread = std::thread::Builder::new()
                .name("UU input deadline".into())
                .spawn(move || {
                    let mut state = worker.deadline.lock().unwrap_or_else(|e| e.into_inner());
                    loop {
                        if state.1 {
                            return;
                        }
                        match state.0 {
                            None => {
                                state = worker.changed.wait(state).unwrap_or_else(|e| e.into_inner());
                            }
                            Some(deadline) => {
                                let now = Instant::now();
                                if now >= deadline {
                                    state.0 = None;
                                    wake.notify_one();
                                } else {
                                    state = worker
                                        .changed
                                        .wait_timeout(state, deadline - now)
                                        .unwrap_or_else(|e| e.into_inner())
                                        .0;
                                }
                            }
                        }
                    }
                })?;
            Ok(Self {
                shared,
                thread: Some(thread),
            })
        }
        pub fn arm(&mut self, deadline: Instant) -> Result<()> {
            let mut state = self.shared.deadline.lock().unwrap_or_else(|e| e.into_inner());
            if state.0 != Some(deadline) {
                state.0 = Some(deadline);
                self.shared.changed.notify_all();
            }
            Ok(())
        }
        pub fn disarm(&mut self) {
            let mut state = self.shared.deadline.lock().unwrap_or_else(|e| e.into_inner());
            state.0 = None;
            self.shared.changed.notify_all();
        }
    }
    impl Drop for Timer {
        fn drop(&mut self) {
            {
                let mut state = self.shared.deadline.lock().unwrap_or_else(|e| e.into_inner());
                state.1 = true;
                self.shared.changed.notify_all();
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
