//! Lab 28 — init: the first program, pid 1. This file is yours.
//!
//! The kernel starts init and nothing else; everything after it is init's
//! doing. It arrives with fds 0, 1 and 2 already open on the console, and
//! hands them on: every child it forks inherits them, and so does every
//! program a child execs. Its first argument is the run's nonce.
//!
//! What it does, in order:
//!
//! 1. says hello: one line, `init: key <16 hex digits>`, the digits being
//!    `derive(nonce, "init.nonce.line")`;
//! 2. mounts the two file systems the kernel only makes when asked —
//!    `/proc` ("proc") and `/dev` ("devfs") — with `mount` (165);
//! 3. starts the shell: `fork`, and in the child `execve("/bin/sh")`;
//! 4. reaps: `wait4(-1)` collects ANY child that has ended. The shell is
//!    one; the others are orphans — a process whose parent ended before it
//!    is handed to pid 1, and until pid 1 waits for it, it stays a zombie;
//! 5. when the shell itself ends, starts another one, so there is always a
//!    prompt.
//!
//! pid 1 never returns from `main`: when init ends, nothing is left to
//! reap orphans or to bring a shell back.
//!
//! The crate under `user/rust/` (`ulib`) has the syscalls (`fork`,
//! `execve`, `wait4`, `mount`, …), `println!`/`eprintln!` and `derive`.
//! Fill in the bodies; keep `main` and the signatures. A `todo!()` that
//! runs ends the program with exit code 101 and its message on fd 2.
#![no_std]
#![no_main]

use ulib::*;

entry!(main);

fn main(args: Args) -> i32 {
    hello(&args);
    mount_all();
    let mut shell = start_shell();
    loop {
        let status = reap_until(shell);
        shell = respawn(shell, status);
    }
}

/// Step 1: print `init: key <16 hex digits>`, `derive(argv[1], "init.nonce.line")`.
fn hello(args: &Args) {
    let _ = args;
    todo!("lab 28 step 1 — hello: print the key line derived from the nonce in argv[1]")
}

/// Step 2: mount "proc" on `/proc` and "devfs" on `/dev`.
fn mount_all() {
    todo!("lab 28 step 2 — mount_all: mount /proc and /dev")
}

/// Step 3: fork; the child execs `/bin/sh`. Return the shell's pid.
fn start_shell() -> i32 {
    todo!("lab 28 step 3 — start_shell: fork, and exec /bin/sh in the child")
}

/// Step 4: `wait4(-1)` until the child that ends is `shell`; reap every
/// other child on the way. Return the shell's status.
fn reap_until(shell: i32) -> WaitStatus {
    let _ = shell;
    todo!("lab 28 step 4 — reap_until: wait for any child until the shell ends")
}

/// Step 5: the shell ended — start another, and return its pid.
fn respawn(old: i32, status: WaitStatus) -> i32 {
    let _ = (old, status);
    todo!("lab 28 step 5 — respawn: start a new shell when the old one ends")
}
