//! Durable forward-recovery transaction for the configuration and its local adapters.
use anyhow::{Context, Result, ensure};
use bmcforge_core::config::Config;
use bmcforge_profiles::{LocalProfile, Registry};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    path.with_file_name(format!(
        "{}.{}",
        path.file_name().unwrap().to_string_lossy(),
        suffix
    ))
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    config: Config,
    local: BTreeMap<String, LocalProfile>,
}
fn validate(local: &BTreeMap<String, LocalProfile>) -> Result<()> {
    let mut origins = std::collections::BTreeSet::new();
    for (id, entry) in local {
        entry.original.validate()?;
        entry.profile.validate()?;
        ensure!(
            id == &entry.profile.id
                && entry.origin_id == entry.original.id
                && id != &entry.origin_id,
            "invalid local adapter identity"
        );
        ensure!(
            origins.insert(&entry.origin_id),
            "duplicate local adapter origin"
        );
    }
    Ok(())
}
pub fn load(path: &Path, registry: &mut Registry) -> Result<()> {
    let store = sibling(path, "profiles.json");
    if !store.exists() {
        return Ok(());
    }
    let mut local: BTreeMap<String, LocalProfile> = serde_json::from_slice(&std::fs::read(&store)?)
        .with_context(|| format!("read {}", store.display()))?;
    validate(&local)?;
    for (id, entry) in &local {
        ensure!(
            !registry.profiles.contains_key(id),
            "local adapter ID conflicts: {id}"
        );
        ensure!(
            registry.profiles.contains_key(&entry.origin_id),
            "missing original adapter: {}",
            entry.origin_id
        );
    }
    for (id, entry) in &mut local {
        entry.profile.origin_id = Some(entry.origin_id.clone());
        registry.profiles.insert(id.clone(), entry.profile.clone());
    }
    registry.local = local;
    Ok(())
}
pub fn recover(path: &Path) -> Result<()> {
    let journal = sibling(path, "profiles-transaction.json");
    if !journal.exists() {
        return Ok(());
    }
    let tx: Transaction = serde_json::from_slice(&std::fs::read(&journal)?)?;
    tx.config.validate()?;
    validate(&tx.local)?;
    // Install adapters before publishing references; keep the journal until both writes finish.
    atomic(
        &sibling(path, "profiles.json"),
        &serde_json::to_vec_pretty(&tx.local)?,
    )?;
    tx.config.save(path)?;
    std::fs::remove_file(journal)?;
    Ok(())
}
pub fn ensure_clean(path: &Path) -> Result<()> {
    ensure!(
        !sibling(path, "profiles-transaction.json").exists(),
        "adapter transaction pending; restart to recover before saving"
    );
    Ok(())
}
pub fn save(path: &Path, config: &Config, local: &BTreeMap<String, LocalProfile>) -> Result<()> {
    config.validate()?;
    validate(local)?;
    let previous_config = if path.exists() {
        Config::load(path)?
    } else {
        Config::default()
    };
    let store = sibling(path, "profiles.json");
    let previous_local: BTreeMap<String, LocalProfile> = if store.exists() {
        serde_json::from_slice(&std::fs::read(&store)?)?
    } else {
        BTreeMap::new()
    };
    validate(&previous_local)?;
    let journal = sibling(path, "profiles-transaction.json");
    ensure!(
        !journal.exists(),
        "adapter transaction pending; restart to recover before saving"
    );
    atomic(
        &journal,
        &serde_json::to_vec_pretty(&Transaction {
            config: config.clone(),
            local: local.clone(),
        })?,
    )?;
    if let Err(error) = recover(path) {
        // Switch the durable intent to rollback before attempting recovery. If rollback is
        // interrupted, startup finishes it before reading either file.
        atomic(
            &journal,
            &serde_json::to_vec_pretty(&Transaction {
                config: previous_config,
                local: previous_local,
            })?,
        )
        .context("could not record adapter rollback; restart to recover the pending transaction")?;
        if let Err(rollback) = recover(path) {
            anyhow::bail!(
                "adapter save failed: {error:#}; rollback pending: {rollback:#}; restart to recover"
            );
        }
        return Err(error.context("adapter save failed; previous adapters and references restored"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn local() -> Result<BTreeMap<String, LocalProfile>> {
        let registry = Registry::load(None)?;
        let original = registry.profiles["dell-idrac6-8"].clone();
        let mut profile = original.clone();
        profile.id = "dell-idrac6-8-local-1".into();
        profile.notes.push_str(" local");
        Ok(BTreeMap::from([(
            profile.id.clone(),
            LocalProfile {
                origin_id: original.id.clone(),
                original,
                profile,
            },
        )]))
    }
    #[test]
    fn durable_save_load_matching_and_reset() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        let local = local()?;
        let mut config = Config::default();
        config.servers.push(bmcforge_core::config::Server {
            id: "s".into(),
            host: "192.0.2.1".into(),
            profile: Some(local.keys().next().unwrap().clone()),
            ..Default::default()
        });
        save(&path, &config, &local)?;
        let mut registry = Registry::load(None)?;
        load(&path, &mut registry)?;
        assert_eq!(registry.local.len(), 1);
        assert_eq!(
            Config::load(&path)?.servers[0].profile,
            config.servers[0].profile
        );
        let identity = bmcforge_core::DeviceIdentity {
            manufacturer_id: 674,
            product_id: 1,
            model: "PowerEdge R730".into(),
            ..Default::default()
        };
        let candidates = registry.candidates(&identity);
        assert!(candidates.contains(local.keys().next().unwrap()));
        assert!(!candidates.contains(&"dell-idrac6-8".into()));
        config.servers[0].profile = Some("dell-idrac6-8".into());
        save(&path, &config, &BTreeMap::new())?;
        let mut registry = Registry::load(None)?;
        load(&path, &mut registry)?;
        assert!(registry.local.is_empty());
        Ok(())
    }
    #[test]
    fn startup_recovers_interrupted_transaction_and_blocks_other_saves() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        Config::default().save(&path)?;
        let tx = Transaction {
            config: Config::default(),
            local: local()?,
        };
        atomic(
            &sibling(&path, "profiles-transaction.json"),
            &serde_json::to_vec(&tx)?,
        )?;
        assert!(ensure_clean(&path).is_err());
        recover(&path)?;
        ensure_clean(&path)?;
        let mut registry = Registry::load(None)?;
        load(&path, &mut registry)?;
        assert_eq!(registry.local.len(), 1);
        Ok(())
    }
    #[test]
    fn write_failure_rolls_back_store_and_restart_finishes_rollback() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        let old = Config::default();
        old.save(&path)?;
        let backup = sibling(&path, "bak");
        std::fs::create_dir(&backup)?;
        let mut changed = old.clone();
        changed.settings.theme = "light".into();
        assert!(save(&path, &changed, &local()?).is_err());
        assert_eq!(Config::load(&path)?.settings.theme, "dark");
        let mut registry = Registry::load(None)?;
        load(&path, &mut registry)?;
        assert!(registry.local.is_empty());
        assert!(ensure_clean(&path).is_err());
        std::fs::remove_dir(&backup)?;
        recover(&path)?;
        ensure_clean(&path)?;
        assert_eq!(Config::load(&path)?.settings.theme, "dark");
        Ok(())
    }
}
