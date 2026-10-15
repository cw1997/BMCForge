"""Regenerate the compatibility inventory from reviewed profiles and candidates."""
import json
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PENDING = [
    ("ASRock AST2500 percentage / older interfaces", "https://download.asrock.com/Rack/TSD/FAQ/TSDQA-72.pdf", "Separate conflicting model/firmware interfaces before enabling writes"),
    ("Supermicro X10QBi", "https://github.com/petersulyok/smfc/blob/main/ARCHITECTURE.md", "Nuvoton TMFR register setup and 0x5c interface"),
    ("Supermicro X14/H14", "https://github.com/petersulyok/smfc/blob/main/doc/X14H14_MANUAL_FANCONTROL.md", "OpenBMC versus ATEN stack discovery, per-stack release and zone discovery"),
    ("HPE iLO", "https://github.com/ST-DEVPT/rack-fan-control", "Control may need modified firmware or SSH, outside this version's protocol scope"),
    ("Inspur", "https://en.inspur.com/eportal//fileDir/active_download/platformBookZh/6680/Inspur%C2%A0Server%C2%A0BMC%C2%A0User%C2%A0Manual%C2%A0V2.7.pdf", "Vendor documents fan settings; full IPMI write/restore sequence not established here"),
]


def catalog():
    entries = []
    profiles = {}
    for file in sorted((ROOT / "profiles").glob("*.toml")):
        profile = tomllib.loads(file.read_text(encoding="utf-8"))
        profiles[profile['id']] = profile
        entries.append({"profile": profile["id"], "vendor": profile["vendor"], "description": profile["description"], "status": profile["verification"], "control": "fan" in profile, "matcher": profile["matcher"], "sources": profile["sources"], "constraints": profile["notes"]})
    entries.extend({"family": family, "status": "pending", "sources": [source], "constraints": constraints} for family, source, constraints in PENDING)
    models = []
    for file in sorted((ROOT / "profiles/models").glob("*.toml")):
        for model in tomllib.loads(file.read_text(encoding="utf-8"))['models']:
            profile_id = model.get('profile')
            profile = profiles.get(profile_id, {})
            models.append({**model, "identification": "catalogued_not_hardware_verified",
                           "fan_adapter": profile_id if "fan" in profile else None,
                           "control_status": profile.get('verification') if 'fan' in profile else 'monitoring_only'})
    return {"reviewed": "2026-10-16", "hardware_verified_by_bmcforge": False, "entries": entries,
            "model_count": len(models), "models": sorted(models, key=lambda m: m['id'])}


if __name__ == "__main__":
    (ROOT / "docs/compatibility.json").write_text(json.dumps(catalog(), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
