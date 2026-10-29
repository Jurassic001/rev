//! Shared HTTP client plus a small on-disk cache so repeat lookups don't hammer
//! Howdy / anex / RMP.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Result, bail};
use reqwest::blocking::{Client, Response};

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!(
            "rev/",
            env!("CARGO_PKG_VERSION"),
            " (TAMU course lookup TUI)"
        ))
        .timeout(Duration::from_secs(120))
        .gzip(true)
        .build()?)
}

/// Returns the body of a successful response, or an error that says plainly when
/// the site answered with a Cloudflare bot challenge instead of data.
pub fn body(resp: Response, site: &str) -> Result<String> {
    let status = resp.status();
    let challenged = resp
        .headers()
        .get("cf-mitigated")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"challenge"));
    let text = resp.text()?;
    if challenged
        || (text.contains("<title>Just a moment...</title>") && text.contains("challenge"))
    {
        bail!(
            "{site} sent a Cloudflare bot challenge instead of data (HTTP {})",
            status.as_u16()
        );
    }
    if !status.is_success() {
        bail!("{site} returned HTTP {}", status.as_u16());
    }
    Ok(text)
}

fn cache_path(key: &str) -> Option<PathBuf> {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(dirs::cache_dir()?.join("rev").join(safe))
}

/// Serves `key` from the cache when it is younger than `ttl`, otherwise calls
/// `fetch` and stores the result. `refresh` skips the cache read.
pub fn cached(
    key: &str,
    ttl: Duration,
    refresh: bool,
    fetch: impl FnOnce() -> Result<String>,
) -> Result<String> {
    let path = cache_path(key);
    if !refresh && let Some(p) = &path {
        let fresh = fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < ttl);
        if fresh && let Ok(s) = fs::read_to_string(p) {
            return Ok(s);
        }
    }
    let s = fetch()?;
    if let Some(p) = &path {
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(p, &s);
    }
    Ok(s)
}
