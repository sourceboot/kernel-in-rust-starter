//! Lab 02's test vehicle: the same Limine shim, but instead of gfx::main the
//! boot hands control to the cockpit. The suite here is platform-authored —
//! it exercises the learner's UART driver, and the transcript arriving over
//! that UART at all is the implicit proof; these checks make the interesting
//! failure modes explicit.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use kernel::ktest;
use kernel::serial::{self, COM1};

// Scratch-register loopback: a byte written to SCR (COM1+7) reads back — the
// cheapest proof the learner's out8/in8 pair really reaches the device.
ktest!("serial.scratch", fn scratch(t: &mut TestCtx) {
    serial::out8(COM1 + 7, 0x5A);
    t.expect_eq("scratch readback", serial::in8(COM1 + 7) as u64, 0x5A);
    serial::out8(COM1 + 7, 0xA5);
    t.expect_eq("scratch readback (inverted)", serial::in8(COM1 + 7) as u64, 0xA5);
});

// After write_byte returns, the transmitter must actually have drained:
// THRE (bit 5) and TEMT (bit 6) both set once the line goes idle.
ktest!("serial.lsr.idle", fn lsr_idle(t: &mut TestCtx) {
    serial::write_str("# lsr probe\n");
    t.expect_eq("LSR & 0x60 after drain", (serial::in8(COM1 + 5) & 0x60) as u64, 0x60);
});

// Four FIFO depths through the polled path. A wrong LSR poll either never
// returns (the harness reports the hang) or overruns the FIFO.
ktest!("serial.burst", fn burst(t: &mut TestCtx) {
    serial::write_str("# ");
    for _ in 0..64 {
        serial::write_byte(b'.');
    }
    serial::write_str("\n");
    t.expect_eq("LSR TEMT after burst", (serial::in8(COM1 + 5) & 0x40) as u64, 0x40);
});

// The SKIP vocabulary, proven on a real check: the receive path needs the
// lab-07 IRQ line — polling RX here would hang the suite waiting for input
// that never comes.
ktest!("serial.rx", fn rx(t: &mut TestCtx) {
    t.skip("receive path arrives with the lab-07 IRQ line");
});

// Plan self-test: the registry the plan line was computed from is non-empty
// and every id is unique — a duplicated id would make a truncated transcript
// ambiguous about which test died.
ktest!("ktap.registry", fn registry(t: &mut TestCtx) {
    let ts = kernel::ktap::tests();
    t.expect(!ts.is_empty(), "registry non-empty");
    for (i, a) in ts.iter().enumerate() {
        t.expect(!a.id.is_empty(), "id non-empty");
        for b in &ts[i + 1..] {
            t.expect(a.id != b.id, "ids unique");
        }
    }
});

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    // The suite needs no framebuffer, but the handoff is still validated —
    // a bad boot must fail as a boot failure, not as mystery test failures.
    let _bi = shim::boot_info();
    kernel::ktap::run_and_exit()
}
