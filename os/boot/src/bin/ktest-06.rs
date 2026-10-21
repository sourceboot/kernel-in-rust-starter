//! Lab 06's vehicle: the tick, the EOI, the keyboard ring, the clock.
//! Every wait here is bounded by the ACPI PM timer (3.579545 MHz, port 0x608
//! on QEMU's `pc` machine as the firmware leaves it), never by the learner's
//! own tick, so a dead PIT fails a row instead of hanging the run. The two
//! keyboard rows share one collection: the vehicle prints
//! `# sboot keys-ready`, then waits up to 3 s for the keys the run sends
//! (the rubric's `[[script]]`: s, b, o, o, t), echoing each as it arrives.
//! Those keys come one at a time, each popped before the next lands, so the
//! ring's ORDER is graded by `kbd.fifo`: bursts of scancodes pushed with
//! interrupts off and popped only once the whole burst is in. And `now_ns`
//! hours into uptime is graded by `clock.hours`, which sets `TICKS` for one
//! read with interrupts off and puts it back (2026-10-18, slice 3 vrow06).

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

/// `expect_eq` for a count: the failure line prints both sides in decimal
/// ("observed 1, expected 10"), where a byte or a mask keeps `expect_eq`'s
/// hex (2026-10-18, fix round sk06-learner, outside the brief).
fn count_eq(t: &mut kernel::ktap::TestCtx, what: &str, observed: u64, expected: u64) {
    if observed != expected {
        t.fail(format_args!("{what}: observed {observed}, expected {expected}"));
    }
}

/// The master PIC's mask (IMR) and in-service (ISR) registers, and whether
/// RFLAGS.IF is set: what a channel that ticks 0 or 1 times is waiting on.
fn pic_state() -> (u8, u8, bool) {
    use kernel::serial::{in8, out8};
    let imr = in8(0x21);
    out8(0x20, 0x0b); // OCW3: the next read of the command port is the ISR
    let isr = in8(0x20);
    out8(0x20, 0x0a); // OCW3: back to the IRR, the power-on default
    let flags: u64;
    // SAFETY: reads RFLAGS through the stack; no memory the program owns is touched.
    unsafe { asm!("pushfq", "pop {}", out(reg) flags, options(nomem, preserves_flags)) };
    (imr, isr, flags & 0x200 != 0)
}

/// Run `f` with interrupts off (no tick, no key can land inside it), then
/// put IF back as it was.
fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let flags: u64;
    // SAFETY: saves RFLAGS through the stack and clears IF; no memory the
    // program owns is touched.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags, options(nomem)) };
    let r = f();
    if flags & 0x200 != 0 {
        // SAFETY: IF was set on entry; this restores it.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
    r
}

/// splitmix64's step: the next word of a stream seeded from the nonce.
fn mix(z: &mut u64) -> u64 {
    *z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut x = *z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
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
    // late (63-67 per 100 ms on the mini over 31 runs, 2026-10-16/17;
    // 100 of 100 on a Linux host's QEMU 10.2.1, 2026-10-17): the floor
    // catches a dead or 18.2 Hz PIT, and the readback checks the rate.
    if n < 25 {
        // The failure line carries the read-back beside the count, so a red
        // on a loaded host reads as the host's timer (count low, read-back
        // right) and not as the learner's PIT (ruled 2026-10-17, CONTRACT
        // §B.2-06).
        if n <= 1 && (mode == 2 || mode == 3) && peak > 1193 {
            // A rate generator that ticked at most once while counting down
            // from ABOVE 1193 is a slow channel, not a stuck PIC: a reload of
            // 0 counts from 65,536 (18.2 Hz, one tick per 55 ms), so 100 ms
            // holds one tick with every EOI sent. The peak is read first, so
            // this line never blames the EOI for the reload (2026-10-18, s06
            // vehicle finding (a)).
            t.fail(format_args!(
                "at least 25 ticks in 100 ms: counted {n}, from a channel in mode {mode} whose count peaked at {peak}, above 1193 — channel 0 is reloading from more than 1193 (a reload of 0, or one that never reached channel 0's data port 0x40, counts from 65,536, 18.2 Hz), so the ticks are slow, not stopped"
            ));
        } else if n <= 1 && (mode == 2 || mode == 3) {
            // A rate generator reloading from 1193 that ticked 0 or 1 times:
            // the PIC's own registers and IF say which. Zero ticks is never
            // the EOI (no tick ever arrived to acknowledge), and one tick is
            // the EOI only while IRQ 0 is still in service: a correct kernel
            // on a loaded host has measured 1 tick in 100 ms with IRQ 0 not
            // in service, and that is the host's timer (2026-10-18, fix
            // round sk06-ladder-6).
            let (imr, isr, ifl) = pic_state();
            if !ifl {
                t.fail(format_args!("at least 25 ticks in 100 ms: counted {n}, with interrupts off after irq::init (RFLAGS.IF clear) — `sti` never ran"));
            } else if imr & 1 != 0 {
                t.fail(format_args!("at least 25 ticks in 100 ms: counted {n}, with IRQ 0 masked (master IMR {imr:#04x})"));
            } else if n == 1 && isr & 1 != 0 {
                t.fail(format_args!("at least 25 ticks in 100 ms: counted 1 — the first tick arrived and no other, and IRQ 0 is still in service (master ISR {isr:#04x}): the PIC is waiting for an EOI that never came"));
            } else if n == 0 {
                t.fail(format_args!("at least 25 ticks in 100 ms: counted 0, from a channel in mode {mode} with IRQ 0 open and interrupts on — no tick reached TICKS: the tick handler is not on vector IRQ_BASE, or does not add to TICKS"));
            } else {
                t.fail(format_args!("at least 25 ticks in 100 ms: counted {n}; the 8254 read-back says mode {mode}, count peaking at {peak}, IRQ 0 is open and not in service — if the read-back is right, the host's timer dropped the ticks"));
            }
        } else if n <= 1 {
            t.fail(format_args!(
                "at least 25 ticks in 100 ms: counted {n}; the 8254 read-back says mode {mode}, not a rate generator (mode 2 or 3) — a one-shot or interrupt-on-terminal-count channel fires once; see the mode row below"
            ));
        } else {
            t.fail(format_args!(
                "at least 25 ticks in 100 ms: counted {n}; the 8254 read-back says mode {mode}, access {}, count peaking at {peak} (1193 = 1 kHz) — if the read-back is right, the host's timer dropped the ticks",
                (status >> 4) & 3
            ));
        }
    }
    t.expect(mode & 3 == 2 || mode & 3 == 3, "channel 0 runs as a rate generator (mode 2) or square wave (mode 3)");
    t.expect_eq("channel 0's access mode (low byte, then high)", ((status >> 4) & 3) as u64, 3);
    t.expect(peak <= 1193 && peak >= 1100, "channel 0 counts down from 1193 (1000 Hz)");
});

ktest!("pic.eoi", fn pic_eoi(t: &mut TestCtx) {
    let mut c = Clock::start();
    let t0 = ticks();
    // 200 ms for three ticks: a missing EOI stops the tick for good, so the
    // window only has to outlast a loaded host's timer, which has measured a
    // stall inside the 50 ms this used to allow on an honest kernel
    // (2026-10-18, fix round: 1 red in 15 runs of an EOI-first handler).
    let more = c.wait(200_000, || ticks() >= t0 + 3);
    if !more {
        t.note(format_args!("pic.eoi: {} ticks in 200 ms, {} since irq::init", ticks() - t0, ticks()));
    }
    if !more && ticks() == 1 {
        // One tick and no other is the missing EOI only from a channel that
        // keeps firing; a one-shot channel stops by itself (2026-10-18, F4).
        // The 8254's read-back (0xe2) gives channel 0's mode, as in pit.ticks.
        kernel::serial::out8(0x43, 0xe2);
        let mode = (kernel::serial::in8(0x40) >> 1) & 7;
        if mode & 3 == 2 || mode & 3 == 3 {
            t.fail(format_args!("exactly one tick arrived since irq::init, from a channel in mode {mode} (a rate generator): the PIC is waiting for an EOI that never came"));
        } else {
            t.fail(format_args!("exactly one tick arrived since irq::init; the 8254 read-back says channel 0 is in mode {mode}, not a rate generator (mode 2) or square wave (mode 3) — a one-shot channel fires once, so this is the mode, not the EOI: see pit.ticks's mode line"));
        }
    }
    t.expect(more, "ticks keep arriving (three within 200 ms)");
    // OCW3 0x0b: the next read of the command port returns the ISR. Out here,
    // between handlers, an acknowledged IRQ 0 is never still in service.
    let mut stuck = 0;
    for _ in 0..64 {
        kernel::serial::out8(0x20, 0x0b);
        if kernel::serial::in8(0x20) & 1 != 0 {
            stuck += 1;
        }
    }
    count_eq(t, "ISR samples with IRQ 0 still in service", stuck, 0);
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
    count_eq(t, "now_ns readings that went backwards", backwards, 0);
    t.expect(d + 1_000_000 >= n * 999_000 && d <= (n + 1) * 1_000_000, "now_ns moved by the ticks waited, at about 1 ms each");
});

/// `TICKS × 1193 × 10⁹` passes `u64::MAX` at 15,462,484 ticks (4 h 18 min).
const U64_TICKS: u64 = 15_462_484;

ktest!("clock.hours", fn clock_hours(t: &mut TestCtx) {
    // One read of now_ns with TICKS set, interrupts off, to a count between
    // 16 and 100 million (4.4 to 27.8 hours at 1 kHz), chosen by the nonce —
    // every count past U64_TICKS — then TICKS put back. The answer is the
    // PIT's own period per tick: 1193 / 1,193,182 s, 999,847.47 ns, which a
    // rounded 1 ms per tick misses by 153 parts per million.
    let k = 16_000_000 + t.pick("clock.hours", 84_000_001);
    let ns = quiet(|| {
        let saved = TICKS.load(Ordering::Relaxed);
        TICKS.store(k, Ordering::Relaxed);
        let ns = time::now_ns();
        TICKS.store(saved, Ordering::Relaxed);
        ns
    });
    let want = (k as u128 * 1193 * 1_000_000_000 / 1_193_182) as u64;
    let ppm = (ns as i128 - want as i128) * 1_000_000 / want as i128;
    let tenths = want / 360_000_000_000;
    t.note(format_args!(
        "clock.hours: TICKS set to {k} for one read (chosen by this run's nonce; {}.{} hours of ticks); now_ns read {ns} ns, the ticks' time is {want} ns ({ppm:+} ppm)",
        tenths / 10,
        tenths % 10
    ));
    // The failure line names a cause only where the error has that cause's
    // shape: 0 is a division that ran first, a factor of a thousand or a
    // million is the unit, and only what is left — far off, and none of
    // those — is the product leaving 64 bits (2026-10-18, fix round
    // sk06-ladder-7).
    let near = |scale: u128| ns > 0 && (ns as u128 * scale).abs_diff(want as u128) <= want as u128 / 100;
    if ns == 0 {
        t.fail(format_args!(
            "now_ns read 0 at {k} ticks: a division ran before the multiplication — 1193 / 1,193,182 is 0 in integers"
        ));
    } else if near(1000) {
        t.fail(format_args!(
            "now_ns is a thousand times too small at {k} ticks ({ppm:+} ppm): it counts microseconds, not nanoseconds"
        ));
    } else if near(1_000_000) {
        t.fail(format_args!(
            "now_ns is a million times too small at {k} ticks ({ppm:+} ppm): it counts milliseconds, not nanoseconds"
        ));
    } else if ns.abs_diff(want) > want / 1000 {
        t.fail(format_args!(
            "now_ns is {ppm:+} ppm off at {k} ticks: past {U64_TICKS} ticks the product TICKS × 1193 × 10⁹ no longer fits 64 bits, so a product kept in u64 wraps"
        ));
    } else if ns == k * 1_000_000 {
        t.fail(format_args!(
            "now_ns is exactly {k} ms: 1 ms per tick, {ppm:+} ppm fast — one PIT period is 1193 / 1,193,182 s, 999,847.47 ns"
        ));
    }
    t.expect(ns.abs_diff(want) <= want / 100_000, "now_ns at that count is the ticks times one PIT period (999,847.47 ns), to within 10 ppm");
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
    count_eq(t, "scancodes received", n as u64, WANT.len() as u64);
    for (i, w) in WANT.iter().enumerate() {
        t.expect_eq("scancode, in order (set 1 make/break)", sc[i] as u64, *w as u64);
    }
});

/// What one popped burst held, for the failure line: the bytes, then
/// `empty` where pop said None.
struct Pops<'a>(&'a [Option<u8>]);

impl core::fmt::Display for Pops<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, p) in self.0.iter().enumerate() {
            let sep = if i == 0 { "" } else { " " };
            match p {
                Some(b) => write!(f, "{sep}{b:02x}")?,
                None => write!(f, "{sep}empty")?,
            }
        }
        Ok(())
    }
}

/// The last burst's size: more than any ring a learner here builds holds.
const FULL: usize = 300;

ktest!("kbd.fifo", fn kbd_fifo(t: &mut TestCtx) {
    // Bursts of 3 to 8 scancodes, pushed with interrupts off and popped only
    // once the whole burst is in, round after round until 160 have gone
    // through — more than twice round a 64-slot ring, so both counters pass
    // the slot array's end. Sizes and codes come from the nonce: set-1 make
    // codes 0x02-0x39, bit 7 set on about half. Each burst must come back
    // oldest first, then the ring empty. Last, one burst of FULL with no pop
    // between, more than any ring here holds: what comes back must be its
    // OLDEST part, in order — a full ring drops the newest — whatever the
    // ring's size (one slot kept empty is as right as all 64 used).
    let mut z = t.derived("kbd.fifo");
    let (mut total, mut bursts, mut bad) = (0usize, 0usize, 0u64);
    let mut first: Option<(usize, usize, [u8; 8], [Option<u8>; 9])> = None;
    let mut full = (0usize, None::<(usize, u8)>, [0u8; FULL]);
    quiet(|| {
        for _ in 0..256 {
            if kbd::pop().is_none() {
                break;
            }
        }
        while total < 160 {
            let k = 3 + (mix(&mut z) % 6) as usize;
            let mut sent = [0u8; 8];
            for s in sent.iter_mut().take(k) {
                let w = mix(&mut z);
                *s = (0x02 + (w % 0x38) as u8) | if w >> 63 == 1 { 0x80 } else { 0 };
                kbd::push(*s);
            }
            let mut got = [None; 9];
            for g in got.iter_mut().take(k + 1) {
                *g = kbd::pop();
            }
            if (0..k).any(|i| got[i] != Some(sent[i])) || got[k].is_some() {
                bad += 1;
                first.get_or_insert((bursts + 1, k, sent, got));
                for _ in 0..256 {
                    if kbd::pop().is_none() {
                        break;
                    }
                }
            }
            total += k;
            bursts += 1;
        }
        let mut sent = [0u8; FULL];
        for s in sent.iter_mut() {
            let w = mix(&mut z);
            *s = (0x02 + (w % 0x38) as u8) | if w >> 63 == 1 { 0x80 } else { 0 };
            kbd::push(*s);
        }
        let (mut m, mut j) = (0usize, None);
        while m <= FULL {
            match kbd::pop() {
                None => break,
                Some(b) => {
                    if j.is_none() && (m == FULL || b != sent[m]) {
                        j = Some((m, b));
                    }
                    m += 1;
                }
            }
        }
        full = (m, j, sent);
    });
    t.note(format_args!(
        "kbd.fifo: {bursts} bursts of 3 to 8 scancodes (sizes and codes chosen by this run's nonce), {total} pushed and popped in all"
    ));
    if let Some((b, k, sent, got)) = first {
        let newest_first = (0..k).all(|i| got[i] == Some(sent[k - 1 - i]));
        t.fail(format_args!(
            "burst {b}: pushed {} before any pop, popped {}{}",
            Pops(&sent.map(Some)[..k]),
            Pops(&got[..k + 1]),
            if newest_first { " — the burst came back newest first" } else { "" }
        ));
    }
    count_eq(t, "bursts that did not come back oldest first, then empty", bad, 0);
    let (m, j, sent) = full;
    t.note(format_args!("kbd.fifo: then {FULL} pushed with no pop between; {m} came back"));
    if let Some((i, b)) = j {
        if i == FULL {
            t.fail(format_args!("pop {} returned {b:02x}: more came back than the {FULL} pushed", i + 1));
        } else {
            let kept_newest = i == 0 && m <= FULL && b == sent[FULL - m];
            t.fail(format_args!(
                "of the {FULL} pushed with no pop, pop {} returned {b:02x} where push {} was {:02x}{}",
                i + 1,
                i + 1,
                sent[i],
                if kept_newest { " — the full ring kept the newest and lost the oldest" } else { "" }
            ));
        }
    }
    t.expect(m > 0 && j.is_none(), "a full ring hands back the oldest scancodes pushed, in order, and drops the newest");
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
