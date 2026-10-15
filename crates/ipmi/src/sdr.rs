use crate::RawTransport;
use anyhow::{Context, Result, ensure};
use bmcforge_core::Sensor;

#[derive(Clone, Debug)]
pub struct Record {
    pub record_id: u16,
    pub number: u8,
    pub owner: u8,
    pub lun: u8,
    pub name: String,
    pub sensor_type: u8,
    pub event_type: u8,
    pub units: u8,
    pub base_unit: u8,
    pub linearization: u8,
    pub m: i16,
    pub b: i16,
    pub r_exp: i8,
    pub b_exp: i8,
    pub full: bool,
    pub raw: Vec<u8>,
}

fn signed(value: u16, bits: u8) -> i16 {
    ((value << (16 - bits)) as i16) >> (16 - bits)
}

impl Record {
    pub fn parse(raw: &[u8]) -> Result<Option<Self>> {
        ensure!(raw.len() >= 5, "short SDR header");
        ensure!(raw.len() == 5 + raw[4] as usize, "SDR length mismatch");
        let full = raw[3] == 1;
        if !full && raw[3] != 2 {
            return Ok(None);
        }
        let body = &raw[5..];
        let name_offset = if full { 42 } else { 26 };
        ensure!(body.len() > name_offset, "short sensor SDR");
        let id_type = body[name_offset];
        let len = (id_type & 31) as usize;
        let name_bytes = body
            .get(name_offset + 1..name_offset + 1 + len)
            .context("truncated SDR name")?;
        let name = crate::fru::decode_field(id_type >> 6, name_bytes);
        Ok(Some(Self {
            record_id: u16::from_le_bytes([raw[0], raw[1]]),
            number: body[2],
            owner: body[0],
            lun: body[1] & 3,
            name,
            sensor_type: body[7],
            event_type: body[8] & 0x7f,
            units: body[15],
            base_unit: body[16],
            linearization: if full { body[18] & 0x7f } else { 0 },
            m: if full {
                signed(u16::from(body[19]) | (u16::from(body[20] & 0xc0) << 2), 10)
            } else {
                0
            },
            b: if full {
                signed(u16::from(body[21]) | (u16::from(body[22] & 0xc0) << 2), 10)
            } else {
                0
            },
            r_exp: if full {
                signed(u16::from(body[24] >> 4), 4) as i8
            } else {
                0
            },
            b_exp: if full {
                signed(u16::from(body[24] & 15), 4) as i8
            } else {
                0
            },
            full,
            raw: raw.to_vec(),
        }))
    }
    pub fn id(&self) -> String {
        format!("{:04x}:{:02x}", self.record_id, self.number)
    }
    pub fn kind(&self) -> String {
        match self.sensor_type {
            1 => "Temperature",
            2 => "Voltage",
            3 => "Current",
            4 => "Fan",
            8 => "Power supply",
            9 => "Power unit",
            0x0c => "Memory",
            _ => "Discrete/OEM",
        }
        .into()
    }
    pub fn unit(&self) -> String {
        match self.base_unit {
            1 => "°C",
            2 => "°F",
            3 => "K",
            4 => "V",
            5 => "A",
            6 => "W",
            18 => "RPM",
            19 => "Hz",
            _ => "",
        }
        .into()
    }
    pub fn convert(&self, raw: u8) -> Option<f64> {
        if !self.full || self.event_type != 1 || self.units >> 6 == 3 {
            return None;
        }
        let x = match self.units >> 6 {
            0 => f64::from(raw),
            1 => {
                if raw & 128 != 0 {
                    -f64::from(!raw & 127)
                } else {
                    f64::from(raw)
                }
            }
            2 => f64::from(raw as i8),
            _ => return None,
        };
        let y = (f64::from(self.m) * x + f64::from(self.b) * 10f64.powi(i32::from(self.b_exp)))
            * 10f64.powi(i32::from(self.r_exp));
        let y = match self.linearization {
            0 => y,
            1 => y.ln(),
            2 => y.log10(),
            3 => y.log2(),
            4 => y.exp(),
            5 => 10f64.powf(y),
            6 => 2f64.powf(y),
            7 => 1.0 / y,
            8 => y * y,
            9 => y * y * y,
            10 => y.sqrt(),
            11 => y.cbrt(),
            _ => return None,
        };
        y.is_finite().then_some(y)
    }
    pub fn reading(&self, data: &[u8]) -> Sensor {
        let unavailable = data.len() < 2 || data[1] & 0x20 != 0;
        let value = if unavailable {
            None
        } else {
            self.convert(data[0])
        };
        let state = if unavailable {
            "unavailable".into()
        } else if data[1] & 0x40 == 0 {
            "scanning disabled".into()
        } else {
            let bits = u16::from(*data.get(2).unwrap_or(&0))
                | (u16::from(*data.get(3).unwrap_or(&0) & 0x7f) << 8);
            if self.event_type == 1 {
                if bits & 0x3f == 0 {
                    "OK".into()
                } else {
                    format!("threshold assertions 0x{:02x}", bits & 0x3f)
                }
            } else {
                format!("discrete assertions 0x{bits:04x}")
            }
        };
        Sensor {
            id: self.id(),
            name: self.name.clone(),
            kind: self.kind(),
            value: if data.get(1).is_some_and(|s| s & 0x40 == 0) {
                None
            } else {
                value
            },
            unit: self.unit(),
            state,
            raw: data.to_vec(),
        }
    }
}

pub async fn enumerate(t: &mut dyn RawTransport) -> Result<Vec<Record>> {
    let reservation = t.raw(0x0a, 0x22, &[]).await?;
    ensure!(reservation.len() >= 2, "short SDR reservation");
    let mut id = 0u16;
    let mut seen = std::collections::HashSet::new();
    let mut records = vec![];
    loop {
        ensure!(seen.insert(id), "SDR chain cycle");
        ensure!(seen.len() <= 4096, "SDR repository exceeds 4096 records");
        let mut raw = vec![];
        let mut next = 0xffff;
        let mut length = 5;
        while raw.len() < length {
            let offset = raw.len();
            let count = (length - offset).min(16);
            let request = [
                reservation[0],
                reservation[1],
                id as u8,
                (id >> 8) as u8,
                offset as u8,
                count as u8,
            ];
            let bytes = t.raw(0x0a, 0x23, &request).await?;
            ensure!(bytes.len() == count + 2, "short Get SDR response");
            next = u16::from_le_bytes([bytes[0], bytes[1]]);
            raw.extend_from_slice(&bytes[2..]);
            if raw.len() >= 5 {
                length = 5 + raw[4] as usize;
            }
            ensure!(length <= 255, "SDR record too large for chunked reader");
        }
        if let Some(record) = Record::parse(&raw)? {
            // Satellite sensors need bridging. Do not substitute BMC-local readings.
            records.push(record);
        }
        if next == 0xffff {
            break;
        }
        id = next;
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_scaling_and_unavailable() {
        let mut r = crate::mock::sensor_record(1, "CPU Temp", 1, 1, 1);
        r[5 + 15] = 0x80;
        let mut record = Record::parse(&r).unwrap().unwrap();
        assert_eq!(record.convert(0xf6), Some(-10.0));
        record.m = 10;
        record.r_exp = -1;
        record.b = 2;
        assert!((record.convert(20).unwrap() - 20.2).abs() < 1e-10);
        assert_eq!(record.reading(&[20, 0x60]).value, None);
        assert_eq!(record.reading(&[20, 0]).state, "scanning disabled");
        record.event_type = 0x6f;
        let s = record.reading(&[0, 0x40, 4, 1]);
        assert_eq!(s.value, None);
        assert_eq!(s.state, "discrete assertions 0x0104");
        assert!(Record::parse(&r[..10]).is_err());
    }
}
