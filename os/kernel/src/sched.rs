//! Lab 11 — the scheduler: who runs next, and when. This file is yours.
//!
//! Round robin over one run queue. A task gives the CPU up two ways: it
//! asks (`yield_now`), or the clock takes it (`tick`, which IRQ 0's handler
//! calls after its EOI). Either way the running task goes to the back of the
//! queue and the one at the front gets the CPU; with nobody waiting, the
//! running task carries on. Around `task::switch`, save what a call does not
//! keep — the x87/SSE registers (`FXSAVE`/`FXRSTOR`) and the FS base (MSR
//! `0xC000_0100`) — and point the TSS's `rsp0` and the syscall entry at the
//! next task's kernel stack. Interrupts stay off from the moment the queue
//! is read until the next task runs.
//!
//! Lab 14 adds waiting without the CPU: `sleep(chan, lock, guard)` takes
//! the task off the CPU and out of the run queue until a `wakeup(chan)` —
//! letting `lock` go and leaving the CPU as one step, so no wakeup falls in
//! between and is lost — and `slices` counts the times each task was given
//! the CPU (a task asleep is given none).
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them.

use crate::addr::VirtAddr;
use crate::sync::{SpinGuard, SpinLock};
use crate::task::Tid;

/// The boot flow becomes task 0, with `boot_stack` as the kernel stack a
/// trap from ring 3 lands on while it runs. Call once, before the first
/// `spawn`.
pub fn init(boot_stack: VirtAddr) {
    let _ = boot_stack;
    todo!("lab 11 step 1 — init: task 0 in the table, its kernel stack named")
}

/// The running task's id (task 0 before anything was spawned).
pub fn current() -> Tid {
    todo!("lab 11 step 6 — current: who is on the CPU")
}

/// How many context switches have happened since boot.
pub fn switches() -> u64 {
    todo!("lab 11 step 6 — switches: count every switch")
}

/// Give the CPU to the next ready task, if there is one; return when this
/// task is picked again. Frees tasks that died meanwhile.
pub fn yield_now() {
    todo!("lab 11 step 2 — yield_now: the running task to the back of the queue, the front one onto the CPU; nobody waiting = return")
}

/// IRQ 0, after the EOI: the running task's time is up. Called with
/// interrupts off and inside the handler. Never allocates or frees (the
/// interrupted code may hold the heap's lock).
pub fn tick() {
    todo!("lab 11 step 3 — tick: the same switch as yield_now, without touching the heap")
}

/// End the running task: dead, out of the run queue, and switched away
/// from for good. Its memory is freed later, by a task not running on it.
pub fn exit_current() -> ! {
    todo!("lab 11 step 7 — exit_current: mark the running task Dead and switch away for good")
}

/// How many slices task `id` has been given: the times the switch put it on
/// the CPU. A task asleep is given none; one that yields in a loop is given
/// one per turn.
pub fn slices(id: Tid) -> u64 {
    let _ = id;
    todo!("lab 14 task 2 — slices: count every time the switch puts a task on the CPU")
}

/// Sleep on `chan` until a `wakeup(chan)`. `held` is the caller's guard on
/// `lock`, under which it found it must wait: `sleep` releases it and
/// leaves the CPU as one step, so no `wakeup` can come between the caller's
/// check and the sleep; on waking it takes `lock` again and returns the new
/// guard. Wakeups wake every sleeper on a channel, so the caller checks its
/// condition again (a loop). Never from the tick.
pub fn sleep<'a, T: ?Sized>(chan: usize, lock: &'a SpinLock<T>, held: SpinGuard<'a, T>) -> SpinGuard<'a, T> {
    let _ = (chan, lock, held);
    todo!("lab 14 task 1 — sleep: off the CPU and out of the run queue until a wakeup, the lock let go as one step with it")
}

/// Wake every task asleep on `chan`: each goes back in the run queue. Safe
/// from an interrupt handler: it never allocates.
pub fn wakeup(chan: usize) {
    let _ = chan;
    todo!("lab 14 task 3 — wakeup: every sleeper on chan back in the run queue")
}

/// Lab 31 step 4: what an interruptible sleep returns when a signal ended it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interrupted;

/// Lab 31 step 4: `sleep`, that a signal the running process would act on
/// also ends: then the task is off the channel and `Err` carries the guard.
pub fn sleep_intr<'a, T: ?Sized>(
    chan: usize,
    lock: &'a SpinLock<T>,
    held: SpinGuard<'a, T>,
) -> Result<SpinGuard<'a, T>, (SpinGuard<'a, T>, Interrupted)> {
    let _ = (chan, lock, held);
    todo!("lab 31 step 4 — sleep_intr: sleep, ended early by a signal the process would act on")
}

/// Lab 31 step 4: wait until the tick count reaches `deadline`, or a signal
/// the running process would act on arrives (`Err`).
pub fn sleep_until_intr(deadline: u64) -> Result<(), Interrupted> {
    let _ = deadline;
    todo!("lab 31 step 4 — sleep_until_intr: the timed wait, ended early by a signal")
}
