# E1 Implementation Notes

## Status

Phase `E1` repository scaffolding is in place.

First successful UEFI boot has been observed in QEMU with kernel serial output. See `docs/e1-boot-evidence.md`.

Implemented so far:

- Cargo workspace
- custom x86_64 target definition
- kernel linker script
- no-std and no-main kernel entry
- serial logger for COM1
- panic handler
- Limine request declarations
- UEFI image-root builder with Limine BOOTX64.EFI
- QEMU UEFI run script using the installed edk2 firmware
- software framebuffer text renderer implementation (experimental)
- architecture module layout
- Limine configuration
- PowerShell build and run placeholders

## Known Gaps

- Runtime framebuffer activation currently causes an early reboot loop in QEMU after `boot: limine handoff active`.
- Framebuffer renderer code exists, but activation is temporarily disabled in boot init to keep the kernel stable.

## Next E1 Tasks

1. Run the QEMU path and confirm serial boot logs appear under UEFI.
2. Resolve framebuffer response parsing/runtime fault and re-enable renderer activation.
3. Add firmware and bootloader info logging from Limine responses.
4. Capture stable serial + framebuffer boot evidence.
5. Only then move into memory-management bring-up.

## Constraint

Do not move to memory management or interrupts until the kernel boots reliably and emits visible serial output through the selected boot path.