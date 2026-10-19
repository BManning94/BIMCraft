//! The desktop's network: HTTPS to this repository's releases on GitHub and the CDN they are served from, through ureq with
//! rustls (Mozilla's root certificates built in), and nowhere else. A request goes only to an address [`hop_allowed`] says yes
//! to, and a redirect is followed by hand, each hop under the same rule, so no reply can send the updater somewhere else.

use std::io::{self, Write};
use std::time::Duration;

use ureq::Agent;
use ureq::http::Uri;

use crate::{MAX_JSON, Net};

/// A release asset is one redirect from github.com to the CDN; a few more are allowed before giving up.
const REDIRECTS: usize = 4;

/// A reply as small as the API's takes seconds; a larger one is the installer, which on a slow line takes minutes.
const QUICK: Duration = Duration::from_secs(15);
const SLOW: Duration = Duration::from_secs(10 * 60);

pub struct Ureq {
    agent: Agent,
    /// In a test, the one address a request may go to (a server on this machine); none: what [`hop_allowed`] allows.
    only: Option<String>,
}

impl Default for Ureq {
    fn default() -> Self {
        Ureq::new()
    }
}

/// An agent that follows no redirect itself (that's done by hand, under [`hop_allowed`]) and gives up on a stalled connection.
fn agent(https_only: bool) -> Agent {
    Agent::config_builder()
        .https_only(https_only)
        .max_redirects(0)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .user_agent(format!("BIMCraft/{} (+https://github.com/IPedrax/BIMCraft)", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

impl Ureq {
    pub fn new() -> Ureq {
        Ureq { agent: agent(true), only: None }
    }

    /// For tests: talks plain HTTP to the server at `base` on this machine, and to nothing else.
    #[cfg(test)]
    pub(crate) fn loopback(base: &str) -> Ureq {
        Ureq { agent: agent(false), only: Some(format!("{base}/")) }
    }

    fn allowed(&self, url: &str) -> bool {
        self.only.as_ref().map_or_else(|| hop_allowed(url), |only| url.starts_with(only))
    }
}

/// Whether a request to `url` is one the updater makes: HTTPS, to a host as named (no port, no credentials in front of it), and
/// where that host is GitHub's own, into this repository's releases (`..` and escapes are no way to leave them). The CDN's
/// addresses are signed and new each time, so any path on its two hosts goes.
pub fn hop_allowed(url: &str) -> bool {
    let Ok(uri) = url.parse::<Uri>() else { return false };
    let (Some(host), path) = (uri.authority(), uri.path().to_ascii_lowercase()) else { return false };
    let repo = crate::REPO.to_ascii_lowercase();
    let ours = |start: String| path.starts_with(&start) && !path.contains("..") && !path.contains('%');
    uri.scheme_str() == Some("https")
        && match host.as_str().to_ascii_lowercase().as_str() {
            "github.com" => ours(format!("/{repo}/releases/download/")),
            "api.github.com" => ours(format!("/repos/{repo}/releases/")),
            "release-assets.githubusercontent.com" | "objects.githubusercontent.com" => true,
            _ => false,
        }
}

/// `url` for a message: its host, not the signed query a CDN adds.
fn host_of(url: &str) -> String {
    url.parse::<Uri>().ok().and_then(|u| u.host().map(str::to_string)).unwrap_or_else(|| "the address".into())
}

impl Net for Ureq {
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<(), String> {
        let budget = if limit <= MAX_JSON { QUICK } else { SLOW };
        let mut url = url.to_string();
        for _ in 0..=REDIRECTS {
            if !self.allowed(&url) {
                return Err(format!("won't fetch from {}", host_of(&url)));
            }
            let host = host_of(&url);
            let response = self
                .agent
                .get(&url)
                .config()
                .timeout_global(Some(budget))
                .build()
                .header("Accept", "application/vnd.github+json, application/octet-stream")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .call()
                .map_err(|e| format!("{host}: {e}"))?;
            if response.status().is_redirection() {
                let to = response
                    .headers()
                    .get("location")
                    .and_then(|l| l.to_str().ok())
                    .ok_or_else(|| format!("{host}: a redirect to nowhere"))?;
                url = to.to_string();
                continue;
            }
            // ureq's limit fails the read after the last byte it allows, so one more than the caller takes: the caller's own
            // cap is the exact one (a body of exactly `limit` bytes is what a download of the listed size is).
            let mut body = response.into_body().into_with_config().limit(limit.saturating_add(1)).reader();
            return io::copy(&mut body, out).map(|_| ()).map_err(|e| format!("{host}: {e}"));
        }
        Err("too many redirects".into())
    }
}
