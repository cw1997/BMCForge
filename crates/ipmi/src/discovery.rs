use anyhow::{Result, ensure};
use bmcforge_core::config::Settings;
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{net::UdpSocket, sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeStatus {
    IpmiCandidate,
    NoResponse,
    InvalidResponse,
    NetworkError,
}
#[derive(Clone, Debug)]
pub struct DiscoveryResult {
    pub host: String,
    pub status: ProbeStatus,
}

pub fn ping(tag: u8) -> Vec<u8> {
    vec![6, 0, 255, 6, 0, 0, 0x11, 0xbe, 0x80, tag, 0, 0]
}
pub fn valid_pong(data: &[u8], tag: u8) -> bool {
    data.len() >= 28
        && data[..9] == [6, 0, 255, 6, 0, 0, 0x11, 0xbe, 0x40]
        && data[9] == tag
        && data[11] == 16
        && data[20] & 0x80 != 0
}
pub async fn probe(host: Ipv4Addr, port: u16, timeout: Duration, retries: u32) -> ProbeStatus {
    let run = async {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(SocketAddr::from((host, port))).await?;
        let tag = socket.local_addr()?.port() as u8;
        for _ in 0..=retries {
            socket.send(&ping(tag)).await?;
            let mut buf = [0u8; 256];
            match tokio::time::timeout(timeout, socket.recv(&mut buf)).await {
                Ok(Ok(size)) => {
                    return Ok::<_, std::io::Error>(if valid_pong(&buf[..size], tag) {
                        ProbeStatus::IpmiCandidate
                    } else {
                        ProbeStatus::InvalidResponse
                    });
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => {}
            }
        }
        Ok(ProbeStatus::NoResponse)
    };
    run.await.unwrap_or(ProbeStatus::NetworkError)
}

pub async fn scan(
    hosts: Vec<String>,
    port: u16,
    settings: Settings,
    cancel: CancellationToken,
    results: mpsc::Sender<DiscoveryResult>,
    demo: bool,
) -> Result<()> {
    ensure!(
        settings.scan_concurrency > 0 && settings.scan_rate_per_second > 0,
        "invalid scan settings"
    );
    let mut pending = hosts.into_iter();
    let mut jobs = JoinSet::new();
    let spacing = Duration::from_secs_f64(1.0 / f64::from(settings.scan_rate_per_second))
        .max(Duration::from_nanos(1));
    let mut ticker = tokio::time::interval(spacing);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut exhausted = false;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => { jobs.abort_all(); return Ok(()); }
            Some(result) = jobs.join_next(), if !jobs.is_empty() => {
                let result = result?;
                tokio::select! { _ = cancel.cancelled() => return Ok(()), sent = results.send(result) => { if sent.is_err() { return Ok(()); } } }
            }
            _ = ticker.tick(), if !exhausted && jobs.len() < settings.scan_concurrency => {
                if let Some(host) = pending.next() {
                    let ip: Ipv4Addr = host.parse()?;
                    let timeout = Duration::from_millis(settings.scan_timeout_ms);
                    let retries = settings.scan_retries;
                    jobs.spawn(async move {
                        let status = if demo { ProbeStatus::IpmiCandidate } else { probe(ip, port, timeout, retries).await };
                        DiscoveryResult { host, status }
                    });
                } else { exhausted = true; }
            }
        }
        if exhausted && jobs.is_empty() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn udp_discovery_checks_tags_and_ipmi_bit() -> Result<()> {
        let socket = UdpSocket::bind("127.0.0.1:0").await?;
        let port = socket.local_addr()?.port();
        let server = tokio::spawn(async move {
            let mut buf = [0u8; 128];
            let (_, peer) = socket.recv_from(&mut buf).await.unwrap();
            let mut reply = vec![0u8; 28];
            reply[..12].copy_from_slice(&ping(buf[9]));
            reply[8] = 0x40;
            reply[11] = 16;
            reply[20] = 128;
            assert!(!valid_pong(&reply, buf[9].wrapping_add(1)));
            socket.send_to(&reply, peer).await.unwrap();
        });
        assert_eq!(
            probe(Ipv4Addr::LOCALHOST, port, Duration::from_millis(100), 0).await,
            ProbeStatus::IpmiCandidate
        );
        server.await?;
        Ok(())
    }
    #[tokio::test]
    async fn cancellation_does_not_wait_for_scan_timeouts() -> Result<()> {
        let cancel = CancellationToken::new();
        let (tx, _rx) = mpsc::channel(1);
        cancel.cancel();
        tokio::time::timeout(
            Duration::from_millis(100),
            scan(
                vec!["192.0.2.1".into(); 1000],
                623,
                Settings::default(),
                cancel,
                tx,
                false,
            ),
        )
        .await??;
        Ok(())
    }
    #[tokio::test]
    async fn silence_is_unknown() -> Result<()> {
        let silent = UdpSocket::bind("127.0.0.1:0").await?;
        assert_eq!(
            probe(
                Ipv4Addr::LOCALHOST,
                silent.local_addr()?.port(),
                Duration::from_millis(10),
                0
            )
            .await,
            ProbeStatus::NoResponse
        );
        Ok(())
    }
}
