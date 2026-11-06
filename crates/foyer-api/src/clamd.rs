//! A minimal clamd client: streams one file per connection with the
//! `INSTREAM` command over TCP.
//!
//! clamd refuses streams longer than its `StreamMaxLength`; such files are
//! reported as [`Verdict::TooLarge`] rather than as an error, so a build with
//! one huge asset is still processed and the reviewer sees the gap.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Bytes sent per INSTREAM chunk.
const CHUNK: usize = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    /// Malware found, with the signature name.
    Found(String),
    /// Larger than clamd's stream limit; not scanned.
    TooLarge,
}

/// Scans one file with the clamd at `addr` (`host:port`).
pub async fn scan_file(addr: &str, path: &Path) -> anyhow::Result<Verdict> {
    tokio::time::timeout(TIMEOUT, scan_file_inner(addr, path))
        .await
        .context("clamd timed out")?
}

async fn scan_file_inner(addr: &str, path: &Path) -> anyhow::Result<Verdict> {
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?;
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect to clamd at {addr}"))?;
    stream.write_all(b"zINSTREAM\0").await?;

    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let len = u32::try_from(n).expect("chunk fits in u32");
        // clamd closes the connection once the stream is over its limit; the
        // write then fails, and the reply says why.
        if stream.write_all(&len.to_be_bytes()).await.is_err()
            || stream.write_all(&buf[..n]).await.is_err()
        {
            return read_reply(&mut stream).await;
        }
    }
    let _ = stream.write_all(&0u32.to_be_bytes()).await;
    read_reply(&mut stream).await
}

async fn read_reply(stream: &mut TcpStream) -> anyhow::Result<Verdict> {
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).await?;
    parse_reply(&String::from_utf8_lossy(&reply))
}

fn parse_reply(reply: &str) -> anyhow::Result<Verdict> {
    let reply = reply.trim_end_matches(['\0', '\n']).trim();
    // Replies look like `stream: OK`, `stream: Eicar-Signature FOUND` or
    // `INSTREAM size limit exceeded. ERROR`.
    let body = reply.strip_prefix("stream:").unwrap_or(reply).trim();
    if body == "OK" {
        Ok(Verdict::Clean)
    } else if let Some(signature) = body.strip_suffix("FOUND") {
        Ok(Verdict::Found(signature.trim().to_string()))
    } else if body.contains("size limit exceeded") {
        Ok(Verdict::TooLarge)
    } else {
        anyhow::bail!("clamd: {reply}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_replies() {
        assert_eq!(parse_reply("stream: OK\0").unwrap(), Verdict::Clean);
        assert_eq!(
            parse_reply("stream: Win.Test.EICAR_HDB-1 FOUND\0").unwrap(),
            Verdict::Found("Win.Test.EICAR_HDB-1".into())
        );
        assert_eq!(
            parse_reply("INSTREAM size limit exceeded. ERROR\0").unwrap(),
            Verdict::TooLarge
        );
        assert!(parse_reply("Can't allocate memory ERROR\0").is_err());
    }
}
