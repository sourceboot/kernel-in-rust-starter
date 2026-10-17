//! `SpinLock<T>` — rfk lab 03's lock, provided here so the course has one.
//! Course-owned (proposed CONTRACT §B.1 amendment, 2026-10-16: the spine uses
//! `SpinLock` from lab 09 on — the console, the task table, pipes — and no
//! learner module owns it). You wrote this one already; it ships whole.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

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
    pub fn lock(&self) -> SpinGuard<'_, T> {
        loop {
            if let Some(g) = self.try_lock() {
                return g;
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
