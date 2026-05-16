# E11 Networking Architecture Note

## Purpose

This note defines the networking architecture direction for E11 without introducing a full network stack implementation yet.

## Scope

In scope:
- networking subsystem interfaces
- socket API direction
- explicit IPv4-first prioritization
- DNS/DHCP/firewall hook points
- architecture sanity checks for any implemented subset

Out of scope:
- full NIC driver stack
- full TCP/IP implementation
- production firewall policy engine
- user-facing network manager UI

## Layer Model

Planned layered model:

1. Link layer
- NIC driver interface (tx/rx queues, link state, MTU)
- packet buffers managed by kernel net buffer pool

2. Network layer
- IPv4 parser/serializer
- ARP cache and resolution path
- routing table (single default route first)

3. Transport layer
- UDP first for service bring-up (DHCP, DNS)
- TCP introduced after UDP path is stable

4. Socket layer
- syscall-facing socket abstraction
- per-socket state machine and buffers

5. Service hooks
- DNS resolver hook
- DHCP client hook
- firewall decision hook

## Interface Direction

Kernel-facing interface groups:

- `net::driver`
  - register_driver
  - set_link_up/down callback
  - submit_tx_frame
  - poll_rx_frame

- `net::stack`
  - ingest_frame
  - process_tick
  - route_packet
  - emit_frame

- `net::socket`
  - create
  - bind
  - connect
  - listen/accept (later)
  - send/recv
  - close

- `net::service`
  - dns_resolve
  - dhcp_start
  - dhcp_renew
  - firewall_decide

## Socket API Direction

Direction: BSD-like socket model with a reduced v0 syscall subset.

Planned v0 syscall shape:
- socket(domain, type, protocol) -> fd/handle
- bind(handle, addr)
- connect(handle, addr)
- send(handle, buf)
- recv(handle, buf)
- close(handle)

Domain/type priorities:
- domain: AF_INET first
- type: SOCK_DGRAM first (UDP)
- SOCK_STREAM added after UDP and socket lifecycle are stable

Error model:
- negative error returns in syscall ABI style
- deterministic non-blocking and timeout behavior documented per call

## IPv4 Priority Statement

IPv4 is the explicit first-class path for E11/E12 networking bring-up.

Implications:
- IPv4 packet path is implemented before IPv6
- DHCPv4 and DNS over IPv4 hooks are first
- route table and address model optimize for a single IPv4 interface first

IPv6 is deferred until IPv4 path is validated and documented.

## Hook Points

### DNS Hook

Purpose:
- name -> IPv4 address resolution for app/runtime callers

Architecture hook:
- resolver API in `net::service`
- transport dependency on UDP socket path
- cache placeholder permitted in v0

### DHCP Hook

Purpose:
- dynamic IPv4 lease acquisition and renewal

Architecture hook:
- client state machine ticked by scheduler/timer
- writes configured address, gateway, and DNS server into network config store

### Firewall Hook

Purpose:
- packet allow/deny decision before socket delivery and before TX emit

Architecture hook:
- decision callback at ingress and egress
- default policy: allow in v0 with explicit extension point for deny rules

## Data Flow (Planned)

Ingress:
1. NIC RX -> driver callback
2. frame handed to `net::stack::ingest_frame`
3. firewall ingress hook
4. protocol dispatch (ARP/IPv4)
5. transport dispatch (UDP/TCP)
6. socket buffer enqueue

Egress:
1. socket send call
2. transport packet build
3. IPv4 encapsulation + route lookup
4. firewall egress hook
5. NIC TX submission

## Concurrency and Scheduling Direction

- network processing remains scheduler-compatible and non-blocking by default
- packet RX polling can be integrated with existing timer/tick paths first
- long operations (e.g. DHCP retries) run as bounded state machines, not busy loops

## Security and Safety Direction

- validate packet lengths and headers before deeper parsing
- enforce ownership checks on socket handles
- avoid exposing kernel pointers to user-space networking syscalls
- bound buffer sizes and copy lengths for send/recv paths

## E11 Sanity Checks

If code is introduced during E11, minimum sanity checks are:
- stack compiles and boots with networking code enabled/disabled
- no regressions in current strict gate
- deterministic logs for at least one implemented subset path
- malformed packet handling does not crash kernel

## Staged Implementation Plan

1. Architecture-only phase (this note)
2. Net buffer + interface scaffolding
3. UDP socket lifecycle baseline
4. DHCP/DNS hook integration
5. Firewall decision plumbing
6. TCP path and richer socket semantics

## Risks and Deferrals

- risk: network parsing bugs can become kernel crash vectors
  - mitigation: strict bounds checks and staged protocol enablement
- risk: scheduler starvation from network polling
  - mitigation: bounded work per tick and explicit backpressure
- deferred: IPv6, advanced routing, NAT, TLS offload, high-throughput optimization

## Conclusion

E11 is now defined as an architecture-first networking phase with explicit interfaces, IPv4-first direction, and clear DNS/DHCP/firewall hook points, while preserving current stability and phased delivery discipline.
