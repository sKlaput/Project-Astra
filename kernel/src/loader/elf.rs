
// ---------------------------------------------------------------------------
// Load error type
// ---------------------------------------------------------------------------
#[derive(Debug)]
pub enum LoadError {
    TooShort,
    InvalidMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedType,
    UnsupportedMachine,
    InvalidProgramHeader,
    UnsupportedSegmentLayout,
    SegmentOutOfBounds,
    AllocFailed,
    MapFailed,
}

// ---------------------------------------------------------------------------
// ELF loader
// ---------------------------------------------------------------------------

/// Load a static ELF64 binary, mapping its PT_LOAD segments into the active
/// page tables as user-accessible pages.
///
/// This loader intentionally supports only simple page-aligned ET_EXEC images.
/// Truncated segments or unsupported PT_LOAD layouts are rejected instead of
/// being repaired implicitly.
///
/// Returns the entry-point virtual address (`e_entry`) on success.
///
/// # Safety
/// The virtual address ranges claimed by PT_LOAD segments must not overlap
/// existing mappings, and no concurrent page-table mutation may occur.
pub fn load_elf(bytes: &[u8]) -> Result<u64, LoadError> {
    let pml4_phys = crate::memory::paging::current_cr3_phys();
    load_elf_into_pml4(bytes, pml4_phys)
}

/// Load an ELF image into the provided page-table root.
///
/// Returns the entry-point virtual address (`e_entry`) on success.
pub fn load_elf_into_pml4(bytes: &[u8], pml4_phys: usize) -> Result<u64, LoadError> {
    use crate::memory::frame_allocator::allocate_frame;
    use crate::memory::paging::{
        hhdm_offset, is_user_range, is_user_virt, map_page_in_pml4, PageTableFlags, PAGE_SIZE,
    };

    #[derive(Clone, Copy)]
    struct SegmentRange {
        start: usize,
        end: usize,
    }

    fn checked_end(start: usize, len: usize) -> Result<usize, LoadError> {
        start.checked_add(len).ok_or(LoadError::SegmentOutOfBounds)
    }

    let mut mapped_ranges = [SegmentRange { start: 0, end: 0 }; MAX_LOAD_SEGS];
    let mut mapped_range_count = 0usize;

    // ---- Validate ELF header ----
    if bytes.len() < 64 {
        return Err(LoadError::TooShort);
    }
    if bytes[0] != 0x7F || bytes[1] != b'E' || bytes[2] != b'L' || bytes[3] != b'F' {
        return Err(LoadError::InvalidMagic);
    }
    if bytes[4] != 2 {
        return Err(LoadError::UnsupportedClass); // not ELFCLASS64
    }
    if bytes[5] != 1 {
        return Err(LoadError::UnsupportedEndian); // not little-endian
    }

    let e_type = u16::from_le_bytes([bytes[16], bytes[17]]);
    let e_machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    if e_type != ET_EXEC {
        return Err(LoadError::UnsupportedType);
    }
    if e_machine != EM_X86_64 {
        return Err(LoadError::UnsupportedMachine);
    }

    // SAFETY: length >= 64 checked above; all index ranges are within header.
    let e_entry = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
    let e_phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes([bytes[54], bytes[55]]) as usize;
    let e_phnum = u16::from_le_bytes([bytes[56], bytes[57]]) as usize;

    if e_phentsize < 56 {
        return Err(LoadError::InvalidProgramHeader);
    }
    if !is_user_virt(e_entry as usize) {
        return Err(LoadError::UnsupportedSegmentLayout);
    }

    let segs = e_phnum.min(MAX_LOAD_SEGS);
    let ph_table_bytes = segs
        .checked_mul(e_phentsize)
        .ok_or(LoadError::InvalidProgramHeader)?;
    let ph_table_end = e_phoff
        .checked_add(ph_table_bytes)
        .ok_or(LoadError::InvalidProgramHeader)?;
    if ph_table_end > bytes.len() {
        return Err(LoadError::InvalidProgramHeader);
    }

    // ---- Map each PT_LOAD segment ----
    for i in 0..segs {
        let ph_start = e_phoff + i * e_phentsize;
        if ph_start + 56 > bytes.len() {
            return Err(LoadError::InvalidProgramHeader);
        }
        let ph = &bytes[ph_start..ph_start + 56];

        let p_type = u32::from_le_bytes([ph[0], ph[1], ph[2], ph[3]]);
        if p_type != PT_LOAD {
            continue;
        }

        let p_flags = u32::from_le_bytes([ph[4], ph[5], ph[6], ph[7]]);
        let p_offset = u64::from_le_bytes(ph[8..16].try_into().unwrap()) as usize;
        let p_vaddr = u64::from_le_bytes(ph[16..24].try_into().unwrap()) as usize;
        let p_filesz = u64::from_le_bytes(ph[32..40].try_into().unwrap()) as usize;
        let p_memsz = u64::from_le_bytes(ph[40..48].try_into().unwrap()) as usize;
        let p_align = u64::from_le_bytes(ph[48..56].try_into().unwrap()) as usize;

        if p_memsz < p_filesz {
            return Err(LoadError::UnsupportedSegmentLayout);
        }
        if p_memsz == 0 {
            continue;
        }
        if p_vaddr % PAGE_SIZE != 0 || p_offset % PAGE_SIZE != 0 {
            return Err(LoadError::UnsupportedSegmentLayout);
        }
        if p_align != 0 && p_align != 1 && p_align != PAGE_SIZE {
            return Err(LoadError::UnsupportedSegmentLayout);
        }

        let virt_end = checked_end(p_vaddr, p_memsz)?;
        if !is_user_range(p_vaddr, p_memsz) {
            return Err(LoadError::UnsupportedSegmentLayout);
        }
        for existing in mapped_ranges.iter().take(mapped_range_count) {
            let overlaps = p_vaddr < existing.end && existing.start < virt_end;
            if overlaps {
                return Err(LoadError::UnsupportedSegmentLayout);
            }
        }
        if mapped_range_count < MAX_LOAD_SEGS {
            mapped_ranges[mapped_range_count] = SegmentRange {
                start: p_vaddr,
                end: virt_end,
            };
            mapped_range_count += 1;
        }

        let file_end = checked_end(p_offset, p_filesz)?;
        if file_end > bytes.len() {
            return Err(LoadError::SegmentOutOfBounds);
        }

        let num_pages = p_memsz
            .checked_add(PAGE_SIZE - 1)
            .ok_or(LoadError::UnsupportedSegmentLayout)?
            / PAGE_SIZE;
        let writable = (p_flags & PF_W) != 0;
        let executable = (p_flags & PF_X) != 0;

        // Enforce W^X: reject any PT_LOAD that is both writable and executable.
        if writable && executable {
            return Err(LoadError::UnsupportedSegmentLayout);
        }

        for page_idx in 0..num_pages {
            let frame = allocate_frame().ok_or(LoadError::AllocFailed)?;
            // Track this frame as owned by the user process for later teardown.
            crate::memory::user_frames::register(pml4_phys as u64, frame.start_address() as u64);

            let mut flag_bits = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
            if writable {
                flag_bits |= PageTableFlags::WRITABLE;
            }
            if !executable {
                flag_bits |= PageTableFlags::EXECUTE_DISABLE;
            }
            let flags = PageTableFlags::new(flag_bits);

            let virt_page = p_vaddr
                .checked_add(
                    page_idx
                        .checked_mul(PAGE_SIZE)
                        .ok_or(LoadError::UnsupportedSegmentLayout)?,
                )
                .ok_or(LoadError::UnsupportedSegmentLayout)?;

            // SAFETY: caller guarantees no overlap with existing mappings.
            unsafe {
                map_page_in_pml4(pml4_phys, virt_page, frame.start_address(), flags)
                    .map_err(|_| LoadError::MapFailed)?;
            }

            // Copy file data into the frame via the HHDM.
            // SAFETY: frame was just allocated; HHDM+phys gives a valid writable pointer.
            let frame_hhdm = frame.start_address() + hhdm_offset();
            let dest = unsafe { core::slice::from_raw_parts_mut(frame_hhdm as *mut u8, PAGE_SIZE) };

            // Zero-fill first (handles the BSS region: memsz > filesz).
            dest.fill(0);

            // Copy the file-backed portion for this page.
            let seg_byte_start = page_idx * PAGE_SIZE;
            if seg_byte_start < p_filesz {
                let file_src_start = checked_end(p_offset, seg_byte_start)?;
                let copy_len = (p_filesz - seg_byte_start).min(PAGE_SIZE);
                let file_src_end = checked_end(file_src_start, copy_len)?;
                if file_src_end > bytes.len() {
                    return Err(LoadError::SegmentOutOfBounds);
                }
                dest[..copy_len].copy_from_slice(&bytes[file_src_start..file_src_end]);
            }
        }
    }

    Ok(e_entry)
}
