use anyhow::Result;
use bmcforge_core::{
    ControlState, DeviceIdentity, Sensor,
    config::{Server, Settings},
};
use bmcforge_ipmi::{
    Device,
    discovery::{self, DiscoveryResult},
};
use bmcforge_profiles::Profile;
use futures_util::{StreamExt, stream};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub identity: DeviceIdentity,
    pub sensors: Vec<Sensor>,
    pub control: ControlState,
    pub taken_over: bool,
    pub status: String,
}
pub enum Message {
    Snapshot(String, Snapshot),
    Log(String),
    Discovered(u64, DiscoveryResult),
    ScanFinished(u64),
    Authenticated(Server, Option<String>),
    Configured(Server, Option<String>),
    Removed(String, Option<String>),
}
pub enum Command {
    Manual(Box<Profile>, u8, u8),
    Automatic(Option<Box<Profile>>),
    Configure(Box<Server>),
    Remove,
    Shutdown,
}
struct Worker {
    tx: mpsc::Sender<Command>,
    task: JoinHandle<()>,
    stop: CancellationToken,
}
pub struct Runtime {
    workers: BTreeMap<String, Worker>,
    pub tx: mpsc::Sender<Message>,
    gate: Arc<Semaphore>,
    pub scan_cancel: CancellationToken,
    jobs: Vec<JoinHandle<()>>,
    demo: bool,
    scan_generation: u64,
    concurrency: usize,
}
impl Runtime {
    pub fn new(tx: mpsc::Sender<Message>, settings: &Settings, demo: bool) -> Self {
        Self {
            workers: BTreeMap::new(),
            tx,
            gate: Arc::new(Semaphore::new(settings.concurrency)),
            scan_cancel: CancellationToken::new(),
            jobs: vec![],
            demo,
            scan_generation: 0,
            concurrency: settings.concurrency,
        }
    }
    pub fn add(&mut self, server: Server) {
        let id = server.id.clone();
        if self.workers.contains_key(&id) {
            return;
        }
        let (tx, mut rx) = mpsc::channel(16);
        let output = self.tx.clone();
        let gate = self.gate.clone();
        let demo = self.demo;
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            let mut device = Device::new(server, demo);
            let mut authentication_blocked = false;
            let mut next_poll = tokio::time::Instant::now();
            loop {
                let command = tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => Some(Command::Shutdown),
                    command = rx.recv() => command,
                    _ = tokio::time::sleep_until(next_poll), if !authentication_blocked => {
                        let _permit = match gate.acquire().await { Ok(p) => p, Err(_) => break };
                        let status = match device.poll().await { Ok(()) => "Online".into(), Err(e) => {
                            authentication_blocked = bmcforge_ipmi::is_authentication_error(&e);
                            if authentication_blocked { format!("Authentication failed; edit credentials to retry: {e:#}") } else { format!("Offline: {e:#}") }
                        } };
                        next_poll = tokio::time::Instant::now() + Duration::from_secs(device.server.poll_seconds);
                        let _ = output.send(Message::Snapshot(device.server.id.clone(), snapshot(&device, &status))).await;
                        continue;
                    }
                };
                let Some(command) = command else {
                    let _ = device.shutdown().await;
                    break;
                };
                let shutting_down = matches!(command, Command::Shutdown);
                let removing = matches!(command, Command::Remove);
                let reconfigure = if let Command::Configure(server) = &command {
                    Some(server.as_ref().clone())
                } else {
                    None
                };
                let _permit = match gate.acquire().await {
                    Ok(p) => p,
                    Err(_) => break,
                };
                let result = match command {
                    Command::Manual(p, percent, zone) => device.manual(&p, percent, zone).await,
                    Command::Automatic(p) => device.automatic(p.as_deref()).await,
                    Command::Configure(server) => match device.shutdown().await {
                        Ok(()) => {
                            device = Device::new(*server, demo);
                            authentication_blocked = false;
                            next_poll = tokio::time::Instant::now();
                            Ok(())
                        }
                        Err(e) => Err(e.context("restore failed; running configuration unchanged")),
                    },
                    Command::Shutdown | Command::Remove => device.shutdown().await,
                };
                if let Some(server) = reconfigure {
                    let _ = output
                        .send(Message::Configured(
                            server,
                            result.as_ref().err().map(|e| format!("{e:#}")),
                        ))
                        .await;
                }
                if removing {
                    let _ = output
                        .send(Message::Removed(
                            device.server.id.clone(),
                            result.as_ref().err().map(|e| format!("{e:#}")),
                        ))
                        .await;
                }
                let status: String = if let Err(error) = &result {
                    format!("Operation failed: {error:#}")
                } else {
                    if device.identity.manufacturer_id == 0 {
                        "Connecting".into()
                    } else {
                        "Online".into()
                    }
                };
                let log = format!(
                    "{}: {}{}",
                    device.server.name,
                    if shutting_down {
                        "shutdown / restore: "
                    } else {
                        "operation: "
                    },
                    result
                        .as_ref()
                        .map(|_| "OK".into())
                        .unwrap_or_else(|e| format!("{e:#}"))
                );
                let _ = output.send(Message::Log(log)).await;
                let _ = output
                    .send(Message::Snapshot(
                        device.server.id.clone(),
                        snapshot(&device, &status),
                    ))
                    .await;
                if shutting_down || (removing && result.is_ok()) {
                    break;
                }
            }
        });
        self.workers.insert(id, Worker { tx, task, stop });
    }
    pub fn send(&self, id: &str, command: Command) -> Result<()> {
        self.workers
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("worker not found"))?
            .tx
            .try_send(command)
            .map_err(|_| anyhow::anyhow!("server command queue is busy"))?;
        Ok(())
    }
    pub fn forget(&mut self, id: &str) {
        self.workers.remove(id);
    }
    pub fn scan(&mut self, hosts: Vec<String>, port: u16, settings: Settings) -> u64 {
        self.scan_cancel.cancel();
        self.scan_cancel = CancellationToken::new();
        let cancel = self.scan_cancel.clone();
        let output = self.tx.clone();
        let demo = self.demo;
        self.scan_generation += 1;
        let generation = self.scan_generation;
        let job = tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(64);
            let scan = discovery::scan(hosts, port, settings, cancel, tx, demo);
            tokio::pin!(scan);
            loop {
                tokio::select! {
                    result = &mut scan => {
                        if let Err(e) = result { let _ = output.send(Message::Log(format!("scan: {e:#}"))).await; }
                        while let Ok(item) = rx.try_recv() { let _ = output.send(Message::Discovered(generation,item)).await; }
                        break;
                    },
                    Some(item) = rx.recv() => { if output.send(Message::Discovered(generation,item)).await.is_err() { break; } }
                }
            }
            let _ = output.send(Message::ScanFinished(generation)).await;
        });
        self.jobs.push(job);
        generation
    }
    pub fn authenticate(&mut self, servers: Vec<Server>) {
        self.jobs.retain(|j| !j.is_finished());
        let gate = self.gate.clone();
        let output = self.tx.clone();
        let demo = self.demo;
        let limit = self.concurrency;
        self.jobs.push(tokio::spawn(async move {
            let mut pending = stream::iter(servers)
                .map(|server| {
                    let gate = gate.clone();
                    let output = output.clone();
                    async move {
                        let Ok(_permit) = gate.acquire().await else {
                            return;
                        };
                        let mut d = Device::new(server.clone(), demo);
                        let result = d.poll().await;
                        let error = result.err().map(|e| format!("{e:#}"));
                        let _ = d.shutdown().await;
                        let _ = output.send(Message::Authenticated(server, error)).await;
                    }
                })
                .buffer_unordered(limit);
            while pending.next().await.is_some() {}
        }));
    }
    pub async fn shutdown(mut self, messages: &mut mpsc::Receiver<Message>) -> Vec<String> {
        self.scan_cancel.cancel();
        for job in self.jobs {
            job.abort();
        }
        for worker in self.workers.values() {
            worker.stop.cancel();
        }
        let mut tasks = tokio::task::JoinSet::new();
        for (_, worker) in std::mem::take(&mut self.workers) {
            tasks.spawn(worker.task);
        }
        let mut errors = vec![];
        if tasks.is_empty() {
            return errors;
        }
        loop {
            tokio::select! {
                Some(result) = tasks.join_next(), if !tasks.is_empty() => { if let Err(e) = result { errors.push(format!("worker task: {e}")); } else if let Ok(Err(e)) = result { errors.push(format!("worker: {e}")); } },
                Some(message) = messages.recv() => if let Message::Log(log) = message && log.contains("shutdown / restore:") && !log.ends_with("OK") { errors.push(log); },
                else => break,
            }
            if tasks.is_empty() {
                break;
            }
        }
        while let Ok(message) = messages.try_recv() {
            if let Message::Log(log) = message
                && log.contains("shutdown / restore:")
                && !log.ends_with("OK")
            {
                errors.push(log);
            }
        }
        errors
    }
}
fn snapshot(d: &Device, status: &str) -> Snapshot {
    Snapshot {
        identity: d.identity.clone(),
        sensors: d.sensors.clone(),
        control: d.control.clone(),
        taken_over: d.taken_over,
        status: status.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmcforge_profiles::Registry;
    #[tokio::test]
    async fn empty_fleet_exit_does_not_wait_on_its_own_sender() {
        let (tx, mut rx) = mpsc::channel(1);
        let rt = Runtime::new(tx, &Settings::default(), true);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), rt.shutdown(&mut rx))
                .await
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn full_event_channel_does_not_deadlock_shutdown() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut rt = Runtime::new(tx, &Settings::default(), true);
        for s in bmcforge_ipmi::mock::servers() {
            rt.add(s);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            tokio::time::timeout(Duration::from_secs(2), rt.shutdown(&mut rx))
                .await
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn worker_serializes_control_and_restores_before_removing() -> Result<()> {
        let (tx, mut rx) = mpsc::channel(32);
        let mut rt = Runtime::new(tx, &Settings::default(), true);
        let s = bmcforge_ipmi::mock::servers().remove(0);
        let id = s.id.clone();
        rt.add(s);
        let profile = Registry::load(None)?.profiles["dell-idrac6-8"].clone();
        rt.send(&id, Command::Manual(Box::new(profile), 35, 255))?;
        let requested = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(message) = rx.recv().await {
                if let Message::Snapshot(_, s) = message
                    && s.control
                        == (ControlState::Requested {
                            percent: 35,
                            zone: 255,
                        })
                {
                    return s;
                }
            }
            panic!("missing control snapshot");
        })
        .await?;
        assert!(requested.taken_over);
        rt.send(&id, Command::Remove)?;
        tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(message) = rx.recv().await {
                if let Message::Removed(removed, error) = message {
                    assert_eq!(removed, id);
                    assert!(error.is_none());
                    return;
                }
            }
        })
        .await?;
        rt.forget(&id);
        assert!(rt.shutdown(&mut rx).await.is_empty());
        Ok(())
    }
}
