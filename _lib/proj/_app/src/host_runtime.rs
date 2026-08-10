use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::atomic_file;
use crate::context::EntryContext;
use crate::entry::EntryIdentity;

pub const HOST_RUNTIME_PROTOCOL: &str = "swawkit.host-runtime/v1";
pub const HOST_BOOT_HEADER: &str = "x-swawkit-host-boot";
pub const HOST_ENTRY_HEADER: &str = "x-swawkit-host-entry";

static NEXT_BOOT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostRuntimeDocument {
    pub protocol: String,
    pub entry_key_sha256: String,
    pub boot_id: String,
    pub pid: u32,
    pub url: String,
}

impl HostRuntimeDocument {
    pub fn new(
        entry_key_sha256: impl Into<String>,
        boot_id: impl Into<String>,
        pid: u32,
        url: impl Into<String>,
    ) -> io::Result<Self> {
        let document = Self {
            protocol: HOST_RUNTIME_PROTOCOL.to_owned(),
            entry_key_sha256: entry_key_sha256.into(),
            boot_id: boot_id.into(),
            pid,
            url: url.into(),
        };
        document.validate(&document.entry_key_sha256)?;
        Ok(document)
    }

    pub fn authority(&self) -> io::Result<String> {
        parse_loopback_url(&self.url).map(|address| address.to_string())
    }

    pub fn identity(&self) -> HostRuntimeIdentity {
        HostRuntimeIdentity {
            entry_key_sha256: self.entry_key_sha256.clone(),
            boot_id: self.boot_id.clone(),
            pid: self.pid,
        }
    }

    fn validate(&self, expected_entry: &str) -> io::Result<()> {
        if self.protocol != HOST_RUNTIME_PROTOCOL {
            return Err(invalid_data("Host runtime protocol is unsupported"));
        }
        if self.entry_key_sha256 != expected_entry || !is_sha256(expected_entry) {
            return Err(invalid_data("Host runtime Entry identity does not match"));
        }
        if self.boot_id.is_empty() || self.boot_id.len() > 160 {
            return Err(invalid_data("Host runtime boot ID is invalid"));
        }
        if self.pid == 0 {
            return Err(invalid_data("Host runtime PID is invalid"));
        }
        parse_loopback_url(&self.url)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct HostRuntimeLocator {
    path: PathBuf,
    entry_key_sha256: String,
}

impl HostRuntimeLocator {
    pub fn new(context: &EntryContext, identity: &EntryIdentity) -> Self {
        let entry_key_sha256 = entry_key_sha256(identity);
        let path = context
            .swawkit_home
            .join("data")
            .join("proj.swawkit")
            .join("runtime")
            .join("hosts")
            .join(format!("{entry_key_sha256}.json"));
        Self {
            path,
            entry_key_sha256,
        }
    }

    pub fn acquire_owner(&self) -> HostRuntimeOwner {
        HostRuntimeOwner {
            locator: self.clone(),
            identity: HostRuntimeIdentity {
                entry_key_sha256: self.entry_key_sha256.clone(),
                boot_id: unique_boot_id(),
                pid: std::process::id(),
            },
        }
    }

    pub fn read(&self) -> io::Result<HostRuntimeDocument> {
        let metadata = fs::metadata(&self.path)?;
        if metadata.len() > 16 * 1024 {
            return Err(invalid_data("Host runtime document is too large"));
        }
        let document: HostRuntimeDocument = serde_json::from_slice(&fs::read(&self.path)?)
            .map_err(|error| invalid_data(format!("Host runtime document is invalid: {error}")))?;
        document.validate(&self.entry_key_sha256)?;
        Ok(document)
    }

    pub fn wait_for_healthy(&self, timeout: Duration) -> io::Result<HostRuntimeDocument> {
        let deadline = Instant::now() + timeout;
        loop {
            let error = match self.read().and_then(|document| {
                probe(&document)?;
                Ok(document)
            }) {
                Ok(document) => return Ok(document),
                Err(error) => error,
            };
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "the existing Entry Host did not publish a healthy control endpoint: {}",
                        error
                    ),
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct HostRuntimeOwner {
    locator: HostRuntimeLocator,
    identity: HostRuntimeIdentity,
}

#[derive(Debug, Clone)]
pub struct HostRuntimeIdentity {
    entry_key_sha256: String,
    boot_id: String,
    pid: u32,
}

impl HostRuntimeIdentity {
    pub fn document(&self, url: impl Into<String>) -> io::Result<HostRuntimeDocument> {
        HostRuntimeDocument::new(
            self.entry_key_sha256.clone(),
            self.boot_id.clone(),
            self.pid,
            url,
        )
    }
}

impl HostRuntimeOwner {
    pub fn document(&self, url: impl Into<String>) -> io::Result<HostRuntimeDocument> {
        self.identity.document(url)
    }

    pub fn identity(&self) -> HostRuntimeIdentity {
        self.identity.clone()
    }

    pub fn publish(&self, document: &HostRuntimeDocument) -> io::Result<()> {
        if document.entry_key_sha256 != self.locator.entry_key_sha256
            || document.boot_id != self.identity.boot_id
            || document.pid != self.identity.pid
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot publish a Host runtime document owned by another process",
            ));
        }
        document.validate(&self.locator.entry_key_sha256)?;
        let directory = self
            .locator
            .path
            .parent()
            .expect("Host runtime path always has a parent");
        fs::create_dir_all(directory)?;
        let content = serde_json::to_vec_pretty(document)
            .map_err(|error| invalid_data(format!("cannot encode Host runtime: {error}")))?;
        atomic_file::publish(&self.locator.path, &content)
    }

    pub fn locator(&self) -> &HostRuntimeLocator {
        &self.locator
    }
}

impl Drop for HostRuntimeOwner {
    fn drop(&mut self) {
        let should_remove = self.locator.read().is_ok_and(|document| {
            document.boot_id == self.identity.boot_id && document.pid == self.identity.pid
        });
        if should_remove {
            let _ = fs::remove_file(&self.locator.path);
        }
    }
}

pub fn entry_key_sha256(identity: &EntryIdentity) -> String {
    let digest = Sha256::digest(identity.key().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn probe(document: &HostRuntimeDocument) -> io::Result<()> {
    let address = parse_loopback_url(&document.url)?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(350))?;
    stream.set_read_timeout(Some(Duration::from_millis(350)))?;
    stream.set_write_timeout(Some(Duration::from_millis(350)))?;
    write!(
        stream,
        "GET /healthz HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        address
    )?;

    let mut response = String::new();
    stream.take(16 * 1024).read_to_string(&mut response)?;
    let headers = response
        .split_once("\r\n\r\n")
        .map(|(headers, _)| headers)
        .ok_or_else(|| invalid_data("Host health response has no header boundary"))?;
    let mut lines = headers.lines();
    if !lines
        .next()
        .is_some_and(|line| line.starts_with("HTTP/1.1 200 "))
    {
        return Err(invalid_data("Host health response is not HTTP 200"));
    }
    let boot_id = response_header(lines.clone(), HOST_BOOT_HEADER);
    let entry_key = response_header(lines, HOST_ENTRY_HEADER);
    if boot_id.as_deref() != Some(document.boot_id.as_str())
        || entry_key.as_deref() != Some(document.entry_key_sha256.as_str())
    {
        return Err(invalid_data(
            "Host health identity does not match runtime state",
        ));
    }
    Ok(())
}

fn response_header<'a>(lines: impl Iterator<Item = &'a str>, name: &str) -> Option<String> {
    lines
        .filter_map(|line| line.split_once(':'))
        .find_map(|(key, value)| {
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
}

fn parse_loopback_url(url: &str) -> io::Result<SocketAddr> {
    let authority = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|value| value.strip_suffix('/'))
        .filter(|value| {
            !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
        })
        .ok_or_else(|| invalid_data("Host runtime URL must be an exact IPv4 loopback URL"))?;
    let port = authority
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| invalid_data("Host runtime URL port is invalid"))?;
    Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unique_boot_id() -> String {
    let sequence = NEXT_BOOT.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{timestamp}-{sequence}", std::process::id())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests;
