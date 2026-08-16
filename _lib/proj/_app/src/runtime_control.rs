use std::fs;
use std::io::{self, Read};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ureq::Agent;

use crate::context::EntryContext;
use crate::entry::EntryIdentity;
use crate::host_runtime::{HostRuntimeDocument, HostRuntimeLocator};
use crate::runtime_release::RuntimeReleaseStore;

pub const HOST_STATUS_PROTOCOL: &str = "swawkit.host-status/v1";
pub const RUNTIME_STATUS_PROTOCOL: &str = "swawkit.runtime-status/v1";
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostStatusDocument {
    pub protocol: String,
    pub entry_key_sha256: String,
    pub boot_id: String,
    pub pid: u32,
    pub url: String,
    pub running_release_id: String,
    pub selected_release_id: String,
    pub update_available: bool,
}

impl HostStatusDocument {
    pub fn new(
        runtime: &HostRuntimeDocument,
        running_release_id: impl Into<String>,
        selected_release_id: impl Into<String>,
    ) -> Result<Self, String> {
        let selected_release_id = selected_release_id.into();
        let document = Self {
            protocol: HOST_STATUS_PROTOCOL.to_owned(),
            entry_key_sha256: runtime.entry_key_sha256.clone(),
            boot_id: runtime.boot_id.clone(),
            pid: runtime.pid,
            url: runtime.url.clone(),
            running_release_id: running_release_id.into(),
            update_available: false,
            selected_release_id,
        };
        let mut document = document;
        document.update_available = document.running_release_id != document.selected_release_id;
        document.validate(runtime)?;
        Ok(document)
    }

    pub fn validate(&self, runtime: &HostRuntimeDocument) -> Result<(), String> {
        if self.protocol != HOST_STATUS_PROTOCOL
            || self.entry_key_sha256 != runtime.entry_key_sha256
            || self.boot_id != runtime.boot_id
            || self.pid != runtime.pid
            || self.url != runtime.url
            || !is_sha256(&self.running_release_id)
            || !is_sha256(&self.selected_release_id)
            || self.update_available != (self.running_release_id != self.selected_release_id)
        {
            return Err("Host returned an invalid runtime status document".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeStatusDocument {
    pub protocol: String,
    pub selected_release_id: String,
    pub release_count: usize,
    pub host: Option<HostStatusDocument>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAction {
    Exit,
    Restart,
}

impl HostAction {
    fn path(self) -> &'static str {
        match self {
            Self::Exit => "api/v2/host/shutdown",
            Self::Restart => "api/v2/host/restart",
        }
    }

    fn control_header(self) -> &'static str {
        match self {
            Self::Exit => "shutdown",
            Self::Restart => "restart",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Exit => "exit",
            Self::Restart => "restart",
        }
    }
}

pub fn inspect(context: &EntryContext) -> Result<RuntimeStatusDocument, String> {
    let (selected_release_id, release_count) = inspect_storage(context)?;
    let host = read_live_host(context)?.map(|(_, status)| status);
    Ok(RuntimeStatusDocument {
        protocol: RUNTIME_STATUS_PROTOCOL.to_owned(),
        selected_release_id,
        release_count,
        host,
    })
}

pub fn inspect_running(
    context: &EntryContext,
    runtime: &HostRuntimeDocument,
) -> Result<RuntimeStatusDocument, String> {
    let (selected_release_id, release_count) = inspect_storage(context)?;
    let host = HostStatusDocument::new(
        runtime,
        context.release_id.clone(),
        selected_release_id.clone(),
    )?;
    Ok(RuntimeStatusDocument {
        protocol: RUNTIME_STATUS_PROTOCOL.to_owned(),
        selected_release_id,
        release_count,
        host: Some(host),
    })
}

fn inspect_storage(context: &EntryContext) -> Result<(String, usize), String> {
    let store = RuntimeReleaseStore::open(&context.swawkit_home)
        .map_err(|error| format!("cannot open Runtime Release storage: {error}"))?;
    let selected_release_id = store
        .selected_release_id()
        .map_err(|error| format!("cannot read selected Runtime Release: {error}"))?;
    let release_count = fs::read_dir(store.releases_root())
        .map_err(|error| format!("cannot enumerate Runtime Releases: {error}"))?
        .try_fold(0_usize, |count, entry| {
            entry
                .map(|_| count + 1)
                .map_err(|error| format!("cannot enumerate Runtime Releases: {error}"))
        })?;
    Ok((selected_release_id, release_count))
}

pub fn request_host_action(context: &EntryContext, action: HostAction) -> Result<(), String> {
    let Some((runtime, status)) = read_live_host(context)? else {
        return Err("the current Entry Host is not running".to_owned());
    };
    if action == HostAction::Restart && !status.update_available {
        return Err("the Host already runs the selected Runtime Release".to_owned());
    }
    let url = format!("{}{path}", runtime.url, path = action.path());
    let mut response = host_agent()
        .post(&url)
        .header("X-SwawKit-Control", action.control_header())
        .send_empty()
        .map_err(|error| format!("cannot request Host {}: {error}", action.label()))?;
    let status_code = response.status().as_u16();
    if matches!(status_code, 202 | 204) {
        return Ok(());
    }
    let body = read_body(&mut response).unwrap_or_default();
    let detail = String::from_utf8_lossy(&body).trim().to_owned();
    Err(if detail.is_empty() {
        format!(
            "Host rejected the {} request with HTTP {status_code}",
            action.label()
        )
    } else {
        detail
    })
}

fn read_live_host(
    context: &EntryContext,
) -> Result<Option<(HostRuntimeDocument, HostStatusDocument)>, String> {
    let identity = EntryIdentity::read(&context.entry_file)
        .map_err(|error| format!("cannot read Entry identity: {error}"))?;
    let locator = HostRuntimeLocator::new(context, &identity);
    let runtime = match locator.read() {
        Ok(runtime) => runtime,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read Host runtime state: {error}")),
    };
    let url = format!("{}api/v2/host", runtime.url);
    let mut response = host_agent()
        .get(&url)
        .header("Accept", "application/json")
        .call()
        .map_err(|error| format!("cannot read Host runtime status: {error}"))?;
    let status_code = response.status().as_u16();
    if status_code != 200 {
        return Err(format!("Host runtime status returned HTTP {status_code}"));
    }
    let body = read_body(&mut response)?;
    let status: HostStatusDocument = serde_json::from_slice(&body)
        .map_err(|error| format!("Host returned invalid runtime status JSON: {error}"))?;
    status.validate(&runtime)?;
    Ok(Some((runtime, status)))
}

fn host_agent() -> Agent {
    Agent::config_builder()
        // The Host endpoint is an exact IPv4 loopback address. Routing this
        // private control channel through an inherited proxy is both incorrect
        // and capable of making a healthy local Host appear unavailable.
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(3)))
        .timeout_connect(Some(Duration::from_millis(500)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_host_control_never_inherits_a_proxy() {
        assert!(host_agent().config().proxy().is_none());
    }
}

fn read_body(response: &mut ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| format!("cannot read Host response: {error}"))?;
    if body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("Host response exceeds the 64 KiB safety limit".to_owned());
    }
    Ok(body)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
