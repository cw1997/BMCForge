//! Hardware evidence is independent of fan command eligibility.
use anyhow::{Context, Result, ensure};
use bmcforge_core::DeviceIdentity;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub vendor: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub manufacturer_ids: Vec<u32>,
    #[serde(default)]
    pub product_ids: Vec<u16>,
    #[serde(default)]
    pub machine_types: Vec<String>,
    pub profile: Option<String>,
    pub sources: Vec<String>,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogFile {
    pub schema_version: u32,
    #[serde(default)]
    pub override_builtin: bool,
    pub models: Vec<Model>,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub id: String,
    pub evidence: Vec<String>,
    pub explicit: bool,
}
pub struct Catalog {
    pub models: BTreeMap<String, Model>,
}
fn normalized(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
impl Catalog {
    pub fn load(dir: Option<&Path>) -> Result<Self> {
        let mut catalog = Self {
            models: BTreeMap::new(),
        };
        catalog.merge(include_str!("../../../profiles/models/dell.toml"), false)?;
        catalog.merge(include_str!("../../../profiles/models/lenovo.toml"), false)?;
        for text in [
            include_str!("../../../profiles/models/hpe.toml"),
            include_str!("../../../profiles/models/lenovo-thinksystem.toml"),
            include_str!("../../../profiles/models/inspur.toml"),
            include_str!("../../../profiles/models/h3c.toml"),
            include_str!("../../../profiles/models/huawei.toml"),
            include_str!("../../../profiles/models/xfusion.toml"),
            include_str!("../../../profiles/models/sugon.toml"),
            include_str!("../../../profiles/models/zte.toml"),
            include_str!("../../../profiles/models/greatwall.toml"),
        ] {
            catalog.merge(text, false)?;
        }
        if let Some(dir) = dir.map(|p| p.join("models")).filter(|p| p.exists()) {
            let mut paths = std::fs::read_dir(dir)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            paths.sort();
            let mut seen = std::collections::BTreeSet::new();
            for path in paths
                .into_iter()
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            {
                let text = std::fs::read_to_string(&path)?;
                let file: CatalogFile =
                    toml::from_str(&text).with_context(|| path.display().to_string())?;
                for model in &file.models {
                    ensure!(
                        seen.insert(model.id.clone()),
                        "duplicate external model {}",
                        model.id
                    );
                }
                catalog
                    .merge(&text, true)
                    .with_context(|| path.display().to_string())?;
            }
        }
        Ok(catalog)
    }
    fn merge(&mut self, text: &str, external: bool) -> Result<()> {
        let file: CatalogFile = toml::from_str(text)?;
        ensure!(file.schema_version == 1, "unsupported model schema");
        for model in file.models {
            ensure!(
                !model.id.is_empty()
                    && !model.name.is_empty()
                    && !model.manufacturer_ids.is_empty(),
                "incomplete model"
            );
            ensure!(
                !model.sources.is_empty()
                    && model.sources.iter().all(|s| s.starts_with("https://")),
                "model requires HTTPS sources"
            );
            ensure!(
                !self.models.contains_key(&model.id) || (external && file.override_builtin),
                "model {} requires explicit override_builtin",
                model.id
            );
            self.models.insert(model.id.clone(), model);
        }
        Ok(())
    }
    pub fn candidates(&self, identity: &DeviceIdentity) -> Vec<Candidate> {
        let raw = normalized(&identity.model);
        let fields: Vec<_> = identity
            .fru
            .iter()
            .filter(|s| s.starts_with("Product[1]:") || s.starts_with("Product[2]:"))
            .chain(&identity.machine_types)
            .map(|s| s.to_lowercase())
            .collect();
        let mut matches = vec![];
        for m in self.models.values() {
            if !m.manufacturer_ids.contains(&identity.manufacturer_id) {
                continue;
            }
            let exact = !raw.is_empty()
                && std::iter::once(&m.name)
                    .chain(&m.aliases)
                    .any(|s| normalized(s) == raw);
            let mt = m.machine_types.iter().find(|mt| {
                let mt = mt.to_lowercase();
                fields.iter().any(|s| {
                    s.split(|c: char| !c.is_ascii_alphanumeric()).any(|part| {
                        part == mt.as_str()
                            || (part.starts_with(mt.as_str()) && part.len() == mt.len() + 3)
                    })
                })
            });
            let product = m.product_ids.contains(&identity.product_id);
            if exact || mt.is_some() || product {
                let mut evidence = vec![];
                if exact {
                    evidence.push(format!("FRU: {}", identity.model));
                }
                if let Some(mt) = mt {
                    evidence.push(format!("Machine type: {mt}"));
                }
                if product {
                    evidence.push(format!(
                        "Product ID: {:#06x} (may be shared)",
                        identity.product_id
                    ));
                }
                matches.push(Candidate {
                    id: m.id.clone(),
                    evidence,
                    explicit: exact || mt.is_some(),
                });
            }
        }
        // Conflicting explicit names and machine types remain ambiguous.
        if matches.iter().any(|c| c.explicit) {
            matches.retain(|c| c.explicit);
        }
        matches
    }
    pub fn schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(CatalogFile)).expect("schema")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_machine_types_conflicts_and_unknown() -> Result<()> {
        let c = Catalog::load(None)?;
        let mut d = DeviceIdentity {
            manufacturer_id: 19046,
            model: "IBM System x3650 M5".into(),
            ..Default::default()
        };
        assert_eq!(c.candidates(&d)[0].id, "lenovo-x3650-m5");
        d.machine_types = vec!["5463-ABC".into()];
        assert_eq!(c.candidates(&d).len(), 2);
        d.model.clear();
        d.machine_types.clear();
        assert!(c.candidates(&d).is_empty());
        d.manufacturer_id = 674;
        d.model = "DELL PowerEdge R210 II".into();
        assert!(c.models[&c.candidates(&d)[0].id].profile.is_none());
        Ok(())
    }
    #[test]
    fn shared_product_ids_require_resolution() -> Result<()> {
        let mut c = Catalog::load(None)?;
        for id in ["dell-r730", "dell-r720"] {
            c.models.get_mut(id).unwrap().product_ids = vec![100];
        }
        let mut d = DeviceIdentity {
            manufacturer_id: 674,
            product_id: 100,
            ..Default::default()
        };
        let choices = c.candidates(&d);
        assert_eq!(choices.len(), 2);
        assert!(choices.iter().all(|c| !c.explicit));
        d.model = "Dell PowerEdge R730".into();
        assert_eq!(c.candidates(&d).len(), 1);
        assert!(c.candidates(&d)[0].explicit);
        Ok(())
    }
    #[test]
    fn mainstream_vendor_aliases_match_only_with_the_expected_manufacturer() -> Result<()> {
        let c = Catalog::load(None)?;
        for (manufacturer, raw, expected) in [
            (11, "HP ProLiant DL380 Gen9", "hpe-proliant-dl380-gen9"),
            (
                19046,
                "Lenovo ThinkSystem SR650 V3",
                "lenovo-thinksystem-sr650-v3",
            ),
            (37945, "浪潮 NF5280M5", "inspur-nf5280m5"),
            (25506, "H3C UniServer R4900 G6", "h3c-uniserver-r4900-g6"),
            (2011, "华为 RH2288 V3", "huawei-rh2288-v3"),
            (2011, "Huawei 2288H V5", "xfusion-fusionserver-2288h-v5"),
            (58132, "xFusion 2288H V7", "xfusion-fusionserver-2288h-v7"),
            (57098, "曙光 I620-G30", "sugon-i620-g30"),
            (3902, "中兴 R5300 G5", "zte-r5300-g5"),
            (58838, "长城擎天EF860", "greatwall-ef860"),
        ] {
            let mut d = DeviceIdentity {
                manufacturer_id: manufacturer,
                model: raw.into(),
                ..Default::default()
            };
            assert_eq!(
                c.candidates(&d)
                    .iter()
                    .map(|c| c.id.as_str())
                    .collect::<Vec<_>>(),
                [expected],
                "{raw}"
            );
            d.manufacturer_id = 674;
            assert!(
                c.candidates(&d).is_empty(),
                "must not guess vendor for {raw}"
            );
        }
        Ok(())
    }
    #[test]
    fn thinksystem_alphanumeric_machine_types_and_shared_types_are_not_guessed() -> Result<()> {
        let c = Catalog::load(None)?;
        let mut d = DeviceIdentity {
            manufacturer_id: 19046,
            machine_types: vec!["7d75abc".into()],
            ..Default::default()
        };
        assert_eq!(c.candidates(&d)[0].id, "lenovo-thinksystem-sr650-v3");
        d.machine_types = vec!["7DGC".into()];
        assert_eq!(c.candidates(&d).len(), 2);
        d.machine_types = vec!["7DG".into()];
        assert!(c.candidates(&d).is_empty());
        Ok(())
    }
}
