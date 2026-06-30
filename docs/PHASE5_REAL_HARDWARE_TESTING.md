# Phase 5: Real Hardware Testing Preparation

## Overview
Phase 5 focuses on validating Astra OS v0.3 on real hardware, identifying and fixing
issues that QEMU hides, and ensuring production readiness.

## Objectives

### 1. Hardware Compatibility
- Test on multiple x86_64 motherboards
- Verify BIOS/UEFI bootloader integration
- Check firmware interactions (SMM, ACPI)
- Validate power management

### 2. Interrupt Handling
- Verify timer interrupt behavior
- Test interrupt nesting
- Validate PIC/APIC configuration
- Check multicore interrupt routing

### 3. Memory & Paging
- Validate memory layout assumptions
- Test with different RAM configurations
- Verify guard page protections work
- Check page allocation patterns

### 4. USB & Peripheral Support
- Real USB device testing
- Keyboard hotplug events
- Mouse functionality
- Hub support

### 5. Networking Stack
- Real NIC drivers (not just virtio-net)
- Verify ethernet/IP/TCP/UDP on real hardware
- Test network performance
- Check packet handling under load

## Testing Phases

### Phase 5.1: Bootloader & Early Initialization (2 hours)
**Goal:** Verify kernel boots and initializes correctly

Test Checklist:
- ✅ BIOS boot (CSM)
- ⚠️ UEFI boot
- ⚠️ Secure Boot compatibility
- ✅ Memory detection (already works in QEMU)
- ✅ Multicore detection
- ⚠️ Real hardware timer calibration

Expected Issues to Address:
1. Bootloader firmware incompatibilities
2. Memory map variations
3. Timer frequency detection
4. Interrupt controller differences

### Phase 5.2: Core Hardware (2 hours)
**Goal:** Validate CPU, memory, and IRQ handling

Test Checklist:
- ✅ CPU feature detection
- ✅ Per-core GDT/TSS initialization
- ⚠️ Real timer interrupt behavior
- ⚠️ IRQ handling on legacy motherboards
- ⚠️ SMP/APIC behavior variations

Known Problem Areas:
1. Intel vs AMD differences
2. Older chipsets (PM, X299, etc.)
3. Interrupt routing on some BIOSes

### Phase 5.3: Input & Output (1 hour)
**Goal:** Verify keyboard, mouse, graphics work

Test Checklist:
- ✅ PS/2 keyboard input
- ✅ PS/2 mouse input
- ⚠️ USB keyboard/mouse
- ⚠️ Framebuffer graphics
- ⚠️ Display resolution detection

### Phase 5.4: Network Stack (1 hour)
**Goal:** Validate networking on real hardware

Test Checklist:
- ✅ virtio-net NIC (QEMU only)
- ⚠️ Intel 82545 NIC
- ⚠️ AMD PCnet-FAST
- ⚠️ Realtek RTL8139
- ⚠️ Ping to gateway
- ⚠️ DNS resolution

### Phase 5.5: Stability & Performance (2 hours)
**Goal:** Long-term testing and optimization

Test Checklist:
- ⚠️ 24-hour uptime test
- ⚠️ High task load (spawn 100+ tasks)
- ⚠️ Network throughput
- ⚠️ Memory pressure test
- ⚠️ Scheduler fairness metrics

## Testing Infrastructure

### Automated Tests
`ash
#!/bin/bash
# run_hardware_tests.sh - Validate hardware compatibility

test_boot_count() {
  # Try 10 boot cycles, check stability
  for i in {1..10}; do
    boot_os
    check_serial_output "kernel: idle loop entered"
    check_uptime 30  # At least 30 seconds
  done
}

test_input() {
  # Press keys, move mouse, verify events
  type_string "hello"
  verify_terminal_output "hello"
  move_mouse 100 100
  verify_mouse_packet
}

test_network() {
  # Ping to test network
  run_cmd "ping -c 5 8.8.8.8"
  verify_response "5 packets"
}
`

### Hardware Test Matrix

| Hardware | Boot | Timer | Keyboard | Mouse | Network | USB |
|----------|------|-------|----------|-------|---------|-----|
| QEMU     | ✅   | ✅    | ✅       | ✅    | ✅      | ⚠️  |
| Intel i7 | ?    | ?     | ?        | ?     | ?       | ?   |
| AMD Ryzen | ?   | ?     | ?        | ?     | ?       | ?   |
| Older PC | ?    | ?     | ?        | ?     | ?       | ?   |

Legend: ✅ = Known working, ⚠️ = Partial/Needs testing, ? = Unknown

## Potential Issues & Mitigations

### 1. Bootloader Incompatibilities
**Issue:** Some firmware doesn't load kernel correctly
**Mitigation:** 
- Test with multiple bootloaders (GRUB, limine, systemd-boot)
- Verify ELF header and program layout
- Check memory layout assumptions

### 2. Timer Calibration
**Issue:** Real hardware may have different timer speeds
**Mitigation:**
- Use multiple time sources (TSC, APIC timer, PIT)
- Validate calibration results before use
- Fallback to slower but more reliable timer

### 3. APIC Configuration
**Issue:** Some BIOSes configure APIC differently
**Mitigation:**
- Read ACPI tables for correct config
- Test both xAPIC and x2APIC modes
- Handle BIOS-disabled APIC gracefully

### 4. NIC Drivers
**Issue:** Need real NIC driver (not virtio-net)
**Mitigation:**
- Implement Intel 82545 driver (common)
- Implement RTL8139 driver (fallback)
- Keep virtio-net as QEMU-only

### 5. USB Controllers
**Issue:** XHCI may need firmware binary loading
**Mitigation:**
- Implement full XHCI driver
- Handle firmware loading for some controllers
- Fallback to PS/2 if USB unavailable

## Expected Hardware for Testing

### Minimum Hardware (for initial testing)
- **CPU:** Intel/AMD x86_64 with 2+ cores
- **RAM:** 512 MB minimum
- **Storage:** USB stick (for bootloader)
- **Input:** USB keyboard and mouse
- **Network:** Ethernet NIC
- **Display:** Any VGA/HDMI monitor

### Recommended Hardware
- **CPU:** Intel Core i5/i7 or AMD Ryzen 5/7 (recent)
- **RAM:** 4 GB+ (tests memory pressure better)
- **Storage:** SSD (faster boot)
- **Mainboard:** Current generation (UEFI support)

## Success Criteria

Phase 5 is successful when:
✅ Kernel boots on 2+ different hardware platforms
✅ Multicore scheduling works on real hardware
✅ Keyboard/mouse input functional
✅ Network communication verified (ping works)
✅ No kernel panics after 1+ hour uptime
✅ All user applications (terminal, desktop, apps) responsive

## Known Limitations for v0.3

Phase 5 will likely identify these limitations (acceptable for v0.3):
- Some old hardware may not support UEFI
- Realtek NIC driver may need implementation
- USB support still incomplete (Phase 4 issue)
- Power management not yet implemented
- Some edge cases in scheduler fairness

## Post-v0.3 Roadmap

After Phase 5 validation, v0.4 would focus on:
1. **Real NIC drivers** (Intel 82545, Realtek 8139)
2. **USB full implementation** (complete Phase 4)
3. **Power management** (CPU frequency scaling, sleep states)
4. **ACPI support** (proper hardware detection)
5. **Performance optimization** (cache-aware scheduling)

## Conclusion

Phase 5 testing is critical for production readiness. The foundation laid in Phases 0-4
is solid, but real hardware will reveal edge cases QEMU doesn't show.

Estimated time: 6-8 hours for complete validation cycle.

