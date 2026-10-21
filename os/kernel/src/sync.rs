//! `SpinLock<T>` — rfk lab 03's lock, provided here so the course has one.
//! Course-owned (proposed CONTRACT §B.1 amendment, 2026-10-16: the spine uses
//! `SpinLock` from lab 09 on — the console, the task table, pipes — and no
//! learner module owns it). You wrote this one already; it ships whole.
//!
//! One thing here is new: the deadlock check (2026-10-17, CONTRACT §A.9
//! S4). `lock` that finds the lock held with interrupts off prints one line,
//! `# deadlock: <file>:<line>: a <type> lock found held with interrupts off;
//! nothing on this CPU can release it`, naming where `lock` was called and
//! what the lock guards — and then keeps spinning, as before. On one CPU
//! (this course has no SMP) a lock found held with interrupts off can never
//! be released: whoever holds it cannot run until an interrupt, and none can
//! come. So the line never prints on a correct kernel; when it prints, the
//! kernel has already hung, and the line says where. The usual cause is a
//! lock taken again under itself — a `FileRef` dropped inside `with_fds`,
//! whose pipe end wakes a reader, and `wakeup` takes the task table's lock
//! that `with_fds` holds — or a lock an interrupt handler takes while the
//! code it stopped holds it.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::panic::Location;
use core::sync::atomic::{AtomicBool, Ordering};

/// Set while a `# deadlock:` line is being printed. The print path may
/// itself take a `SpinLock` (your serial, your console); if that is the lock
/// found held, its `lock` lands here again, finds this set, and only spins —
/// one line, never a line inside a line.
static NAMING: AtomicBool = AtomicBool::new(false);

/// RFLAGS.IF: are interrupts on?
fn interrupts_on() -> bool {
    let rflags: u64;
    // SAFETY: reads RFLAGS through the stack; changes nothing.
    unsafe { core::arch::asm!("pushfq", "pop {}", out(reg) rflags) };
    rflags & (1 << 9) != 0
}

/// The deadlock check's line (the module doc). Out of line: `lock` stays
/// small, and this runs at most once per `lock` call.
#[cold]
#[inline(never)]
fn name_deadlock(at: &Location<'_>, what: &str) {
    if NAMING.swap(true, Ordering::Acquire) {
        return;
    }
    crate::kprintln!(
        "\n# deadlock: {}:{}: a {} lock found held with interrupts off; nothing on this CPU can release it",
        at.file(),
        at.line(),
        what
    );
    NAMING.store(false, Ordering::Release);
}

pub struct SpinLock<T: ?Sized> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: the lock hands out at most one guard at a time, so sharing the
// SpinLock across CPUs or interrupt contexts moves T between them; T: Send
// makes that sound.
unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}
// SAFETY: owning the lock owns the T inside it.
unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> SpinLock<T> {
        SpinLock { locked: AtomicBool::new(false), value: UnsafeCell::new(value) }
    }
}

impl<T: ?Sized> SpinLock<T> {
    /// Take the lock, spinning while someone else holds it. Found held with
    /// interrupts off, it prints the deadlock check's line once (the module
    /// doc) and spins on: on a correct kernel that never happens.
    #[track_caller]
    pub fn lock(&self) -> SpinGuard<'_, T> {
        let mut checked = false;
        loop {
            if let Some(g) = self.try_lock() {
                return g;
            }
            if !checked {
                checked = true;
                if !interrupts_on() {
                    name_deadlock(Location::caller(), core::any::type_name::<T>());
                }
            }
            core::hint::spin_loop();
        }
    }

    pub fn try_lock(&self) -> Option<SpinGuard<'_, T>> {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| SpinGuard { lock: self })
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }
}

pub struct SpinGuard<'a, T: ?Sized> {
    lock: &'a SpinLock<T>,
}

impl<T: ?Sized> Deref for SpinGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: a guard exists only while `locked` is true and we set it.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T: ?Sized> DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as deref; &mut self means no other reference through this guard.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T: ?Sized> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release)
    }
}
