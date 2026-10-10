//! Desktop notifications through the freedesktop `notify-send` helper.
//! Toast buttons/activation URIs are Windows-specific and are not offered.
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, mpsc};

type Action = crate::application::app::notifications::Action;
fn activation_sink() -> &'static Mutex<Option<mpsc::Sender<Action>>> {
    static SINK: OnceLock<Mutex<Option<mpsc::Sender<Action>>>> = OnceLock::new();
    SINK.get_or_init(Mutex::default)
}
pub(crate) fn set_activation_sink(sender: Option<mpsc::Sender<Action>>) {
    *activation_sink().lock().unwrap_or_else(|p| p.into_inner()) = sender;
}
pub(crate) fn receive_activation(uri: &str) -> bool {
    let Ok(action) = Action::parse(uri) else {
        return false;
    };
    activation_sink()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .is_some_and(|s| s.send(action).is_ok())
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ToastAction {
    pub label: String,
    pub uri: String,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Toast {
    pub progress: Option<u16>,
    pub key: String,
    pub title: String,
    pub body: String,
    pub expires_at: i64,
    pub buttons: Vec<ToastAction>,
}
pub(crate) struct Native {
    shown: HashMap<String, (String, String)>,
}
impl Native {
    pub fn new() -> Result<Self> {
        Ok(Self {
            shown: HashMap::new(),
        })
    }
    pub fn synchronize(&mut self, items: &[Toast]) -> Result<()> {
        self.shown
            .retain(|key, _| items.iter().any(|item| &item.key == key));
        for item in items {
            let content = (item.title.clone(), item.body.clone());
            if self.shown.get(&item.key) == Some(&content) {
                continue;
            }
            // Progress-only updates do not re-notify.
            let result = std::process::Command::new("notify-send")
                .args(["--app-name=OpenUUYC", "--icon=openuuyc", &item.title, &item.body])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Err(error) = result {
                tracing::debug!(%error, "notify-send unavailable");
            }
            self.shown.insert(item.key.clone(), content);
        }
        Ok(())
    }
    pub fn clear(&mut self) -> Result<()> {
        self.shown.clear();
        Ok(())
    }
}
