// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Async client for machina-bpfd (used by machina-daemon, machina-agent, machina-cni).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use crate::api::{Request, Response, StreamEvent, DEFAULT_SOCKET, SOCKET_ENV};

#[derive(Debug, Clone)]
pub struct BpfdClient {
    path: PathBuf,
    timeout: Duration,
}

impl Default for BpfdClient {
    fn default() -> Self {
        Self::from_env()
    }
}

impl BpfdClient {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            timeout: Duration::from_secs(15),
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var(SOCKET_ENV).unwrap_or_else(|_| DEFAULT_SOCKET.into()))
    }

    pub fn socket_path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn reachable(&self) -> bool {
        self.path.exists()
    }

    async fn connect(&self) -> Result<UnixStream> {
        tokio::time::timeout(Duration::from_secs(3), UnixStream::connect(&self.path))
            .await
            .map_err(|_| {
                anyhow!(
                    "timed out connecting to machina-bpfd at {}",
                    self.path.display()
                )
            })?
            .with_context(|| {
                format!(
                    "machina-bpfd not reachable at {} (is machina-bpfd.service running?)",
                    self.path.display()
                )
            })
    }

    /// Send one request; returns `data` on success or the daemon's error.
    pub async fn call(&self, req: &Request) -> Result<Value> {
        let fut = async {
            let mut s = self.connect().await?;
            let mut line = serde_json::to_vec(req)?;
            line.push(b'\n');
            s.write_all(&line).await?;
            let mut reader = BufReader::new(s);
            let mut buf = String::new();
            reader.read_line(&mut buf).await?;
            if buf.is_empty() {
                return Err(anyhow!("machina-bpfd closed the connection"));
            }
            let resp: Response =
                serde_json::from_str(&buf).context("invalid machina-bpfd response")?;
            if resp.ok {
                Ok(resp.data)
            } else {
                Err(anyhow!(resp
                    .error
                    .unwrap_or_else(|| "machina-bpfd error".into())))
            }
        };
        tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| anyhow!("machina-bpfd request timed out"))?
    }

    pub async fn call_as<T: DeserializeOwned>(&self, req: &Request) -> Result<T> {
        Ok(serde_json::from_value(self.call(req).await?)?)
    }

    /// Subscribe to event topics; events arrive on the returned channel until
    /// it is dropped or the daemon disconnects.
    pub async fn subscribe(&self, topics: &[&str]) -> Result<mpsc::Receiver<StreamEvent>> {
        let mut s = self.connect().await?;
        let req = Request::Subscribe {
            topics: topics.iter().map(|t| t.to_string()).collect(),
        };
        let mut line = serde_json::to_vec(&req)?;
        line.push(b'\n');
        s.write_all(&line).await?;
        let (tx, rx) = mpsc::channel(1024);
        tokio::spawn(async move {
            let mut lines = BufReader::new(s).lines();
            // First line is the subscribe acknowledgement.
            let _ = lines.next_line().await;
            while let Ok(Some(l)) = lines.next_line().await {
                let Ok(ev) = serde_json::from_str::<StreamEvent>(&l) else {
                    continue;
                };
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
        });
        Ok(rx)
    }

    /// Hand a bound AF_XDP socket to bpfd for (iface, queue) and open that
    /// queue's gate. bpfd keeps no fd: closing `xsk` unregisters it.
    pub fn register_xsk(
        &self,
        iface: &str,
        queue: u32,
        xsk: std::os::fd::BorrowedFd<'_>,
    ) -> Result<Value> {
        use std::io::{BufRead, BufReader};
        use std::os::fd::AsRawFd;
        let s = std::os::unix::net::UnixStream::connect(&self.path)
            .with_context(|| format!("connect {}", self.path.display()))?;
        s.set_read_timeout(Some(self.timeout))?;
        let mut line = serde_json::to_vec(&Request::AfxdpRegister {
            iface: iface.into(),
            queue,
        })?;
        line.push(b'\n');
        let fd = xsk.as_raw_fd();
        let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as u32) } as usize;
        let mut cbuf = vec![0u64; space.div_ceil(8)];
        let mut iov = libc::iovec {
            iov_base: line.as_mut_ptr().cast(),
            iov_len: line.len(),
        };
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cbuf.as_mut_ptr().cast();
        msg.msg_controllen = space as _;
        unsafe {
            let c = libc::CMSG_FIRSTHDR(&msg);
            (*c).cmsg_level = libc::SOL_SOCKET;
            (*c).cmsg_type = libc::SCM_RIGHTS;
            (*c).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<libc::c_int>() as u32) as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(c) as *mut libc::c_int, fd);
        }
        if unsafe { libc::sendmsg(s.as_raw_fd(), &msg, 0) } < 0 {
            return Err(std::io::Error::last_os_error()).context("sendmsg to machina-bpfd");
        }
        let mut buf = String::new();
        BufReader::new(&s).read_line(&mut buf)?;
        let resp: Response = serde_json::from_str(&buf).context("invalid machina-bpfd response")?;
        if resp.ok {
            Ok(resp.data)
        } else {
            Err(anyhow!(resp
                .error
                .unwrap_or_else(|| "machina-bpfd error".into())))
        }
    }
}
