# E1 Boot Evidence

## Environment

- Host OS: Windows
- Emulator: QEMU 10.2.0
- Firmware: `edk2-x86_64-code.fd`
- Boot path: UEFI -> Limine -> kernel (`kmain`)
- Build mode: debug

## Observed Serial Output

From `scripts/run-qemu.ps1`:

```text
BdsDxe: loading Boot0001 "UEFI QEMU HARDDISK QM00001 " from PciRoot(0x0)/Pci(0x1F,0x2)/Sata(0x0,0xFFFF,0x0)
BdsDxe: starting Boot0001 "UEFI QEMU HARDDISK QM00001 " from PciRoot(0x0)/Pci(0x1F,0x2)/Sata(0x0,0xFFFF,0x0)
kernel: boot entry reached
kernel: phase E1 skeleton active
boot: limine handoff active
boot: framebuffer response present
```

## Interpretation

- Limine loaded the kernel entry point successfully.
- Kernel serial logging is active from early boot.
- Limine framebuffer request returned a response.

## Remaining E1 Work

- Replace framebuffer stub with real text output.
- Add bootloader and firmware info logging from Limine responses.
- Stabilize image/run scripts for repeatable CI-style invocation.