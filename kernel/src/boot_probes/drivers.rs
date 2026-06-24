use crate::{drivers, serial};

// ---------------------------------------------------------------------------
pub(crate) fn probe_driver_model() {
    use drivers::block::RamBlockDriver;
    use drivers::keyboard::Ps2KeyboardDriver;
    use drivers::{for_each, register, registered_count, DriverError};

    // Static driver instances with 'static lifetime required by the registry.
    static KB_DRIVER: Ps2KeyboardDriver = Ps2KeyboardDriver;
    static BLK_DRIVER: RamBlockDriver = RamBlockDriver;

    // Register keyboard (input category).
    let kb_ok = match register(&KB_DRIVER) {
        Ok(_) => true,
        Err(e) => {
            serial::write_str("drivers: keyboard init error=");
            serial::write_u64(e as u64);
            serial::write_line("");
            false
        }
    };

    // Register block device (block category).
    let blk_ok = match register(&BLK_DRIVER) {
        Ok(_) => {
            // Verify round-trip write/read on block 0.
            let mut wbuf = [0u8; 512];
            wbuf[0] = 0xDE;
            wbuf[1] = 0xAD;
            wbuf[2] = 0xBE;
            wbuf[3] = 0xEF;
            let write_ok = BLK_DRIVER.write_block(0, &wbuf).is_ok();
            let mut rbuf = [0u8; 512];
            let read_ok = BLK_DRIVER.read_block(0, &mut rbuf).is_ok();
            let match_ok = rbuf[0] == 0xDE && rbuf[1] == 0xAD && rbuf[2] == 0xBE && rbuf[3] == 0xEF;
            let oob_err = BLK_DRIVER.read_block(1, &mut rbuf) == Err(DriverError::OutOfRange);
            write_ok && read_ok && match_ok && oob_err
        }
        Err(e) => {
            serial::write_str("drivers: block init error=");
            serial::write_u64(e as u64);
            serial::write_line("");
            false
        }
    };

    let count = registered_count();

    // Enumerate registry and count by category.
    let mut input_count = 0usize;
    let mut block_count = 0usize;
    for_each(|_, d| match d.category() {
        "input" => input_count += 1,
        "block" => block_count += 1,
        _ => {}
    });

    serial::write_str("drivers: registered=");
    serial::write_u64(count as u64);
    serial::write_str(" input=");
    serial::write_u64(input_count as u64);
    serial::write_str(" block=");
    serial::write_u64(block_count as u64);
    serial::write_str(" kb_ok=");
    serial::write_u64(kb_ok as u64);
    serial::write_str(" blk_ok=");
    serial::write_u64(blk_ok as u64);
    serial::write_line("");

    let pass = count == 2 && input_count == 1 && block_count == 1 && kb_ok && blk_ok;
    serial::write_line(if pass {
        "drivers: driver-model PASS"
    } else {
        "drivers: driver-model FAIL"
    });
}
