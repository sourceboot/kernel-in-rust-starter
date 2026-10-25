//! Lab 29 — your shell. This file is yours.
//!
//! init (lab 28) is not what starts it here: the course's shell does, when
//! you type its name, `ush`, at the course shell's prompt. It arrives with
//! fds 0, 1 and 2 on the console, prints a prompt, reads a line from fd 0,
//! runs it, and goes round again until the input ends or `exit`.
//!
//! The grammar, all of it:
//!
//! - a line is words separated by spaces;
//! - the first word names the program: `ls` runs `/bin/ls`, a word with a
//!   `/` in it runs that path; the rest are its arguments;
//! - `cmd > path` sends the command's fd 1 to `path` (created, or emptied
//!   first), `cmd < path` takes its fd 0 from `path`;
//! - `a | b` runs both at once, `a`'s fd 1 the write end of a pipe and `b`'s
//!   fd 0 its read end;
//! - a word that is exactly `$?` is the last command's status: its exit
//!   code, 127 when the name was not found, 128 + the signal for a process
//!   the kernel killed;
//! - three builtins: `cd [dir]` and `exit` run in the shell itself (in a
//!   child they would change nothing that lasts), `echo` prints its words
//!   joined by one space and runs in a child like any command, so its output
//!   can be redirected and piped.
//!
//! No quoting, no `$NAME`, no `;`, `&&` or `&`.
//!
//! The crate under `user/rust/` (`ulib`) has the syscalls, `print!` and
//! `eprintln!`. Fill in the steps; keep their signatures. A `todo!()` that
//! runs ends the program with exit code 101 and its message on fd 2.
#![no_std]
#![no_main]
// The steps are unused until `main` calls them; delete this line when it does.
#![allow(dead_code, unused_imports)]

use core::ffi::CStr;
use core::fmt::Write;
use ulib::*;

entry!(main);

/// The longest line, its newline included (the console's own line is 256).
const LINE_MAX: usize = 256;
/// The most words in one line, `|`, `<` and `>` included.
const MAX_WORDS: usize = 32;
/// The most commands in one pipeline.
const MAX_STAGES: usize = 8;

/// Step 1: the loop. Prompt, read a line, split it into words, and run it;
/// `$?` is the status of the line before. At the end of the input the shell
/// exits with that status, as `exit` does.
fn main(_args: Args) -> i32 {
    todo!("lab 29 step 1 — main: print `$ `, read a line, split it and run it, until the input ends")
}

/// Step 2: one line from fd 0 into `buf`, ONE byte per `read`, so a program
/// this line starts reads what is typed after it, not bytes the shell took.
/// Returns the line's length; `buf[n]` is a NUL where its `\n` was. `None`
/// at the end of the input with nothing typed. A line longer than `buf`
/// keeps its start.
fn read_line(buf: &mut [u8]) -> Option<usize> {
    let _ = buf;
    todo!("lab 29 step 2 — read_line: one line from fd 0, one byte per read")
}

/// Step 3: the words of `line` (which ends with a NUL), in `words`; returns
/// how many, or `None` when there are more than `words` holds. Each space
/// becomes a NUL, so every word is a C string inside `line` itself, ready for
/// `execve`.
fn split<'a>(line: &'a mut [u8], words: &mut [&'a CStr]) -> Option<usize> {
    let _ = (line, words);
    todo!("lab 29 step 3 — split: the line's words, each a C string inside the line")
}

/// Step 4: fork; the child becomes `cmd` with `input` as its fd 0 and
/// `output` as its fd 1 (any `<` or `>` in `cmd` applied after them). The
/// parent gets the child's pid, or `None` when the fork failed.
fn spawn(cmd: &[&CStr], input: i32, output: i32) -> Option<i32> {
    let _ = (cmd, input, output);
    todo!("lab 29 step 4 — spawn: fork; in the child, fds 0 and 1 set and the command run")
}

/// In the child: run `cmd[0]` — `/bin/<name>` when the name has no `/` — with
/// `cmd` as its argv and an empty environment. Never returns: a refused exec
/// says why and exits 127 for a name not found, 126 otherwise.
fn exec(cmd: &[&CStr]) -> ! {
    let _ = cmd;
    todo!("lab 29 step 4 — exec: run /bin/<name> or the path; 127 when it is not found")
}

/// Step 5: wait for `pid` and return its status as `$?` shows it: the exit
/// code, or 128 + the signal that killed it.
fn wait(pid: i32) -> i32 {
    let _ = pid;
    todo!("lab 29 step 5 — wait: the child's status as $? shows it")
}

/// Step 6: every word that is exactly `$?` becomes `last`, in decimal,
/// written into `num`.
fn expand<'a>(words: &mut [&'a CStr], last: i32, num: &'a mut [u8; 12]) {
    let _ = (words, last, num);
    todo!("lab 29 step 6 — expand: each `$?` word becomes the last status")
}

/// Step 7: the builtin `echo`, in the child: its words joined by one space,
/// then a newline, in one `write`. Returns the exit code.
fn echo(words: &[&CStr]) -> i32 {
    let _ = words;
    todo!("lab 29 step 7 — echo: the words joined by one space, and a newline")
}

/// Step 8: in the child, `cmd`'s `< path` and `> path` applied to fds 0 and
/// 1; the words left (the command and its arguments) are copied to `argv`,
/// and their count returned. `>` creates the file or empties it; a path
/// that cannot be opened ends the child with 1, a `<` or `>` with no path
/// after it with 2.
fn redirect<'a>(cmd: &[&'a CStr], argv: &mut [&'a CStr; MAX_WORDS]) -> usize {
    let _ = (cmd, argv);
    todo!("lab 29 step 8 — redirect: `< path` and `> path` applied to fds 0 and 1")
}

/// Step 9: `a | b | …`: every command started at once, each one's fd 1 the
/// write end of a pipe whose read end is the next one's fd 0. The shell
/// closes its own copy of each end as soon as the child that needs it has
/// one — a read end never sees the end of its input while any write end is
/// open, the shell's included. Returns the last command's status.
fn pipeline(words: &[&CStr]) -> i32 {
    let _ = words;
    todo!("lab 29 step 9 — pipeline: `a | b` started at once, joined by a pipe")
}

/// Step 10: the builtin `cd`, in the shell itself: to `dir`, or to `/` with
/// no argument. Returns 0, or 1 when the kernel refuses.
fn cd(words: &[&CStr]) -> i32 {
    let _ = words;
    todo!("lab 29 step 10 — cd: change the shell's own directory")
}

// ---- small helpers ----

/// `w` is exactly the word `s`.
fn is(w: &CStr, s: &str) -> bool {
    w.to_bytes() == s.as_bytes()
}

/// The C string that starts `bytes` and ends at its first NUL (or at its
/// end, whose last byte must then be one).
fn word(bytes: &[u8]) -> Option<&CStr> {
    CStr::from_bytes_until_nul(bytes).ok()
}

/// A C string as text, for messages.
fn text(s: &CStr) -> &str {
    s.to_str().unwrap_or("?")
}
