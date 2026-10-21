//! Lab 11 — tasks, and the switch between two of them. This file is yours.
//!
//! A task is a thread of kernel execution: its own kernel stack, and what
//! the CPU must be given back to carry on where it stopped. A task that
//! stops is always inside a call to `switch`, so the registers the System V
//! ABI says a callee keeps (`rbx`, `rbp`, `r12`–`r15`) go on its own stack,
//! and the one value that finds them again, the stack pointer, is the whole
//! `Context`. What a call does not cover is kept beside it in `Task`: the
//! x87/SSE registers (one `FXSAVE` area) and the FS base (an MSR). A new
//! task has never been inside `switch`, so `spawn` builds the stack it would
//! have had, with a return address that starts the task's function; a
//! function that returns ends its task.
//!
//! The boot flow — `kernel_main`, or a vehicle's `_start` — is task 0, and
//! it exists from the first time anyone asks for the table. The table is
//! only ever touched with interrupts off (`with_table`): a tick that found
//! its lock held would spin on it forever.
//!
//! Fill in the bodies; keep the signatures, the types and the static — later
//! labs and the course's tests use them.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::arch::asm;

use crate::addr::VirtAddr;
use crate::console::FdTable;
use crate::paging::AddressSpace;
use crate::sync::SpinLock;

/// Each spawned task's kernel stack.
pub const KSTACK_BYTES: usize = 16 * 1024;

/// A task's id. Task 0 is the boot flow; ids are never reused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Tid(pub u64);

/// Where a task is in its life. Lab 14 may add a state for a sleeping task
/// (or keep a sleeper `Ready` and off the run queue).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// In the run queue, waiting for the CPU.
    Ready,
    /// On the CPU now. Exactly one task is.
    Running,
    /// Ended; its memory goes back the next time another task looks.
    Dead,
}

/// A kernel stack. A spawned task's comes from the heap and is freed with
/// the task; the boot flow's was there before the heap and is only named.
pub struct KStack {
    mem: Option<Box<[u128]>>,
    top: Option<VirtAddr>,
}

impl KStack {
    /// `KSTACK_BYTES` from the heap, zeroed; its top is 16-aligned.
    pub fn new() -> KStack {
        let mem = alloc::vec![0u128; KSTACK_BYTES / 16].into_boxed_slice();
        let top = VirtAddr(mem.as_ptr() as u64 + KSTACK_BYTES as u64);
        KStack { mem: Some(mem), top: Some(top) }
    }

    /// A stack this task does not own: the boot flow's. `top` is the stack
    /// a trap from ring 3 lands on while it runs, when known.
    pub const fn borrowed(top: Option<VirtAddr>) -> KStack {
        KStack { mem: None, top }
    }

    /// The top: what `rsp0` and the syscall entry name while the task runs.
    pub fn top(&self) -> Option<VirtAddr> {
        self.top
    }

    /// Whether the memory is this task's own (and freed with it).
    pub fn is_owned(&self) -> bool {
        self.mem.is_some()
    }
}

impl Default for KStack {
    fn default() -> KStack {
        KStack::new()
    }
}

/// What `switch` saves of a stopped task: its stack pointer.
#[repr(C)]
#[derive(Default)]
pub struct Context {
    pub rsp: u64,
}

/// One `FXSAVE`/`FXRSTOR` image: the x87 and SSE state, 512 bytes, 16-aligned.
#[repr(C, align(16))]
pub struct FxArea(pub [u8; 512]);

impl FxArea {
    /// The state `fninit` and `ldmxcsr 0x1f80` leave: FCW = 0x37f (bytes
    /// 0–1), MXCSR = 0x1f80 (bytes 24–27) — every exception masked.
    pub const fn new() -> FxArea {
        let mut b = [0u8; 512];
        b[0] = 0x7f;
        b[1] = 0x03;
        b[24] = 0x80;
        b[25] = 0x1f;
        FxArea(b)
    }
}

impl Default for FxArea {
    fn default() -> FxArea {
        FxArea::new()
    }
}

/// A task. It must stay `Send`, so the table of them can sit behind a
/// `SpinLock`.
pub struct Task {
    pub id: Tid,
    pub kstack: KStack,
    pub ctx: Context,
    pub fx: FxArea,
    pub fs_base: u64,
    /// The address space to load when this task runs; `None` = whatever CR3
    /// is live serves (lab 12 gives processes one).
    pub aspace: Option<AddressSpace>,
    /// The fd table that lived in `console.rs` until now.
    pub fds: FdTable,
    pub state: State,
}

/// Every task, the run queue, and who is on the CPU.
pub struct TaskTable {
    pub tasks: Vec<Box<Task>>,
    /// Ready tasks in the order they get the CPU; the running task is not in it.
    pub ready: VecDeque<Tid>,
    pub current: Tid,
    next_id: u64,
}

impl TaskTable {
    pub const fn new() -> TaskTable {
        TaskTable { tasks: Vec::new(), ready: VecDeque::new(), current: Tid(0), next_id: 1 }
    }

    /// Task 0, the boot flow, the first time anyone needs the table.
    pub fn ensure_boot(&mut self) {
        let _ = self.next_id;
        todo!("lab 11 step 1 — ensure_boot: an empty table gets task 0, the boot flow, Running")
    }

    pub fn get(&self, id: Tid) -> Option<&Task> {
        let _ = id;
        todo!("lab 11 step 1 — get: the task with this id")
    }

    pub fn get_mut(&mut self, id: Tid) -> Option<&mut Task> {
        let _ = id;
        todo!("lab 11 step 1 — get_mut: the task with this id")
    }

    /// Take out every dead task except the running one (a task that just
    /// died may still be on its own stack). The caller drops them.
    pub fn take_dead(&mut self) -> Vec<Box<Task>> {
        todo!("lab 11 step 2 — take_dead: remove the dead tasks no one is running on")
    }
}

impl Default for TaskTable {
    fn default() -> TaskTable {
        TaskTable::new()
    }
}

/// The one task table.
pub static TASKS: SpinLock<TaskTable> = SpinLock::new(TaskTable::new());

/// Interrupts off; returns whether they were on.
pub(crate) fn irq_save() -> bool {
    let rflags: u64;
    // SAFETY: reads RFLAGS through the stack and clears IF.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) rflags) };
    rflags & (1 << 9) != 0
}

/// Interrupts back on if `irq_save` found them on.
pub(crate) fn irq_restore(was_on: bool) {
    if was_on {
        // SAFETY: setting IF touches no memory.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
}

/// Run `f` on the table with interrupts off.
pub fn with_table<R>(f: impl FnOnce(&mut TaskTable) -> R) -> R {
    let on = irq_save();
    let r = f(&mut TASKS.lock());
    irq_restore(on);
    r
}

/// Run `f` on the running task (task 0 if nothing was ever spawned).
pub fn with_current<R>(f: impl FnOnce(&mut Task) -> R) -> R {
    let _ = f;
    todo!("lab 11 step 1 — with_current: the running task, task 0 if the table is new")
}

/// Where task `id` is in its life; `None` once it is gone from the table.
pub fn state(id: Tid) -> Option<State> {
    let _ = id;
    todo!("lab 11 step 8 — state: the task's State, None once it is gone")
}

/// Start `f` as a new task, ready to run; returns its id. A task whose
/// function returns ends.
pub fn spawn(f: fn()) -> Tid {
    let _ = f;
    todo!("lab 11 step 4 — spawn: a kernel stack laid out as `switch` would have left it, a Task, the back of the queue")
}

/// End the running task. Never returns.
pub fn exit() -> ! {
    crate::sched::exit_current()
}

/// Save the running task's callee-saved registers on its stack and its
/// stack pointer in `*from`; load `*to`'s stack pointer, pop what that stack
/// holds, and return — on the other task's stack. Interrupts must be off.
/// Yours is `#[unsafe(naked)]`, its body one `naked_asm!`.
///
/// # Safety
/// `from` must be writable and `to` must hold a stack pointer `switch` saved
/// (or `spawn` built), whose stack is still allocated.
pub unsafe extern "C" fn switch(from: *mut Context, to: *const Context) {
    let _ = (from, to);
    todo!("lab 11 step 5 — switch: push the six callee-saved registers, swap stack pointers, pop, ret")
}
