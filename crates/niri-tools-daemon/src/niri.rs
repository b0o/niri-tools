use std::collections::HashMap;
use std::pin::Pin;

use futures_util::StreamExt;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use niri_tools_common::error::NiriToolsError;
use niri_tools_common::traits::NiriClient;
use niri_tools_common::types::{NiriEvent, OutputInfo, WindowInfo, WorkspaceInfo};

use crate::events::{parse_niri_event, parse_output_info, parse_window_info, parse_workspace_info};

pub struct RealNiriClient;

#[async_trait::async_trait]
impl NiriClient for RealNiriClient {
    async fn run_action(&self, action: &str, args: &[&str]) -> niri_tools_common::Result<()> {
        let output = Command::new("niri")
            .args(["msg", "action", action])
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NiriToolsError::NiriCommand(format!(
                "niri action {action} failed: {stderr}"
            )));
        }
        Ok(())
    }

    async fn get_windows(&self) -> niri_tools_common::Result<Vec<WindowInfo>> {
        let output = run_niri_msg(&["windows"]).await?;
        let json: serde_json::Value =
            serde_json::from_str(&output).map_err(|e| NiriToolsError::Serialization(e.to_string()))?;

        let arr = json
            .as_array()
            .ok_or_else(|| NiriToolsError::NiriCommand("expected array from niri msg windows".into()))?;

        Ok(arr.iter().filter_map(parse_window_info).collect())
    }

    async fn get_workspaces(&self) -> niri_tools_common::Result<Vec<WorkspaceInfo>> {
        let output = run_niri_msg(&["workspaces"]).await?;
        let json: serde_json::Value =
            serde_json::from_str(&output).map_err(|e| NiriToolsError::Serialization(e.to_string()))?;

        let arr = json.as_array().ok_or_else(|| {
            NiriToolsError::NiriCommand("expected array from niri msg workspaces".into())
        })?;

        Ok(arr.iter().filter_map(parse_workspace_info).collect())
    }

    async fn get_outputs(&self) -> niri_tools_common::Result<HashMap<String, OutputInfo>> {
        let output = run_niri_msg(&["outputs"]).await?;
        let json: serde_json::Value =
            serde_json::from_str(&output).map_err(|e| NiriToolsError::Serialization(e.to_string()))?;

        let map = json.as_object().ok_or_else(|| {
            NiriToolsError::NiriCommand("expected object from niri msg outputs".into())
        })?;

        let mut outputs = HashMap::new();
        for (name, val) in map {
            outputs.insert(name.clone(), parse_output_info(name, val));
        }
        Ok(outputs)
    }

    async fn get_focused_output(&self) -> niri_tools_common::Result<String> {
        let output = run_niri_msg(&["focused-output"]).await?;
        let json: serde_json::Value =
            serde_json::from_str(&output).map_err(|e| NiriToolsError::Serialization(e.to_string()))?;

        json.get("name")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| {
                NiriToolsError::NiriCommand(
                    "missing 'name' field in focused-output response".into(),
                )
            })
    }

    async fn subscribe_events(
        &self,
    ) -> niri_tools_common::Result<
        Pin<Box<dyn futures_core::Stream<Item = niri_tools_common::Result<NiriEvent>> + Send>>,
    > {
        let mut child = Command::new("niri")
            .args(["msg", "-j", "event-stream"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| NiriToolsError::NiriCommand("failed to capture stdout".into()))?;

        let reader = tokio::io::BufReader::new(stdout);
        let lines = tokio_stream::wrappers::LinesStream::new(reader.lines());

        let stream = lines.filter_map(|line_result: Result<String, std::io::Error>| async move {
            match line_result {
                Ok(line) => {
                    if line.trim().is_empty() {
                        return None;
                    }
                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(json) => parse_niri_event(&json)
                            .map(Ok),
                        Err(e) => Some(Err(NiriToolsError::Serialization(e.to_string()))),
                    }
                }
                Err(e) => Some(Err(NiriToolsError::Io(e))),
            }
        });

        Ok(Box::pin(stream))
    }
}

/// Run `niri msg -j <args...>` and return stdout as a string.
async fn run_niri_msg(args: &[&str]) -> niri_tools_common::Result<String> {
    let output = Command::new("niri")
        .args(["msg", "-j"])
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(NiriToolsError::NiriCommand(format!(
            "niri msg {} failed: {stderr}", args.join(" ")
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| NiriToolsError::NiriCommand(format!("invalid UTF-8 in niri output: {e}")))
}
