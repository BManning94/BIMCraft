//! What the updater's tests stand in for the network and the disk with, and the desktop's tests too:
//! the crate's `testing` feature turns this on for them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;

use crate::{Net, Target};

/// A network that answers from a table of addresses, and remembers what it was asked.
#[derive(Default)]
pub struct Fake {
    answers: Vec<(String, Result<Vec<u8>, String>)>,
    asked: Mutex<Vec<String>>,
}

impl Fake {
    /// Answers `url` with `body`.
    pub fn answer(mut self, url: &str, body: impl Into<Vec<u8>>) -> Fake {
        self.answers.push((url.to_string(), Ok(body.into())));
        self
    }

    /// Answers `url` with a failure, as a dropped connection or a 404 does.
    pub fn fail(mut self, url: &str, error: &str) -> Fake {
        self.answers.push((url.to_string(), Err(error.to_string())));
        self
    }

    /// The addresses asked for so far, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Net for Fake {
    /// Writes the whole answer in pieces, however long the limit: the caller's own cap is what's under test.
    fn get(&self, url: &str, _limit: u64, out: &mut dyn Write) -> Result<(), String> {
        self.asked.lock().unwrap().push(url.to_string());
        match self.answers.iter().find(|(u, _)| u == url) {
            Some((_, Ok(body))) => body.chunks(16 << 10).try_for_each(|piece| out.write_all(piece)).map_err(|e| e.to_string()),
            Some((_, Err(e))) => Err(e.clone()),
            None => Err(format!("404 for {url}")),
        }
    }
}

/// A network that sends the start of an answer and then drops the connection.
pub struct Cut(pub Vec<u8>);

impl Net for Cut {
    fn get(&self, _: &str, _: u64, out: &mut dyn Write) -> Result<(), String> {
        out.write_all(&self.0).map_err(|e| e.to_string())?;
        Err("connection reset".into())
    }
}

/// A folder of its own under the system's temp folder, which goes when this does.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let dir =
            std::env::temp_dir().join(format!("bimcraft-test-{name}-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `bytes` as the `digest` field GitHub gives an asset.
pub fn digest_field(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    format!("sha256:{}", digest.as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// What `releases/latest` says of release `tag` (`v0.5.0`), whose installer is `installer`, when it has one.
pub fn release_json(tag: &str, installer: Option<&[u8]>) -> String {
    let version = tag.trim_start_matches('v');
    let assets: Vec<_> = installer
        .map(|bytes| {
            json!({
                "name": Target::Windows.asset_name(version),
                "size": bytes.len(),
                "digest": digest_field(bytes),
                "state": "uploaded",
                "browser_download_url": format!("https://github.com/IPedrax/BIMCraft/releases/download/{tag}/{}", Target::Windows.asset_name(version)),
            })
        })
        .into_iter()
        .collect();
    json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": assets}).to_string()
}
