use crate::RawTransport;
use anyhow::{Context, Result, ensure};

pub struct FruInfo {
    pub model: String,
    pub fields: Vec<String>,
}

pub fn decode_field(kind: u8, bytes: &[u8]) -> String {
    match kind {
        3 => bytes
            .iter()
            .map(|b| char::from(*b))
            .collect::<String>()
            .trim_end_matches(['\0', ' '])
            .to_string(),
        2 => {
            let mut result = String::new();
            let mut value = 0u32;
            let mut bits = 0;
            for byte in bytes {
                value |= u32::from(*byte) << bits;
                bits += 8;
                while bits >= 6 {
                    result.push(char::from((value & 63) as u8 + 32));
                    value >>= 6;
                    bits -= 6;
                }
            }
            result
        }
        1 => bytes
            .iter()
            .flat_map(|b| [b & 15, b >> 4])
            .map(|v| b"0123456789 -.:,_"[usize::from(v)] as char)
            .collect(),
        _ => bytes.iter().map(|b| format!("{b:02x}")).collect(),
    }
}

pub fn parse(bytes: &[u8]) -> Result<FruInfo> {
    ensure!(
        bytes.len() >= 8
            && bytes[0] & 15 == 1
            && bytes[..8].iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0,
        "invalid FRU common header/checksum"
    );
    let mut fields = vec![];
    let mut model = String::new();
    for (header, start, label) in [(3, 6, "Board"), (4, 3, "Product")] {
        let offset = usize::from(bytes[header]) * 8;
        if offset == 0 {
            continue;
        }
        let area_len = usize::from(*bytes.get(offset + 1).context("short FRU area header")?) * 8;
        let area = bytes
            .get(offset..offset + area_len)
            .context("short FRU area")?;
        ensure!(
            area.len() > start && area.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0,
            "invalid FRU area checksum"
        );
        let mut pos = start;
        let mut index = 0;
        while pos < area.len() - 1 && area[pos] != 0xc1 {
            let tag = area[pos];
            pos += 1;
            let len = usize::from(tag & 63);
            let value = decode_field(
                tag >> 6,
                area.get(pos..pos + len).context("truncated FRU field")?,
            );
            if index == 1 && (header == 4 || model.is_empty()) {
                model.clone_from(&value);
            }
            fields.push(format!("{label}[{index}]: {value}"));
            pos += len;
            index += 1;
        }
    }
    Ok(FruInfo { model, fields })
}

pub async fn read(t: &mut dyn RawTransport, device: u8) -> Result<FruInfo> {
    let info = t.raw(0x0a, 0x10, &[device]).await?;
    ensure!(info.len() >= 3, "short FRU info");
    let size = usize::from(u16::from_le_bytes([info[0], info[1]]));
    ensure!(size >= 8, "empty FRU");
    let word = info[2] & 1 != 0;
    let mut data = vec![];
    while data.len() < size {
        let offset = if word { data.len() / 2 } else { data.len() };
        let count = (size - data.len()).min(16);
        let out = t
            .raw(
                0x0a,
                0x11,
                &[device, offset as u8, (offset >> 8) as u8, count as u8],
            )
            .await?;
        ensure!(
            out.len() >= 2 && usize::from(out[0]) == out.len() - 1 && out.len() - 1 <= count,
            "invalid FRU read length"
        );
        data.extend_from_slice(&out[1..]);
        ensure!(
            !word || data.len() % 2 == 0 || data.len() == size,
            "unaligned word-access FRU response"
        );
    }
    parse(&data)
}

#[cfg(test)]
mod tests {
    #[test]
    fn checksums_and_model() {
        let mut bytes = crate::mock::fru_bytes("PowerEdge R730");
        assert_eq!(super::parse(&bytes).unwrap().model, "PowerEdge R730");
        bytes[10] ^= 1;
        assert!(super::parse(&bytes).is_err());
        assert!(super::parse(&[1]).is_err());
    }
}
