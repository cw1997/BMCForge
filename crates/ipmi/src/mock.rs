//! In-memory command-level BMC used by the demo and deterministic control tests.
//! This does not simulate RMCP+ authentication; wire tests exercise that separately.
use crate::RawTransport;
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use bmcforge_core::config::{Credentials, Server};

pub fn servers() -> Vec<Server> {
    [
        ("demo-dell", "PowerEdge R730", "192.0.2.10", "dell-idrac6-8"),
        (
            "demo-smc",
            "Supermicro X11",
            "192.0.2.11",
            "supermicro-x10-x13",
        ),
    ]
    .into_iter()
    .map(|(id, name, host, profile)| Server {
        id: id.into(),
        name: name.into(),
        host: host.into(),
        profile: Some(profile.into()),
        credentials: Credentials {
            username: "demo".into(),
            password: "demo".into(),
        },
        tags: vec!["Demo lab".into()],
        allow_experimental: true,
        ..Default::default()
    })
    .collect()
}
pub fn sensor_record(number: u8, name: &str, kind: u8, unit: u8, m: u16) -> Vec<u8> {
    let name = &name.as_bytes()[..name.len().min(16)];
    let mut body = vec![0u8; 43 + name.len()];
    body[0] = 0x20;
    body[2] = number;
    body[3] = 7;
    body[7] = kind;
    body[8] = 1;
    body[16] = unit;
    body[19] = m as u8;
    body[20] = ((m >> 8) as u8) << 6;
    body[42] = 0xc0 | name.len() as u8;
    body[43..].copy_from_slice(name);
    let mut raw = vec![number, 0, 0x51, 1, body.len() as u8];
    raw.extend(body);
    raw
}
pub fn fru_bytes(model: &str) -> Vec<u8> {
    let mut area = vec![1, 0, 0];
    for value in ["BMCForge simulator", model, "SIM-001"] {
        let bytes = value.as_bytes();
        area.push(0xc0 | bytes.len() as u8);
        area.extend(bytes);
    }
    area.push(0xc1);
    while (area.len() + 1) % 8 != 0 {
        area.push(0);
    }
    area[1] = ((area.len() + 1) / 8) as u8;
    let checksum = 0u8.wrapping_sub(area.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    area.push(checksum);
    let mut bytes = vec![1, 0, 0, 0, 1, 0, 0, 254];
    bytes.extend(area);
    bytes
}

pub struct MockBmc {
    pub writes: Vec<Vec<u8>>,
    pub fail_write: Option<usize>,
    records: Vec<Vec<u8>>,
    fru: Vec<u8>,
    smc: bool,
    duty: u8,
    tick: u8,
}
impl MockBmc {
    pub fn new(server: &Server) -> Self {
        let smc = server
            .profile
            .as_deref()
            .is_some_and(|p| p.starts_with("supermicro"));
        let mut records = vec![
            sensor_record(1, "CPU Temp", 1, 1, 1),
            sensor_record(2, "Inlet Temp", 1, 1, 1),
        ];
        for n in 3..=6 {
            records.push(sensor_record(n, &format!("FAN{}", n - 2), 4, 18, 100));
        }
        records.push(sensor_record(7, "System Power", 0x0b, 6, 10));
        records.push(sensor_record(8, "+12V", 2, 4, 1));
        records.last_mut().unwrap()[5 + 24] = 0xf0;
        let mut discrete = sensor_record(9, "PSU Status", 8, 0, 1);
        discrete[5 + 8] = 0x6f;
        records.push(discrete);
        Self {
            writes: vec![],
            fail_write: None,
            records,
            fru: fru_bytes(if smc { "X11SSL-F" } else { "PowerEdge R730" }),
            smc,
            duty: 50,
            tick: 0,
        }
    }
}
#[async_trait]
impl RawTransport for MockBmc {
    async fn raw(&mut self, netfn: u8, command: u8, data: &[u8]) -> Result<Vec<u8>> {
        match (netfn, command) {
            (6, 1) => {
                let m: u32 = if self.smc { 10876 } else { 674 };
                Ok(vec![
                    1,
                    0,
                    2,
                    0x30,
                    2,
                    0,
                    m as u8,
                    (m >> 8) as u8,
                    (m >> 16) as u8,
                    1,
                    0,
                ])
            }
            (10, 0x10) => Ok(vec![self.fru.len() as u8, (self.fru.len() >> 8) as u8, 0]),
            (10, 0x11) => {
                ensure!(data.len() == 4, "bad FRU request");
                let offset = u16::from_le_bytes([data[1], data[2]]) as usize;
                let end = (offset + data[3] as usize).min(self.fru.len());
                ensure!(offset < end, "bad FRU offset");
                let mut out = vec![(end - offset) as u8];
                out.extend_from_slice(&self.fru[offset..end]);
                Ok(out)
            }
            (10, 0x22) => Ok(vec![1, 0]),
            (10, 0x23) => {
                ensure!(data.len() == 6, "bad SDR request");
                let id = u16::from_le_bytes([data[2], data[3]]) as usize;
                let index = if id == 0 { 0 } else { id - 1 };
                let record = self
                    .records
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("record not found"))?;
                let offset = usize::from(data[4]);
                let end = (offset + usize::from(data[5])).min(record.len());
                ensure!(offset <= end, "bad offset");
                let next: u16 = if index + 1 == self.records.len() {
                    0xffff
                } else {
                    (index + 2) as u16
                };
                let mut out = next.to_le_bytes().to_vec();
                out.extend_from_slice(&record[offset..end]);
                Ok(out)
            }
            (4, 0x2d) => {
                ensure!(data.len() == 1, "bad reading request");
                self.tick = self.tick.wrapping_add(1);
                let v = match data[0] {
                    1 => 48 + self.tick % 4,
                    2 => 24,
                    3..=6 => self.duty.saturating_add(self.tick % 3),
                    7 => 18,
                    8 => 120,
                    _ => 0,
                };
                Ok(vec![v, 0xc0, 0, 0])
            }
            (0x30, _) => {
                let mut request = vec![netfn, command];
                request.extend_from_slice(data);
                self.writes.push(request);
                if self.fail_write == Some(self.writes.len()) {
                    bail!("simulated timeout");
                }
                if (command == 0x30 && data.first() == Some(&2))
                    || (command == 0x70 && data.first() == Some(&0x66))
                {
                    self.duty = *data.last().unwrap();
                }
                Ok(vec![])
            }
            _ => bail!("unsupported simulated command {netfn:02x}/{command:02x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Device;
    use std::sync::{Arc, Mutex};
    struct Tracked {
        inner: MockBmc,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }
    #[async_trait]
    impl RawTransport for Tracked {
        async fn raw(&mut self, n: u8, c: u8, data: &[u8]) -> Result<Vec<u8>> {
            if n == 0x30 {
                let mut v = vec![n, c];
                v.extend_from_slice(data);
                self.writes.lock().unwrap().push(v);
            }
            self.inner.raw(n, c, data).await
        }
    }
    #[tokio::test(start_paused = true)]
    async fn failed_sequence_is_not_replayed_and_restore_is_retained() -> Result<()> {
        let server = servers().remove(0);
        let mut inner = MockBmc::new(&server);
        inner.fail_write = Some(2);
        let writes = Arc::new(Mutex::new(vec![]));
        let t = Tracked {
            inner,
            writes: writes.clone(),
        };
        let mut d = Device::with_transport(server, Box::new(t));
        let r = Registry::load(None)?;
        assert!(
            d.manual(&r.profiles["dell-idrac6-8"], 30, 255)
                .await
                .is_err()
        );
        assert_eq!(
            *writes.lock().unwrap(),
            [vec![48, 48, 1, 0], vec![48, 48, 2, 255, 30]]
        );
        assert!(
            d.manual(&r.profiles["dell-idrac6-8"], 30, 255)
                .await
                .is_err()
        );
        assert_eq!(writes.lock().unwrap().len(), 2);
        d.shutdown().await?;
        assert_eq!(writes.lock().unwrap().last(), Some(&vec![48, 48, 1, 1]));
        Ok(())
    }
    #[tokio::test(start_paused = true)]
    async fn changing_zones_does_not_reenter_full_mode() -> Result<()> {
        let server = servers().remove(1);
        let writes = Arc::new(Mutex::new(vec![]));
        let t = Tracked {
            inner: MockBmc::new(&server),
            writes: writes.clone(),
        };
        let mut d = Device::with_transport(server, Box::new(t));
        let r = Registry::load(None)?;
        let p = &r.profiles["supermicro-x10-x13"];
        d.manual(p, 40, 0).await?;
        d.manual(p, 50, 1).await?;
        assert_eq!(
            *writes.lock().unwrap(),
            [
                vec![48, 69, 1, 1],
                vec![48, 112, 102, 1, 0, 40],
                vec![48, 112, 102, 1, 1, 50]
            ]
        );
        assert!(
            d.manual(&r.profiles["supermicro-x9"], 50, 16)
                .await
                .is_err()
        );
        d.shutdown().await?;
        Ok(())
    }
    use bmcforge_core::ControlState;
    use bmcforge_profiles::Registry;
    #[tokio::test(start_paused = true)]
    async fn full_demo_poll_control_and_restore() -> Result<()> {
        let r = Registry::load(None)?;
        let mut d = Device::new(servers().remove(0), true);
        d.poll().await?;
        assert_eq!(d.identity.model, "PowerEdge R730");
        assert_eq!(d.sensors.len(), 9);
        assert!(
            d.sensors
                .iter()
                .any(|s| s.unit == "RPM" && s.value.is_some())
        );
        assert_eq!(d.sensors[7].value, Some(12.0));
        d.manual(&r.profiles["dell-idrac6-8"], 35, 255).await?;
        assert!(d.taken_over);
        assert_eq!(
            d.control,
            ControlState::Requested {
                percent: 35,
                zone: 255
            }
        );
        d.shutdown().await?;
        assert!(!d.taken_over);
        assert_eq!(d.control, ControlState::Automatic);
        Ok(())
    }
    #[tokio::test(start_paused = true)]
    async fn partial_write_retains_restore_obligation() -> Result<()> {
        let s = servers().remove(0);
        let mut transport = MockBmc::new(&s);
        transport.fail_write = Some(2);
        let mut d = Device::with_transport(s, Box::new(transport));
        let r = Registry::load(None)?;
        assert!(
            d.manual(&r.profiles["dell-idrac6-8"], 35, 255)
                .await
                .is_err()
        );
        assert!(d.taken_over);
        assert_eq!(d.control, ControlState::Uncertain);
        d.shutdown().await?;
        assert_eq!(d.control, ControlState::Automatic);
        Ok(())
    }
    #[tokio::test(start_paused = true)]
    async fn restore_failure_is_reported() -> Result<()> {
        let s = servers().remove(0);
        let mut transport = MockBmc::new(&s);
        transport.fail_write = Some(3);
        let mut d = Device::with_transport(s, Box::new(transport));
        let r = Registry::load(None)?;
        d.manual(&r.profiles["dell-idrac6-8"], 35, 255).await?;
        assert!(d.shutdown().await.is_err());
        assert!(d.taken_over);
        assert_eq!(d.control, ControlState::Uncertain);
        Ok(())
    }
}
