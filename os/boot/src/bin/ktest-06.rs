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
//! Forger round 1 (2026-10-21): `pic.eoi` also reads the master ISR inside
//! one tick (auto-EOI reads clear), `kbd.fifo`'s full ring must hold 63 or
//! 64, and `clock.monotonic` counts the IRQ 0 deliveries itself, through the
//! same borrowed IDT copy, and holds `TICKS` to that count (round 2: the PM
//! timer bound stays beside it as a sanity check, never the pin). Round 3
//! (2026-10-21 17:45): `pit.ticks` has no count floor (0 ticks, or 1 with
//! IRQ 0 still in service, is all it refuses), and `clock.monotonic`'s wait
//! ends when the CPU has taken the chosen ticks, the PM timer only its guard.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::arch::{asm, naked_asm};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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
    // The count refuses only what the read-back cannot see (ruled 2026-10-21
    // 17:45, forger round 3): no tick at all, and exactly one tick with IRQ 0
    // still in service, the PIC waiting for an EOI. The in-service bit is read
    // from the PIC (OCW3), never inferred from the count. No floor above one:
    // QEMU delivers fewer ticks than the PIT raises when its host timer runs
    // late (63-67 per 100 ms on the mini at load 3-7; 1, 2, 2 and 9 on honest
    // kernels at load 24-37; 100 of 100 on a Linux host's QEMU 10.2.1), and
    // the 18.2 Hz default (1-2 per 100 ms) is the peak row's to refuse: its
    // reload reads back above 1193 on every boot. CONTRACT appendix (10) has
    // the measurements.
    let (imr, isr, ifl) = pic_state();
    if n == 1 && isr & 1 != 0 {
        t.fail(format_args!("a tick arrives and is acknowledged: counted 1 in 100 ms — the first tick arrived and no other, and IRQ 0 is still in service (master ISR {isr:#04x}): the PIC is waiting for an EOI that never came"));
    } else if n == 0 {
        // No tick: RFLAGS.IF, the master IMR and the read-back say which part
        // is missing. The peak is read before the handler is blamed, so a
        // slow channel is never reported as a missing handler (2026-10-18, s06
        // vehicle finding (a); since forger round 3 no count condition, so the
        // slow-channel line is the only one beside a read-back above 1193).
        if !ifl {
            t.fail(format_args!("a tick arrives: counted 0 in 100 ms, with interrupts off after irq::init (RFLAGS.IF clear) — `sti` never ran"));
        } else if imr & 1 != 0 {
            t.fail(format_args!("a tick arrives: counted 0 in 100 ms, with IRQ 0 masked (master IMR {imr:#04x})"));
        } else if !(mode & 3 == 2 || mode & 3 == 3) {
            t.fail(format_args!(
                "a tick arrives: counted 0 in 100 ms; the 8254 read-back says mode {mode}, not a rate generator (mode 2 or 3) — a one-shot or interrupt-on-terminal-count channel fires once; see the mode row below"
            ));
        } else if peak > 1193 {
            t.fail(format_args!(
                "a tick arrives: counted 0 in 100 ms, from a channel in mode {mode} whose count peaked at {peak}, above 1193 — channel 0 is reloading from more than 1193 (a reload of 0, or one that never reached channel 0's data port 0x40, counts from 65,536, 18.2 Hz), so the ticks are slow, not stopped"
            ));
        } else {
            t.fail(format_args!("a tick arrives: counted 0 in 100 ms, from a channel in mode {mode} with IRQ 0 open and interrupts on — no tick reached TICKS: the tick handler is not on vector IRQ_BASE, or does not add to TICKS"));
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
    if more {
        in_service_probe(t, &mut c);
    }
});

// ── pic.eoi's probe inside one tick (2026-10-21, forger round 1, F2) ───────
// Between handlers an acknowledged IRQ 0 reads clear, and so does one the PIC
// acknowledged by itself: with ICW4's auto-EOI bit set (0x03), the 8259
// clears the in-service bit as the CPU takes the interrupt, so the tick runs
// with an `eoi` that writes nothing. Inside a tick the two differ. The
// vehicle loads a copy of the kernel's IDT whose IRQ 0 gate is its own
// entry, takes one tick there, reads the master ISR (IRQ 0 must be in
// service), calls the learner's `irq::eoi(0)`, reads it again (IRQ 0 must be
// clear), and loads the kernel's IDT back. The kernel's table is never
// written, and its handler misses that one tick.

#[repr(C, packed)]
struct Dtr {
    limit: u16,
    base: u64,
}

static PROBE_IDT: [[AtomicU64; 2]; 256] = [const { [AtomicU64::new(0), AtomicU64::new(0)] }; 256];
/// 0 until the probe's tick: then 0x100 | the master ISR on entry, and the
/// same after `eoi(0)`.
static PROBE_IN: AtomicU32 = AtomicU32::new(0);
static PROBE_OUT: AtomicU32 = AtomicU32::new(0);

fn master_isr() -> u8 {
    use kernel::serial::{in8, out8};
    out8(0x20, 0x0b); // OCW3: the next read of the command port is the ISR
    let isr = in8(0x20);
    out8(0x20, 0x0a); // OCW3: back to the IRR, the power-on default
    isr
}

/// The probe's tick, called from `probe_entry` with interrupts off (an
/// interrupt gate). Every tick that lands here acknowledges itself through
/// the learner's `eoi(0)`; only the first is recorded.
extern "C" fn probe_tick() {
    let isr_in = master_isr();
    irq::eoi(0);
    let isr_out = master_isr();
    if PROBE_IN.load(Ordering::Relaxed) == 0 {
        PROBE_OUT.store(0x100 | isr_out as u32, Ordering::Relaxed);
        PROBE_IN.store(0x100 | isr_in as u32, Ordering::Release);
    }
}

/// The probe's gate: saves the registers a call may clobber, calls
/// `probe_tick`, returns from the interrupt. On entry rsp is 8 mod 16 (the
/// CPU's five words on a 16-aligned stack); nine pushes make the call's
/// alignment right.
#[unsafe(naked)]
extern "C" fn probe_entry() {
    naked_asm!(
        "push rax", "push rcx", "push rdx", "push rsi", "push rdi",
        "push r8", "push r9", "push r10", "push r11",
        "cld",
        "call {f}",
        "pop r11", "pop r10", "pop r9", "pop r8",
        "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rax",
        "iretq",
        f = sym probe_tick,
    )
}

/// The borrowed IDT: with interrupts off, copy every gate of the kernel's
/// loaded table into `PROBE_IDT`, let `irq0` rewrite the IRQ 0 gate (it gets
/// the kernel's two words and returns the copy's), load the copy, and return
/// the kernel's IDTR for `give_back_idt`. The kernel's table is only read.
fn borrow_idt(irq0: impl FnOnce(u64, u64) -> (u64, u64)) -> Dtr {
    let mut old = Dtr { limit: 0, base: 0 };
    quiet(|| {
        // SAFETY: sidt stores 10 bytes into `old`.
        unsafe { asm!("sidt [{}]", in(reg) &mut old, options(nostack, preserves_flags)) };
        let (base, limit) = (old.base, old.limit as usize);
        for (v, g) in PROBE_IDT.iter().enumerate() {
            let (lo, hi) = if v * 16 + 15 <= limit {
                // SAFETY: the gate lies inside the loaded IDT's limit; a read.
                unsafe {
                    let p = (base as *const u64).add(v * 2);
                    (core::ptr::read_volatile(p), core::ptr::read_volatile(p.add(1)))
                }
            } else {
                (0, 0)
            };
            g[0].store(lo, Ordering::Relaxed);
            g[1].store(hi, Ordering::Relaxed);
        }
        let v = irq::IRQ_BASE as usize;
        let (lo, hi) = irq0(PROBE_IDT[v][0].load(Ordering::Relaxed), PROBE_IDT[v][1].load(Ordering::Relaxed));
        PROBE_IDT[v][0].store(lo, Ordering::Relaxed);
        PROBE_IDT[v][1].store(hi, Ordering::Relaxed);
        let new = Dtr { limit: 256 * 16 - 1, base: PROBE_IDT.as_ptr() as u64 };
        // SAFETY: the copy holds every gate of the kernel's table, with the
        // IRQ 0 gate as `irq0` built it; interrupts are off until `quiet`
        // returns.
        unsafe { asm!("lidt [{}]", in(reg) &new, options(readonly, nostack, preserves_flags)) };
    });
    old
}

/// Load the kernel's own IDTR back, as `borrow_idt` saved it.
fn give_back_idt(old: &Dtr) {
    quiet(|| {
        // SAFETY: the kernel's own IDTR, as sidt stored it.
        unsafe { asm!("lidt [{}]", in(reg) old, options(readonly, nostack, preserves_flags)) };
    });
}

/// An interrupt gate's two words for `h`, keeping `lo`'s selector, IST and
/// type bits (16-47) and replacing the offset.
fn gate_to(lo: u64, h: u64) -> (u64, u64) {
    ((h & 0xffff) | (lo & 0x0000_ffff_ffff_0000) | ((h >> 16) & 0xffff) << 48, h >> 32)
}

fn in_service_probe(t: &mut kernel::ktap::TestCtx, c: &mut Clock) {
    PROBE_IN.store(0, Ordering::Relaxed);
    let old = borrow_idt(|_, _| {
        let cs: u64;
        // SAFETY: reads CS; nothing else.
        unsafe { asm!("mov {:x}, cs", out(reg) cs, options(nomem, nostack, preserves_flags)) };
        gate_to((cs & 0xffff) << 16 | 0x8e << 40, probe_entry as extern "C" fn() as usize as u64)
    });
    let landed = c.wait(200_000, || PROBE_IN.load(Ordering::Acquire) != 0);
    give_back_idt(&old);
    let (pin, pout) = (PROBE_IN.load(Ordering::Acquire), PROBE_OUT.load(Ordering::Relaxed));
    if !landed {
        t.fail(format_args!("no tick arrived in 200 ms on the vector IRQ_BASE while the in-service bit was read"));
        return;
    }
    let (isr_in, isr_out) = (pin as u8, pout as u8);
    t.note(format_args!("pic.eoi: inside a tick, the master ISR read {isr_in:#04x} on entry and {isr_out:#04x} after eoi(0)"));
    if isr_in & 1 == 0 {
        t.fail(format_args!(
            "inside a tick, before any EOI, the master ISR read {isr_in:#04x}: IRQ 0 was not in service — the PIC acknowledged the tick by itself, which is auto-EOI (ICW4 bit 1 set, as in 0x03); with ICW4 = 0x01 the line stays in service until eoi(0)"
        ));
    } else if isr_out & 1 != 0 {
        t.fail(format_args!(
            "inside a tick, eoi(0) returned with IRQ 0 still in service (master ISR {isr_out:#04x}): eoi(0) wrote no EOI to the master's command port, 0x20"
        ));
    }
    t.expect(isr_in & 1 != 0 && isr_out & 1 == 0, "inside a tick, IRQ 0 is in service until eoi(0) clears it");
}

// ── clock.monotonic's delivery count (2026-10-21, forger round 2, F4) ──────
// For the wait, the vehicle borrows the IDT again (as pic.eoi's probe does)
// and points the IRQ 0 gate at `count_entry`: one locked add to DELIVERED,
// then a jump to the kernel's own IRQ 0 entry with the CPU's frame and every
// register untouched, so the kernel's handler runs exactly as it would, EOI
// and all. TICKS must then move by the number of ticks the CPU took: a host
// that loses ticks lowers both counts alike (a tick that adds 2 to TICKS read
// 1.3 times the PM timer on the mini at load 5 but under 1 with 24 hogs, so
// the PM-timer bound alone was load-dependent). Since forger round 3 (ruled
// 2026-10-21 17:45) the wait itself ends on delivery progress too: it ends
// when the CPU has taken the chosen number of ticks, and the PM timer only
// bounds it as a hang guard (MONO_GUARD_US) that prints "no timer progress".
// TICKS, DELIVERED and now_ns are read together with interrupts off, so they
// agree exactly on an honest kernel; the row allows 1 either way.

/// IRQ 0 deliveries taken through `count_entry`.
static DELIVERED: AtomicU64 = AtomicU64::new(0);
/// The kernel's own IRQ 0 entry, from its IDT gate: where `count_entry` goes.
static CHAIN: AtomicU64 = AtomicU64::new(0);

/// The counting gate: count, then continue at the kernel's entry. `lock inc`
/// changes only the arithmetic flags, which the frame's RFLAGS does not see.
#[unsafe(naked)]
extern "C" fn count_entry() {
    naked_asm!(
        "lock inc qword ptr [rip + {d}]",
        "jmp qword ptr [rip + {c}]",
        d = sym DELIVERED,
        c = sym CHAIN,
    )
}

ktest!("clock.monotonic", fn clock_monotonic(t: &mut TestCtx) {
    let n = 20 + t.pick("clock.monotonic", 100);
    let mut c = Clock::start();
    // The IRQ 0 gate's present bit (47): with none, no tick is counted, the
    // wait below ends at its guard, and there is nothing to hold TICKS to.
    let mut present = false;
    let old = borrow_idt(|lo, hi| {
        present = lo >> 47 & 1 == 1;
        if !present {
            return (lo, hi);
        }
        CHAIN.store((lo & 0xffff) | ((lo >> 48) & 0xffff) << 16 | hi << 32, Ordering::Relaxed);
        gate_to(lo, count_entry as extern "C" fn() as usize as u64)
    });
    let (k0, e0, ns0, u0) = quiet(|| (ticks(), DELIVERED.load(Ordering::Relaxed), time::now_ns(), c.us()));
    // The wait ends on delivery progress: when the CPU has taken n ticks.
    // The PM timer is only its hang guard: no tick for MONO_STALL_US, or
    // MONO_GUARD_US in all, ends it with a named line.
    let mut prev = ns0;
    let mut backwards = 0;
    let (mut seen, mut seen_u, mut gap) = (e0, u0, 0);
    let mut stalled = false;
    let (mut last_u, mut paused) = (u0, 0u64);
    let reached = loop {
        let now = time::now_ns();
        if now < prev {
            backwards += 1;
        }
        prev = now;
        let (e, u) = (DELIVERED.load(Ordering::Relaxed), c.us());
        // This loop only spins: a jump of more than MONO_PAUSE_US between two
        // of its own turns is time the host did not run this machine, so it
        // does not count toward the stall (the 2 s ceiling still counts it).
        if u - last_u > MONO_PAUSE_US {
            seen_u += u - last_u;
            paused += u - last_u;
        }
        last_u = u;
        if e != seen {
            gap = gap.max(u - seen_u);
            (seen, seen_u) = (e, u);
        }
        if e >= e0 + n {
            break true;
        }
        if u - seen_u >= MONO_STALL_US {
            stalled = true;
            break false;
        }
        if u - u0 >= MONO_GUARD_US {
            break false;
        }
        core::hint::spin_loop();
    };
    let (k1, e1, ns1, u1) = quiet(|| (ticks(), DELIVERED.load(Ordering::Relaxed), time::now_ns(), c.us()));
    give_back_idt(&old);
    let d = ns1.wrapping_sub(ns0);
    let real = (u1 - u0) * 1000;
    let (moved, taken) = (k1.wrapping_sub(k0), e1 - e0);
    t.note(format_args!(
        "clock.monotonic: waited {n} ticks (chosen by this run's nonce); TICKS moved {moved} while the CPU took IRQ 0 {taken} times; now_ns moved {d} ns in {real} ns of the {} clock; the longest stretch between two ticks was {gap} us; host pauses {paused} us, not counted toward the stall",
        if c.pm { "PM-timer" } else { "TSC" }
    ));
    if stalled {
        t.fail(format_args!(
            "no timer progress: the CPU took IRQ 0 {taken} times of the {n} waited for, then none for {} ms — if pit.ticks or pic.eoi is red, the tick stopped and that row says why; if both pass, the tick is fine and the host paused this machine for that long (a sleeping or overloaded host): run it again",
            MONO_STALL_US / 1000
        ));
    } else if !reached {
        t.fail(format_args!(
            "no timer progress: the CPU took IRQ 0 {taken} times of the {n} waited for in {} ms — the ticks come far below 1000 Hz: if pit.ticks's read-back shows a count peaking above 1193, that is the reload; if it peaks at 1193, the host is running this machine too slowly to grade: run it again on a quieter machine",
            real / 1_000_000
        ));
    }
    count_eq(t, "now_ns readings that went backwards", backwards, 0);
    t.expect(d + 1_000_000 >= taken * 999_000 && d <= (taken + 1) * 1_000_000, "now_ns moved by the ticks the CPU took, at about 1 ms each");
    // The pin (forger round 2, F4): one count per tick the CPU took, 1
    // either way as ruled (read together, an honest kernel's two are equal).
    if present && moved > taken + 1 {
        t.fail(format_args!(
            "TICKS moved {moved} while the CPU took IRQ 0 {taken} times: TICKS grows by more than one per tick, or something besides the tick handler adds to it"
        ));
    } else if present && moved + 1 < taken {
        t.fail(format_args!(
            "TICKS moved {moved} while the CPU took IRQ 0 {taken} times: the tick handler does not add one to TICKS on every tick"
        ));
    }
    t.expect(!present || moved.abs_diff(taken) <= 1, "TICKS moves by one for every tick the CPU takes");
    // A sanity check beside the pin, never the pin: ticks can be lost, never
    // gained (the PIT and the PM timer run off one clock in the machine), so
    // now_ns may fall behind the PM timer and must not run ahead of it by
    // more than MONO_SLACK. A host that loses ticks only lowers now_ns here,
    // so this cannot refuse an honest kernel; on its own it missed a doubled
    // tick wherever the host lost half its ticks (forger round 1, F4).
    if reached && d > real + real / MONO_SLACK_DIV + MONO_SLACK_NS {
        t.fail(format_args!(
            "now_ns moved {d} ns while {real} ns passed on the PM timer: the clock runs {}.{:02} times real time — TICKS grows by more than one per tick, or something besides the tick adds to it",
            d / real.max(1),
            d % real.max(1) * 100 / real.max(1)
        ));
    }
    t.expect(!reached || d <= real + real / MONO_SLACK_DIV + MONO_SLACK_NS, "now_ns runs no faster than real time");
});

/// clock.monotonic's margin over the PM timer: now_ns may lead it by
/// 1/MONO_SLACK_DIV of the window plus MONO_SLACK_NS. Measured 2026-10-21
/// (the mini, the rubric's comment on clock.monotonic has the numbers): an
/// honest kernel reads 0.26–0.69 of PM time on the mini as its load rises,
/// and about 0.99985 on Linux, where QEMU loses no ticks; 2 ms covers a
/// tick merged at either end of the window.
const MONO_SLACK_DIV: u64 = 50;
const MONO_SLACK_NS: u64 = 2_000_000;

/// clock.monotonic's hang guard (forger round 3, ruled 2026-10-21 17:45): the
/// wait for the chosen ticks gives up, printing "no timer progress", after
/// MONO_STALL_US with no tick at all or MONO_GUARD_US in all (PM-timer time).
/// A broken tick ends the wait at the first, so its run still finishes inside
/// the rubric's timeout; a slow one (the 18.2 Hz default, when the nonce asks
/// for more ticks than it delivers in 2 s; pit.ticks' peak line refuses it
/// either way) at the second. The
/// honest kernels' longest wait and longest stretch between two ticks are in
/// CONTRACT appendix (10): the guard did not fire on any honest boot measured
/// there. Since the round-4 close-out (the round-3 TL review, finding 1, Fix A)
/// a host pause — a jump of more than MONO_PAUSE_US between two turns of the
/// spinning wait — does not count toward MONO_STALL_US (it fired on an honest
/// kernel after one 700 ms pause before); MONO_GUARD_US is still host time.
const MONO_STALL_US: u64 = 500_000;
const MONO_GUARD_US: u64 = 2_000_000;
const MONO_PAUSE_US: u64 = 20_000;

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

/// The last burst's size: more than the 64-slot ring holds.
const FULL: usize = 300;

ktest!("kbd.fifo", fn kbd_fifo(t: &mut TestCtx) {
    // Bursts of 3 to 8 scancodes, pushed with interrupts off and popped only
    // once the whole burst is in, round after round until 160 have gone
    // through — more than twice round a 64-slot ring, so both counters pass
    // the slot array's end. Sizes and codes come from the nonce: set-1 make
    // codes 0x02-0x39, bit 7 set on about half. Each burst must come back
    // oldest first, then the ring empty. Last, one burst of FULL with no pop
    // between, more than the ring holds: what comes back must be its OLDEST
    // part, in order — a full ring drops the newest — and 64 or 63 of them
    // (one slot kept empty is as right as all 64 used).
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
    // The ring is 64 slots (the page's task 4; one kept empty is as right),
    // so a full ring holds 63 or 64. A bigger ring never fills under this
    // burst, and its full check is never run (2026-10-21, forger round 1, F1:
    // 1024 slots with no full check came back 300 of 300).
    if j.is_none() && m > 0 && m != 63 && m != 64 {
        t.fail(format_args!(
            "of the {FULL} pushed with no pop, the oldest {m} came back in order: a ring of 64 slots holds 64 when full, or 63 if one slot is kept empty"
        ));
    }
    t.expect(j.is_some() || m == 63 || m == 64, "the full ring held 64 scancodes (63 with one slot kept empty)");
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
