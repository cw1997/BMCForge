# Opt-in hardware testing

All automated tests use loopback UDP or in-memory BMCs. CI never scans a real network or consumes real BMC credentials. `--demo` and `demo-check` do not send network traffic.

To validate a physical server:

1. Record exact chassis/board model, BMC identity, firmware, supported cipher suite and the source of the selected OEM commands. Confirm BMC automatic fan control works first.
2. Create a separate private configuration and validate it. Start monitoring, compare every fan RPM and relevant temperature/status against the BMC interface. Unknown/unavailable must remain distinct from zero.
3. Check profile bounds and zones, then explicitly enable experimental control if applicable. Start with a high duty value appropriate to the device. Verify the actual response using RPM and the BMC interface; successful command completion alone does not prove fans changed.
4. Request BMC automatic mode, confirm that automatic behavior returns, then test normal application exit after a manual request. Confirm only devices taken over by this process were restored.
5. In a controlled environment, verify timeout reporting and recovery. A missing response must leave control uncertain, without automatic replay. Restore automatic mode using a working BMC connection.
6. Submit a sanitized report with model, firmware, zone, input duty, observed RPM, command completion and restore result. Do not include credentials or serial numbers.

Do not run unsupported fan commands merely to discover whether they might work. This application does not guarantee restoration after kill, host power loss or a network outage and does not provide unattended thermal protection.
