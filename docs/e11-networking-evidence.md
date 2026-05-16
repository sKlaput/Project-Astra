# E11 Networking Evidence

## Slice 1: Networking Scaffold (v0)

Date: 2026-04-05

## Implemented Subset

- Added networking scaffold module at `kernel/src/net/mod.rs` with interface groups:
  - `net::driver`
  - `net::stack`
  - `net::socket`
  - `net::service`
- Added boot probe in `kernel/src/main.rs`:
  - `probe_network_scaffold_v0()`
  - serial markers:
    - `net: scaffold ...`
    - `net: scaffold PASS|FAIL`
- Added feature gate in `kernel/Cargo.toml`:
  - `net-scaffold = []`

## Build Sanity (Enabled/Disabled)

Commands executed:

```powershell
cargo check -Z build-std=core,alloc -p kernel
cargo check -Z build-std=core,alloc -p kernel --features net-scaffold
```

Result:
- both checks finished successfully (warnings only).

## Focused Runtime Evidence

Command:

```powershell
.\scripts\run-qemu.ps1 -LogPath build/e11-net-slice1.log -TimeoutSeconds 35 -CargoFeatures net-scaffold
```

Expected timeout behavior:
- QEMU run is timeout-bounded in this workflow.

Observed markers in `build/e11-net-slice1.log`:
- `net: scaffold drv=1 link=1 tx=1 rx=1 ingest=1`
- `net: scaffold PASS`
- no `FAIL` markers found for the networking scaffold path.

## Regression Gate

Command:

```powershell
.\scripts\validate-e9-gate.ps1 -OutPrefix build/e9-gate-e11-slice1
```

Summary:
- `build/e9-gate-e11-slice1-summary.txt`: `E9 Tripwire Summary: PASS`
- stable lane: PASS
- user-deep lane: PASS
- kernel-deep lane (blocking): PASS

## Outcome

E11 Slice 1 networking scaffold is integrated, build-sane with feature enabled/disabled, emits deterministic runtime probe markers, and preserves strict all-lane regression gate green.

## Slice 2: UDP Socket Lifecycle (v0)

Date: 2026-04-05

## Implemented Subset

- Extended `net::socket` in `kernel/src/net/mod.rs` with a fixed-capacity state model:
  - states: `Closed`, `Created`, `Bound`, `Connected`
  - explicit lifecycle checks in `bind/connect/send/recv/close`
  - deterministic `stats()` for open/bound/connected socket counts
- Extended `probe_network_scaffold_v0()` in `kernel/src/main.rs` with lifecycle validation:
  - invalid transition checks (send before connect, invalid bind)
  - unsupported domain check
  - serial marker: `net: udp-lifecycle PASS|FAIL`

## Build Sanity

Commands executed:

```powershell
cargo check -Z build-std=core,alloc -p kernel
cargo check -Z build-std=core,alloc -p kernel --features net-scaffold
```

Result:
- both checks finished successfully.

## Focused Runtime Evidence

Command:

```powershell
.\scripts\run-qemu.ps1 -LogPath build/e11-net-slice2.log -TimeoutSeconds 35 -CargoFeatures net-scaffold
```

Observed markers in `build/e11-net-slice2.log`:
- `net: scaffold drv=1 link=1 tx=1 rx=1 ingest=1 sockets(open,bound,connected)=0,0,0`
- `net: scaffold PASS`
- `net: udp-lifecycle PASS`

## Regression Gate

Command:

```powershell
.\scripts\validate-e9-gate.ps1 -OutPrefix build/e9-gate-e11-slice2
```

Summary:
- `build/e9-gate-e11-slice2-summary.txt`: `E9 Tripwire Summary: PASS`
- stable lane: PASS
- user-deep lane: PASS
- kernel-deep lane (blocking): PASS

## Outcome

E11 Slice 2 establishes a deterministic UDP socket lifecycle state machine and probe coverage while preserving strict all-lane gate stability.

## Slice 3: DHCP and DNS Hook Integration (v0)

Date: 2026-04-05

## Implemented Subset

- Extended `net::service` in `kernel/src/net/mod.rs` with a minimal network config store:
  - DHCP states: `Idle`, `Discovering`, `Bound`
  - deterministic lease assignment through `dhcp_start()` + `dhcp_tick()`
  - renewal path via `dhcp_renew()`
  - config inspection via `network_config()`
- Updated DNS hook to resolve from active config:
  - `kernel.local` -> configured interface IPv4
  - `resolver.local` -> configured DNS server IPv4
- Extended `probe_network_scaffold_v0()` in `kernel/src/main.rs`:
  - validates DHCP start/bind/renew and expected address triplet
  - validates DNS resolution through hook path
  - emits marker `net: hooks PASS|FAIL`

## Build Sanity

Commands executed:

```powershell
cargo check -Z build-std=core,alloc -p kernel
cargo check -Z build-std=core,alloc -p kernel --features net-scaffold
```

Result:
- both checks finished successfully.

## Focused Runtime Evidence

Command:

```powershell
.\scripts\run-qemu.ps1 -LogPath build/e11-net-slice3.log -TimeoutSeconds 35 -CargoFeatures net-scaffold
```

Observed markers in `build/e11-net-slice3.log`:
- `net: scaffold drv=1 link=1 tx=1 rx=1 ingest=1 sockets(open,bound,connected)=0,0,0 dhcp(addr,gw,dns,lease,bound)=10.0.2.15,10.0.2.2,1.1.1.1,300,1`
- `net: scaffold PASS`
- `net: udp-lifecycle PASS`
- `net: hooks PASS`

## Regression Gate

Command:

```powershell
.\scripts\validate-e9-gate.ps1 -OutPrefix build/e9-gate-e11-slice3
```

Summary:
- `build/e9-gate-e11-slice3-summary.txt`: `E9 Tripwire Summary: PASS`
- stable lane: PASS
- user-deep lane: PASS
- kernel-deep lane (blocking): PASS

## Outcome

E11 Slice 3 integrates DHCP and DNS service hooks with deterministic probe coverage and keeps strict all-lane regression gates green.

## Slice 4: Firewall Rule Plumbing (v0)

Date: 2026-04-05

## Implemented Subset

- Extended `net::service` in `kernel/src/net/mod.rs` with firewall mode and counters:
  - modes: `AllowAll`, `BlockUdp`
  - rule control: `firewall_set_udp_block(enabled)`
  - telemetry: `firewall_stats()` for ingress/egress allow/deny counts
- Updated `firewall_decide()` to enforce mode-aware ingress/egress decisions:
  - UDP denied when `BlockUdp` is enabled
  - TCP remains allowed in v0 policy
- Extended `probe_network_scaffold_v0()` in `kernel/src/main.rs`:
  - validates allow-all baseline decisions
  - validates deny behavior for UDP ingress and egress under block mode
  - validates TCP remains allowed under block mode
  - emits marker `net: firewall PASS|FAIL`

## Build Sanity

Commands executed:

```powershell
cargo check -Z build-std=core,alloc -p kernel
cargo check -Z build-std=core,alloc -p kernel --features net-scaffold
```

Result:
- both checks finished successfully.

## Focused Runtime Evidence

Command:

```powershell
.\scripts\run-qemu.ps1 -LogPath build/e11-net-slice4.log -TimeoutSeconds 35 -CargoFeatures net-scaffold
```

Observed markers in `build/e11-net-slice4.log`:
- `net: scaffold drv=1 link=1 tx=1 rx=1 ingest=1 sockets(open,bound,connected)=0,0,0 dhcp(addr,gw,dns,lease,bound)=10.0.2.15,10.0.2.2,1.1.1.1,300,1 fw(ai,di,ae,de,udp_block)=2,1,1,1,1`
- `net: scaffold PASS`
- `net: udp-lifecycle PASS`
- `net: hooks PASS`
- `net: firewall PASS`

## Regression Gate

Command:

```powershell
.\scripts\validate-e9-gate.ps1 -OutPrefix build/e9-gate-e11-slice4
```

Summary:
- `build/e9-gate-e11-slice4-summary.txt`: `E9 Tripwire Summary: PASS`
- stable lane: PASS
- user-deep lane: PASS
- kernel-deep lane (blocking): PASS

## Outcome

E11 Slice 4 adds explicit firewall rule plumbing and ingress/egress decision validation while preserving strict all-lane gate stability.

## Slice 5: Integration Sweep and Contract Packaging

Date: 2026-04-05

## Implemented Subset

- Added integrated contract marker in `probe_network_scaffold_v0()`:
  - `net: e11-contract PASS|FAIL`
- Added focused validator script:
  - `scripts/validate-e11-networking.ps1`
  - executes a net-scaffold focused run
  - enforces required marker set
  - emits text and JSON summaries

Required marker contract:
- `net: scaffold PASS`
- `net: udp-lifecycle PASS`
- `net: hooks PASS`
- `net: firewall PASS`
- `net: e11-contract PASS`

## Build and Focused Validation

Commands executed:

```powershell
cargo check -Z build-std=core,alloc -p kernel --features net-scaffold
.\scripts\validate-e11-networking.ps1 -OutPrefix build/e11-validate-slice5-rerun
```

Focused summary:
- `build/e11-validate-slice5-rerun-summary.txt`: `E11 Networking Validation: PASS`
- `build/e11-validate-slice5-rerun-summary.json`: `result = PASS`

## Regression Gate

Command:

```powershell
.\scripts\validate-e9-gate.ps1 -OutPrefix build/e9-gate-e11-slice5
```

Summary:
- `build/e9-gate-e11-slice5-summary.txt`: `E9 Tripwire Summary: PASS`
- stable lane: PASS
- user-deep lane: PASS
- kernel-deep lane (blocking): PASS

## Outcome

E11 Slice 5 packages networking validation into a reusable contract gate with machine-readable summary output while maintaining strict all-lane stability.
