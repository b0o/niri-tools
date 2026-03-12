use std::collections::HashMap;
use std::pin::Pin;

use futures_core::Stream;

use crate::error::Result;
use crate::types::{NiriEvent, OutputInfo, WindowInfo, WorkspaceInfo};

#[async_trait::async_trait]
pub trait NiriClient: Send + Sync {
    async fn run_action(&self, action: &str, args: &[&str]) -> Result<()>;
    async fn get_windows(&self) -> Result<Vec<WindowInfo>>;
    async fn get_workspaces(&self) -> Result<Vec<WorkspaceInfo>>;
    async fn get_outputs(&self) -> Result<HashMap<String, OutputInfo>>;
    async fn get_focused_output(&self) -> Result<String>;
    async fn subscribe_events(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<NiriEvent>> + Send>>>;
}

pub trait Notifier: Send + Sync {
    fn notify_error(&self, title: &str, message: &str);
    fn notify_warning(&self, title: &str, message: &str);
    fn notify_info(&self, title: &str, message: &str);
}
