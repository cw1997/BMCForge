pub mod discovery;
pub mod fru;
pub mod mock;
pub mod sdr;

use anyhow::{Context, Result, anyhow, ensure};
use async_trait::async_trait;
use bmcforge_core::{ControlState, ControlValue, DeviceIdentity, Sensor, config::Server};
use bmcforge_profiles::{Command, Profile};
use std::time::Duration;

#[async_trait]
pub trait RawTransport: Send {
    async fn raw(&mut self, netfn: u8, command: u8, data: &[u8]) -> Result<Vec<u8>>;
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Stop background credential retries after an explicit RAKP failure.
pub fn is_authentication_error(error: &anyhow::Error) -> bool {
    error.chain().any(|e| {
        matches!(
            e.downcast_ref::<ipmi::Error>(),
            Some(ipmi::Error::AuthenticationFailed(_))
        )
    })
}

pub struct NativeTransport {
    client: std::sync::Arc<std::sync::Mutex<ipmi::BlockingClient>>,
}
impl NativeTransport {
    pub async fn connect(server: &Server) -> Result<Self> {
        server.validate()?;
        let addr = tokio::net::lookup_host((server.host.as_str(), server.port))
            .await?
            .next()
            .context("hostname has no addresses")?;
        let privilege = match server.privilege {
            1 => ipmi::PrivilegeLevel::Callback,
            2 => ipmi::PrivilegeLevel::User,
            3 => ipmi::PrivilegeLevel::Operator,
            5 => ipmi::PrivilegeLevel::Oem,
            _ => ipmi::PrivilegeLevel::Administrator,
        };
        // Exactly one transport attempt, including writes. Read retries live above this layer.
        let server = server.clone();
        let client = tokio::task::spawn_blocking(move || {
            let mut builder = ipmi::BlockingClient::builder(addr)
                .username(&server.credentials.username)
                .password(&server.credentials.password)
                .privilege_level(privilege)
                .timeout(Duration::from_millis(server.timeout_ms))
                .retries(1);
            if let Some(kg) = &server.kg {
                builder = builder.bmc_key(kg);
            }
            let client = builder.build()?;
            let response = client.send_raw(6, 0x3b, &[server.privilege])?;
            if response.completion_code != 0 {
                let _ = client.close_session();
                return Err(ipmi::Error::Protocol(
                    "requested session privilege rejected",
                ));
            }
            Ok::<_, ipmi::Error>(client)
        })
        .await?
        .context("RMCP+ session authentication failed")?;
        Ok(Self {
            client: std::sync::Arc::new(std::sync::Mutex::new(client)),
        })
    }
}
#[async_trait]
impl RawTransport for NativeTransport {
    async fn raw(&mut self, netfn: u8, command: u8, data: &[u8]) -> Result<Vec<u8>> {
        let client = self.client.clone();
        let data = data.to_vec();
        let response = tokio::task::spawn_blocking(move || {
            client
                .lock()
                .map_err(|_| anyhow!("IPMI lock poisoned"))?
                .send_raw(netfn, command, &data)
                .map_err(anyhow::Error::from)
        })
        .await??;
        if response.completion_code != 0 {
            return Err(ipmi::Error::CompletionCode {
                completion_code: response.completion_code,
            }
            .into());
        }
        Ok(response.data)
    }
    async fn close(&mut self) -> Result<()> {
        let client = self.client.clone();
        tokio::task::spawn_blocking(move || {
            client
                .lock()
                .map_err(|_| anyhow!("IPMI lock poisoned"))?
                .close_session()
                .map_err(anyhow::Error::from)
        })
        .await?
    }
}

pub struct Device {
    pub server: Server,
    transport: Option<Box<dyn RawTransport>>,
    pub identity: DeviceIdentity,
    pub sensors: Vec<Sensor>,
    records: Vec<sdr::Record>,
    pub control: ControlState,
    pub taken_over: bool,
    restore: Vec<Command>,
    zone_restore: std::collections::BTreeMap<u8, Vec<Command>>,
    demo: bool,
    injected: bool,
    active_profile: Option<String>,
}

impl Device {
    pub fn new(server: Server, demo: bool) -> Self {
        Self {
            server,
            transport: None,
            identity: DeviceIdentity::default(),
            sensors: vec![],
            records: vec![],
            control: ControlState::Unknown,
            taken_over: false,
            restore: vec![],
            zone_restore: Default::default(),
            demo,
            injected: false,
            active_profile: None,
        }
    }
    pub fn with_transport(server: Server, transport: Box<dyn RawTransport>) -> Self {
        let mut d = Self::new(server, false);
        d.transport = Some(transport);
        d.injected = true;
        d
    }
    async fn connect(&mut self) -> Result<()> {
        if self.transport.is_none() {
            self.identity = DeviceIdentity::default();
            self.transport = Some(if self.demo {
                Box::new(mock::MockBmc::new(&self.server))
            } else {
                Box::new(NativeTransport::connect(&self.server).await?)
            });
        }
        Ok(())
    }
    /// Authentication and identity only. Does not enumerate SDRs or write fan commands.
    pub async fn identify(&mut self) -> Result<DeviceIdentity> {
        self.connect().await?;
        let t = self.transport.as_mut().context("no connection")?;
        if self.identity.manufacturer_id == 0 {
            let bytes = t.raw(6, 1, &[]).await?;
            ensure!(bytes.len() >= 11, "truncated device ID");
            self.identity = DeviceIdentity {
                manufacturer_id: u32::from_le_bytes([bytes[6], bytes[7], bytes[8], 0]),
                product_id: u16::from_le_bytes([bytes[9], bytes[10]]),
                firmware: if bytes[3] >> 4 <= 9 && bytes[3] & 15 <= 9 {
                    format!(
                        "{}.{}",
                        bytes[2] & 0x7f,
                        (bytes[3] >> 4) * 10 + (bytes[3] & 15)
                    )
                } else {
                    String::new()
                },
                device_id_raw: bytes.clone(),
                ..Default::default()
            };
            match fru::read(t.as_mut(), 0).await {
                Ok(info) => {
                    self.identity.model = info.model;
                    self.identity.machine_types = info
                        .fields
                        .iter()
                        .filter(|s| s.starts_with("Product[1]:") || s.starts_with("Product[2]:"))
                        .flat_map(|s| s.split(|c: char| !c.is_ascii_alphanumeric()))
                        .filter(|s| {
                            (s.len() == 4 || s.len() == 7)
                                && s.as_bytes()[0].is_ascii_digit()
                                && s.bytes().take(4).all(|c| c.is_ascii_alphanumeric())
                        })
                        .map(str::to_owned)
                        .collect();
                    self.identity.fru = info.fields;
                }
                Err(err) => self.identity.fru = vec![format!("FRU unavailable: {err}")],
            }
        }
        Ok(self.identity.clone())
    }
    async fn read_once(&mut self) -> Result<()> {
        self.identify().await?;
        let t = self.transport.as_mut().context("no connection")?;
        if self.records.is_empty() {
            self.records = sdr::enumerate(t.as_mut()).await?;
        }
        let mut sensors = Vec::with_capacity(self.records.len());
        let mut successful_reads = 0;
        for record in &self.records {
            if record.owner != 0x20 || record.lun != 0 {
                sensors.push(Sensor {
                    id: record.id(),
                    name: record.name.clone(),
                    kind: record.kind(),
                    value: None,
                    unit: record.unit(),
                    state: "bridged/satellite sensor: reading not implemented".into(),
                    raw: record.raw.clone(),
                });
                continue;
            }
            let sensor = match t.raw(4, 0x2d, &[record.number]).await {
                Ok(reading) => {
                    successful_reads += 1;
                    record.reading(&reading)
                }
                Err(error)
                    if matches!(
                        error.downcast_ref::<ipmi::Error>(),
                        Some(ipmi::Error::CompletionCode { .. })
                    ) =>
                {
                    Sensor {
                        id: record.id(),
                        name: record.name.clone(),
                        kind: record.kind(),
                        value: None,
                        unit: record.unit(),
                        state: format!("read failed: {error}"),
                        raw: vec![],
                    }
                }
                Err(error) => {
                    return Err(error.context("sensor transport failed; stop polling this session"));
                }
            };
            sensors.push(sensor);
        }
        self.sensors = sensors;
        ensure!(
            successful_reads > 0 || self.records.iter().all(|r| r.owner != 0x20 || r.lun != 0),
            "all sensor reads failed; reconnect required"
        );
        Ok(())
    }
    pub async fn poll(&mut self) -> Result<()> {
        let mut last = anyhow!("poll failed");
        for attempt in 0..=self.server.retries {
            match self.read_once().await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    let authentication_failed = is_authentication_error(&error);
                    last = error;
                    if let Some(mut transport) = self.transport.take() {
                        let _ = transport.close().await;
                    }
                    self.records.clear();
                    if authentication_failed {
                        break;
                    }
                    if attempt < self.server.retries {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
        for s in &mut self.sensors {
            s.value = None;
            s.state = "stale / disconnected".into();
        }
        Err(last)
    }
    async fn sequence(&mut self, commands: &[Command]) -> Result<()> {
        self.connect().await?;
        for command in commands {
            let response = self
                .transport
                .as_mut()
                .context("no connection")?
                .raw(command.netfn, command.command, &command.data)
                .await?;
            if let Some(expected) = &command.expect {
                ensure!(&response == expected, "OEM command response mismatch");
            }
            if command.delay_ms > 0 && !self.demo {
                tokio::time::sleep(Duration::from_millis(command.delay_ms)).await;
            }
        }
        Ok(())
    }
    pub async fn manual(&mut self, profile: &Profile, percent: u8, zone: u8) -> Result<()> {
        self.manual_value(profile, ControlValue::Percent(percent), zone)
            .await
    }
    pub async fn manual_value(
        &mut self,
        profile: &Profile,
        value: ControlValue,
        zone: u8,
    ) -> Result<()> {
        ensure!(
            profile.eligible(self.server.allow_experimental),
            "profile is monitoring-only or experimental control is disabled"
        );
        ensure!(
            !matches!(self.control, ControlState::Uncertain),
            "restore automatic mode before another manual request"
        );
        let commands = profile.commands_value(value, zone, !self.taken_over)?;
        let per_zone = profile.fan.as_ref().is_some_and(|f| f.restore_per_zone);
        let restore = if per_zone {
            profile.automatic_zone(zone)?
        } else {
            profile.automatic()?
        };
        self.connect().await?;
        if self.identity.manufacturer_id == 0 {
            self.identify().await?;
        }
        ensure!(
            profile.compatible(&self.identity),
            "device identity or firmware is outside this profile's constraints"
        );
        if !self.taken_over {
            self.sequence(&profile.probe(value, zone)?)
                .await
                .context("adapter protocol probe failed; fan writes were not started")?;
        }
        if self.taken_over {
            ensure!(
                self.active_profile.as_deref() == Some(profile.id.as_str())
                    && (per_zone || self.restore == restore),
                "restore automatic mode before changing the active control profile"
            );
        }
        if per_zone {
            // Record obligations before attempting I/O: timeout/partial failure may still have applied.
            self.zone_restore.insert(zone, restore);
        } else {
            self.restore = restore;
        }
        self.active_profile = Some(profile.id.clone());
        self.taken_over = true;
        self.control = ControlState::Uncertain;
        match self.sequence(&commands).await {
            Ok(()) => {
                self.control = match value {
                    ControlValue::Percent(percent) => ControlState::Requested { percent, zone },
                    ControlValue::Raw(value) => ControlState::RawRequested { value, zone },
                };
                Ok(())
            }
            Err(error) => {
                if !self.injected
                    && let Some(mut transport) = self.transport.take()
                {
                    let _ = transport.close().await;
                }
                Err(error.context(
                    "control result uncertain; sequence was not replayed; restore automatic mode",
                ))
            }
        }
    }
    pub async fn automatic(&mut self, profile: Option<&Profile>) -> Result<()> {
        if self.taken_over && !self.zone_restore.is_empty() {
            self.control = ControlState::Uncertain;
            let mut errors = vec![];
            for (zone, commands) in self.zone_restore.clone() {
                match self.sequence(&commands).await {
                    Ok(()) => {
                        self.zone_restore.remove(&zone);
                    }
                    Err(e) => errors.push(format!("bank {zone}: {e:#}")),
                }
            }
            ensure!(
                errors.is_empty(),
                "automatic bank restore failed: {}",
                errors.join("; ")
            );
            self.control = ControlState::Automatic;
            self.taken_over = false;
            self.active_profile = None;
            return Ok(());
        }
        let commands = if self.taken_over {
            self.restore.clone()
        } else {
            let p = profile.context("choose a control profile")?;
            ensure!(
                p.eligible(self.server.allow_experimental),
                "control unavailable"
            );
            self.connect().await?;
            if self.identity.manufacturer_id == 0 {
                self.identify().await?;
            }
            ensure!(
                p.compatible(&self.identity),
                "device identity or firmware is outside this profile's constraints"
            );
            p.automatic()?
        };
        self.control = ControlState::Uncertain;
        self.sequence(&commands)
            .await
            .context("automatic restore failed; control remains uncertain")?;
        self.control = ControlState::Automatic;
        self.taken_over = false;
        self.restore.clear();
        self.active_profile = None;
        Ok(())
    }
    pub async fn shutdown(&mut self) -> Result<()> {
        if self.taken_over {
            self.automatic(None).await?;
        }
        if let Some(t) = &mut self.transport {
            t.close().await?;
        }
        Ok(())
    }
}
