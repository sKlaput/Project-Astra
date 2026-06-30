
// ---------------------------------------------------------------------------
// Embedded "hello from elf" user program
//
// A complete, valid ELF64 static executable (171 = 0xAB bytes).
//
// Layout
//   [0x00..0x3F] ELF64 header           (64 bytes)
//   [0x40..0x77] PT_LOAD program header  (56 bytes)
//   [0x78..0xAA] code + inline data      (51 bytes)
//
// The single PT_LOAD segment maps the full file at p_vaddr = 0x400000
// with flags PF_R|PF_X (read + execute, no write).
// Entry point e_entry = 0x400078 (immediately after the two headers).
//
// x86_64 machine code (at virtual address 0x400078):
//
//   +0x00  48 C7 C0 13 00 00 00   mov rax, 19  (SYS_WRITE_CONSOLE)
//   +0x07  48 8D 3D 16 00 00 00   lea rdi, [rip+0x16]   → 0x40009C ("hello from elf\n")
//   +0x0E  48 C7 C6 0F 00 00 00   mov rsi, 15           (message length)
//   +0x15  0F 05                  syscall
//   +0x17  48 C7 C0 15 00 00 00   mov rax, 21  (SYS_EXIT)
//   +0x1E  48 31 FF               xor rdi, rdi
//   +0x21  0F 05                  syscall
//   +0x23  F4                     hlt          (unreachable; ring-3 #GP → kills task)
//   +0x24  "hello from elf\n"     (15 bytes)
//
// Displacement proof:
//   RIP after `lea` instruction = 0x400078 + 0x07 + 0x07 = 0x400086
//   message address              = 0x400078 + 0x24 = 0x40009C
//   displacement                 = 0x40009C − 0x400086 = 0x16 ✓
// ---------------------------------------------------------------------------
#[rustfmt::skip]
pub static HELLO_ELF: &[u8] = &[
    // ---- ELF64 Header (64 bytes, offset 0x00) ----
    0x7F, 0x45, 0x4C, 0x46,                         // magic: \x7fELF
    0x02,                                            // EI_CLASS:   ELFCLASS64
    0x01,                                            // EI_DATA:    ELFDATA2LSB
    0x01,                                            // EI_VERSION: 1
    0x00,                                            // EI_OSABI:   ELFOSABI_NONE
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // e_ident padding
    0x02, 0x00,                                      // e_type:      ET_EXEC
    0x3E, 0x00,                                      // e_machine:   EM_X86_64 (62)
    0x01, 0x00, 0x00, 0x00,                          // e_version:   1
    0x78, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, // e_entry:     0x400078
    0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // e_phoff:     64  (0x40)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // e_shoff:     0   (no sections)
    0x00, 0x00, 0x00, 0x00,                          // e_flags:     0
    0x40, 0x00,                                      // e_ehsize:    64
    0x38, 0x00,                                      // e_phentsize: 56
    0x01, 0x00,                                      // e_phnum:     1
    0x40, 0x00,                                      // e_shentsize: 64
    0x00, 0x00,                                      // e_shnum:     0
    0x00, 0x00,                                      // e_shstrndx:  0

    // ---- PT_LOAD Program Header (56 bytes, offset 0x40) ----
    0x01, 0x00, 0x00, 0x00,                          // p_type:   PT_LOAD
    0x05, 0x00, 0x00, 0x00,                          // p_flags:  PF_R | PF_X
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_offset: 0  (load from file start)
    0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, // p_vaddr:  0x400000
    0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, // p_paddr:  0x400000
    0xAB, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_filesz: 171 (full file)
    0xAB, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_memsz:  171
    0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_align:  0x1000

    // ---- Code + inline data (51 bytes, offset 0x78) ----
    // mov rax, 19  (SYS_WRITE_CONSOLE)
    0x48, 0xC7, 0xC0, 0x13, 0x00, 0x00, 0x00,
    // lea rdi, [rip + 0x16]  →  points at "hello from elf\n" below
    0x48, 0x8D, 0x3D, 0x16, 0x00, 0x00, 0x00,
    // mov rsi, 15  (message length)
    0x48, 0xC7, 0xC6, 0x0F, 0x00, 0x00, 0x00,
    // syscall
    0x0F, 0x05,
    // mov rax, 21  (SYS_EXIT)
    0x48, 0xC7, 0xC0, 0x15, 0x00, 0x00, 0x00,
    // xor rdi, rdi
    0x48, 0x31, 0xFF,
    // syscall
    0x0F, 0x05,
    // hlt  (unreachable fallback; ring-3 #GP → abort_current_user_task_from_fault)
    0xF4,
    // "hello from elf\n"
    0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x66, 0x72,
    0x6F, 0x6D, 0x20, 0x65, 0x6C, 0x66, 0x0A,
];

