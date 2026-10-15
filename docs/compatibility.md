# Compatibility and evidence

Inventory reviewed 2026-10-15. No adapter in this repository has been verified on physical hardware by BMCForge. Public implementation reports are evidence for experimental profiles, not a blanket compatibility claim. Machine-readable research inventory: `docs/compatibility.json`.

## Included control adapters

- `dell-idrac6-8`: selected PowerEdge 11G/12G/13G R/T models listed in the profile, whole-system duty. Community OEM commands, experimental. R210 II is deliberately excluded. Source: https://github.com/ipmitool/ipmitool/issues/30 and https://github.com/nmaggioni/r710-fan-controller.
- `dell-idrac9-legacy`: selected 14G models with a conservative pre-3.34 firmware bound, experimental. Later firmware commonly restricts the raw command. Source: https://github.com/t3chnaztea/dell-ipmi-fan-control.
- `supermicro-x10-x13`: ATEN two-zone family including corresponding H10-H13 boards, explicit selection, experimental. Does not cover X10QBi or new OpenBMC interfaces. Source: https://github.com/petersulyok/smfc.
- `supermicro-x9`: selected X9 boards using four register zones and 0..255 duty, explicit selection, experimental. Not every X9 board accepts these commands. Source: https://github.com/petersulyok/smfc.
- `asrock-ast2600`: sixteen-slot percentage interface from the vendor's technical document, explicit selection and experimental until hardware testing. Source: https://download.asrock.com/Rack/TSD/FAQ/TSDQA-72.pdf.

- `lenovo-x3650-m5-imm2`: IBM/Lenovo x3650 M5 IMM2, two independent banks with sourced non-linear raw levels and automatic recovery. Experimental; no claim about other System x models or every firmware. Source: https://github.com/laingcc/Lenovo-M3650-Nvidia-GPU-Fan-Control. See [catalog and control details](models.md).

The separate `profiles/models/` catalog recognizes 68 Dell PowerEdge and IBM/Lenovo System x models/variants, including monitoring-only entries. Recognition does not promote their fan control status.

## Known candidates requiring more work

- ASRock AST2500/X570D4U firmware 1.35: included as monitoring-only `asrock-aspeed`. The firmware uses raw 15..63 scaling and requires read-before-write slew limits. A simple percentage sequence would be incorrect. Source: https://github.com/stblassitude/asrockrack-ipmi-fan-control.
- Other ASRock AST2500 percentage and old 0x3a/0x01 interfaces: different command families; exact model/firmware and recovery behavior must be separated before enabling. Source: https://download.asrock.com/Rack/TSD/FAQ/TSDQA-72.pdf.
- Supermicro X10QBi: Nuvoton register setup differs from ordinary X9 commands. X14/H14: OpenBMC and ATEN require different manual-state discovery, release and zone logic. Sources: https://github.com/petersulyok/smfc/blob/main/ARCHITECTURE.md and https://github.com/petersulyok/smfc/blob/main/doc/X14H14_MANUAL_FANCONTROL.md.
- HPE iLO: generic IPMI monitoring is separate from fan control, which may require modified firmware or SSH and is outside this version's protocol scope. Source: https://github.com/ST-DEVPT/rack-fan-control.
- Inspur: vendor documents BMC fan controls, but a complete model-specific IPMI write/restore sequence has not been established here. Source: https://en.inspur.com/eportal//fileDir/active_download/platformBookZh/6680/Inspur%C2%A0Server%C2%A0BMC%C2%A0User%C2%A0Manual%C2%A0V2.7.pdf.
- Gigabyte, Huawei, ASUS, Fujitsu, Quanta, Intel, Cisco and other OEMs remain generic monitoring unless a reviewed external profile is supplied. This inventory makes no unsupported assertion that those vendors cannot control fans.

There is no authoritative finite list of every server/firmware that supports OEM fan writes. New reports should extend this catalog and add reproducible profiles rather than treating an unverified product name as supported.

## Protocol boundaries

IPMI 2.0 LAN suite 3: RAKP-HMAC-SHA1, HMAC-SHA1-96, AES-CBC-128. No automatic unencrypted fallback, suite 17, IPMI 1.5, local OpenIPMI device, Redfish or SSH. Unsupported cipher configuration produces an error rather than invoking a separate tool.

Full and compact BMC-local SDR sensors are read, including threshold and discrete assertions. Unsupported nonlinear OEM conversions retain raw bytes and report no numerical value. Satellite/IPMB or nonzero-LUN sensors are listed as unimplemented, not misread as BMC-local sensors. Sensor labels and FRU text support IPMI ASCII/Latin-1, BCD+ and packed 6-bit fields; vendor-specific language encodings need separate adapters.
