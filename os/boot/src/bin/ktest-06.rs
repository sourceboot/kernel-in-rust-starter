//! Lab 06's vehicle: the tick, the EOI, the keyboard ring, the clock.
//! Every wait here is bounded by the ACPI PM timer (3.579545 MHz, port 0x608
//! on QEMU's `pc` machine as the firmware leaves it), never by the learner's
//! own tick, so a dead PIT fails a row instead of hanging the run. The two
//! keyboard rows share one collection: the vehicle prints
//! `# sboot keys-ready`, then waits up to 3 s for the keys the run sends
//! (the rubric's `[[script]]`: s, b, o, o, t), echoing each as it arrives.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::arch::asm;
use core::sync::atomic::Ordering;

use kernel::sync::SpinLock;
use kernel::time::{self, TICKS};
use kernel::{irq, kbd, ktest};

const PM_HZ: u64 = 3_579_545;

fn pm() -> u32 {
    let v: u32;
    // SAFETY: a 32-bit read of the ACPI PM timer port; no memory is touched.
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") 0x608u16, options(nomem, nostack, preserves_flags)) };
    v & 0xff_ffff
}

fn tsc() -> u64 {
    let (lo, hi): (u32, u32);
    // SAFETY: rdtsc writes edx:eax only.
    unsafe { asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    (hi as u64) << 32 | lo as u64
}

/// Microseconds elapsed, from the PM timer when it ticks, else from the TSC
/// at the 1 GHz QEMU's TCG runs it at (measured 2026-10-16).
struct Clock {
    pm: bool,
    last: u32,
    acc: u64,
    t0: u64,
}

impl Clock {
    fn start() -> Clock {
        let a = pm();
        let t0 = tsc();
        let mut moved = false;
        for _ in 0..10_000 {
            if pm() != a {
                moved = true;
                break;
            }
        }
        Clock { pm: moved, last: pm(), acc: 0, t0 }
    }
    fn us(&mut self) -> u64 {
        if self.pm {
            let p = pm();
            self.acc += (p.wrapping_sub(self.last) & 0xff_ffff) as u64;
            self.last = p;
            self.acc * 1_000_000 / PM_HZ
        } else {
            (tsc() - self.t0) / 1000
        }
    }
    /// Spin until `done()` or `limit_us` passes; true if `done()` held.
    fn wait(&mut self, limit_us: u64, mut done: impl FnMut() -> bool) -> bool {
        let start = self.us();
        while self.us() - start < limit_us {
            if done() {
                return true;
            }
            core::hint::spin_loop();
        }
        done()
    }
}

fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

ktest!("pit.ticks", fn pit_ticks(t: &mut TestCtx) {
    use kernel::serial::{in8, out8};
    // The 8254's read-back command (0xe2): channel 0's status byte, whose
    // bits 1-3 are the mode and 4-5 the access mode. Then, all through the
    // 100 ms, latched counts (command 0x00): a counter reloaded from 1193
    // never reads above it, and over a hundred periods it reads close to it.
    out8(0x43, 0xe2);
    let status = in8(0x40);
    let mut peak = 0u16;
    let mut c = Clock::start();
    let t0 = ticks();
    c.wait(100_000, || {
        out8(0x43, 0x00);
        let lo = in8(0x40) as u16;
        let hi = in8(0x40) as u16;
        peak = peak.max(hi << 8 | lo);
        false
    });
    let n = ticks() - t0;
    let mode = (status >> 1) & 7;
    t.note(format_args!(
        "pit.ticks: {n} ticks in 100 ms of the {} clock; channel 0 in mode {mode}, access {}, count peaked at {peak}",
        if c.pm { "PM-timer" } else { "TSC" },
        (status >> 4) & 3
    ));
    // QEMU delivers fewer ticks than the PIT raises when its host timer runs
    // late (64-66 per 100 ms on the mini, measured 2026-10-16): the floor
    // catches a dead or 18.2 Hz PIT, and the readback checks the rate.
    t.expect(n >= 25, "at least 25 ticks in 100 ms");
    t.expect(mode & 3 == 2 || mode & 3 == 3, "channel 0 runs as a rate generator (mode 2) or square wave (mode 3)");
    t.expect_eq("channel 0's access mode (low byte, then high)", ((status >> 4) & 3) as u64, 3);
    t.expect(peak <= 1193 && peak >= 1100, "channel 0 counts down from 1193 (1000 Hz)");
});

ktest!("pic.eoi", fn pic_eoi(t: &mut TestCtx) {
    let mut c = Clock::start();
    let t0 = ticks();
    let more = c.wait(50_000, || ticks() >= t0 + 3);
    if !more && ticks() == 1 {
        t.fail(format_args!("exactly one tick arrived since irq::init: the PIC is waiting for an EOI that never came"));
    }
    t.expect(more, "ticks keep arriving (three within 50 ms)");
    // OCW3 0x0b: the next read of the command port returns the ISR. Out here,
    // between handlers, an acknowledged IRQ 0 is never still in service.
    let mut stuck = 0;
    for _ in 0..64 {
        kernel::serial::out8(0x20, 0x0b);
        if kernel::serial::in8(0x20) & 1 != 0 {
            stuck += 1;
        }
    }
    t.expect_eq("ISR samples with IRQ 0 still in service", stuck, 0);
    t.expect_eq("master IMR (only IRQ 0 and 1 unmasked)", kernel::serial::in8(0x21) as u64, 0xfc);
    t.expect_eq("slave IMR (all masked)", kernel::serial::in8(0xa1) as u64, 0xff);
});

ktest!("clock.monotonic", fn clock_monotonic(t: &mut TestCtx) {
    let n = 20 + t.pick("clock.monotonic", 100);
    let mut c = Clock::start();
    let t0 = ticks();
    c.wait(50_000, || ticks() > t0);
    let (k0, ns0) = (ticks(), time::now_ns());
    let mut prev = ns0;
    let mut backwards = 0;
    let reached = c.wait(n * 4_000 + 100_000, || {
        let now = time::now_ns();
        if now < prev {
            backwards += 1;
        }
        prev = now;
        ticks() >= k0 + n
    });
    let ns1 = time::now_ns();
    let d = ns1.wrapping_sub(ns0);
    t.note(format_args!("clock.monotonic: waited {n} ticks (chosen by this run's nonce); now_ns moved {d} ns"));
    t.expect(reached, "the chosen number of ticks arrived");
    t.expect_eq("now_ns readings that went backwards", backwards, 0);
    t.expect(d + 1_000_000 >= n * 999_000 && d <= (n + 1) * 1_000_000, "now_ns moved by the ticks waited, at about 1 ms each");
});

const WANT: [u8; 10] = [0x1f, 0x9f, 0x30, 0xb0, 0x18, 0x98, 0x18, 0x98, 0x14, 0x94];

/// The scancodes collected after `# sboot keys-ready`, once per run.
static KEYS: SpinLock<Option<([u8; 32], usize, [u8; 16], usize)>> = SpinLock::new(None);

fn collect() -> ([u8; 32], usize, [u8; 16], usize) {
    let mut g = KEYS.lock();
    if let Some(k) = *g {
        return k;
    }
    let (mut sc, mut n, mut echo, mut e) = ([0u8; 32], 0, [0u8; 16], 0);
    while kbd::pop().is_some() {}
    kernel::ktap::note(format_args!("sboot keys-ready"));
    let mut c = Clock::start();
    c.wait(3_000_000, || {
        while let Some(s) = kbd::pop() {
            if n < sc.len() {
                sc[n] = s;
                n += 1;
            }
            if let Some(a) = kbd::to_ascii(s) {
                if e < echo.len() {
                    echo[e] = a;
                    e += 1;
                }
                kernel::ktap::note(format_args!("echo {}", a as char));
            }
        }
        n >= WANT.len()
    });
    *g = Some((sc, n, echo, e));
    (sc, n, echo, e)
}

ktest!("kbd.ring", fn kbd_ring(t: &mut TestCtx) {
    let (sc, n, _, _) = collect();
    t.expect_eq("scancodes received", n as u64, WANT.len() as u64);
    for (i, w) in WANT.iter().enumerate() {
        t.expect_eq("scancode, in order (set 1 make/break)", sc[i] as u64, *w as u64);
    }
});

ktest!("kbd.echo.paced", fn kbd_echo(t: &mut TestCtx) {
    let (_, _, echo, e) = collect();
    t.expect(&echo[..e] == b"sboot", "the echoed keys spell the keys sent");
    t.expect(WANT.iter().skip(1).step_by(2).all(|&b| kbd::to_ascii(b).is_none()), "a break code types nothing");
});

fn prelude() {
    let (k, i, _) = shim::stacks();
    kernel::gdt::init(k, i);
    kernel::idt::init();
    irq::init();
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    kernel::ktap::run_and_halt_after(prelude)
}
