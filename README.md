# kernel-in-rust-starter

The starter workspace for **Kernel in Rust: Boot to a Shell** — a
[SourceBoot](https://sourceboot.com) course where you write a 64-bit x86-64
kernel in stable Rust instead of watching someone else write one. It starts with
pixels on the screen and a serial line you can trust, then the CPU's descriptor
tables, its faults, the timer and the keyboard, your own frames and page tables,
and a program running at ring 3 that talks to your kernel through Linux's own
system-call interface. Processes, pipes, a disk, a file system Linux can mount
and a shell come after that.

This repo is exactly what a learner's workspace starts as: a Rust workspace that
boots through a vendored, pinned [Limine](https://limine-bootloader.org) and hands
your kernel a framebuffer and a memory map. The lessons, each lab's checks and the
grader are deliberately **not** in here — they live on
[sourceboot.com](https://sourceboot.com) and arrive through the `sboot` CLI, into
a separate cache directory. A repo created from this template stays your code,
which is what makes it worth showing people.

> Renamed 2026-09-01 (was `os2-starter`, when the course id was `os2-rust`). GitHub redirects
> renamed repos, so a template link you already have keeps working.

## What you install

A Mac or a Linux machine and a terminal, plus QEMU, the PC emulator your kernel
boots on. Lab 00 walks you through Rust (stable, with the bare-metal
`x86_64-unknown-none` target), the C compiler and linker your build needs, and
QEMU itself — on disk, about 230 MB on Linux and 1 GB on a Mac. Git is optional.

**Windows:** this course does not run on native Windows. Its Windows path is
WSL2, and lab 00 gains its WSL2 steps once that path has been tested end to end.
Until then, use a Mac or a Linux machine.

## Use it

Two ways in; both give you the same tree.

**With GitHub** — your kernel starts life as a private repo you own:

```sh
gh repo create my-os --private --template sourceboot/kernel-in-rust-starter --clone
cd my-os
```

Then install `sboot` and work from inside the clone:

```sh
curl -fsSL https://sourceboot.com/install.sh | sh
sboot login                   # connects this machine, in your browser
sboot test 00-welcome         # fetches the lab's tests and grader, builds, boots, grades
```

`sboot test <lab>` is the whole loop: it builds your kernel, boots it and grades
what it does, on your own machine, as often as you like. `sboot submit <lab>`
records the official result — the same checks. `sboot` recognises the repo by its
`sboot.toml` and downloads each lab's tests on first use (`sboot where` prints
where they live — outside this repo). Running `sboot start kernel-in-rust` inside
the clone is safe: it puts back anything that is missing and never touches a file
you have edited. With the template you already have the tree, so you don't need it.

**Without GitHub:**

```sh
sboot start kernel-in-rust
```

materialises this same tree into `./sourceboot-kernel-in-rust/` — named after the course you typed —
makes it a git repository and commits it, asking
once for the name and email git stamps on your commits. No `gh` and no template
involved; `--dir <name>` picks a different folder. When you want it on GitHub,
`sboot repo` creates the private repo and pushes it.

## What's in the tree

```
os/                     the cargo workspace
  kernel/               your kernel, a no_std library crate
    src/                one module per subject: gfx.rs (lab 01), serial.rs
                        (lab 02), panic.rs, gdt.rs, idt.rs and on — each a
                        stub with its published signatures over `todo!()`
                        bodies, for the lab that writes it; fb.rs is a
                        provided framebuffer helper you may change
  boot/                 the course's boot code: Limine (vendored + pinned),
                        the boot shim and the linker script
  user/                 the course's own user programs, C sources and built
                        binaries, which your kernel runs from lab 09 on
  .cargo/               the bare-metal build settings
rust-toolchain.toml     stable Rust and the x86_64-unknown-none target
sboot.toml              tells the sboot CLI which course this repo is for
```

**Some of these files are the course's, not yours:** everything under `os/boot/`
and `os/user/`, and in `os/kernel/src/` the module list in `lib.rs` and the test
harness your kernel reports through (`ktap.rs`, with `bootinfo.rs`,
`handshake.rs`, `fwcfg.rs`, `sync.rs`, `addr.rs` and `exit.rs`). They are public,
so publishing your repo gives nothing away. From lab 02 on, `sboot test` and
`sboot submit` put back the course's copy of any of them that differs — your
version is saved, not lost — so an edit there never reaches a grade. The files to
edit are the ones each lab names.

The tree compiles as-is on stable Rust, offline, with zero external crates (the
stubs are `todo!()`s — they build, they just don't do anything yet):

```sh
cd os && cargo build --release --target x86_64-unknown-none
```

Making a bootable image and booting it is `sboot test`'s job: the course's build
tooling is fetched with each lab, versioned, so this repo never goes stale
underneath you.

## The course

https://sourceboot.com — lessons, labs and grading. This template is just the
starting tree.

## License

MIT — see [LICENSE](LICENSE). The scaffold is yours to build on and publish.
The course prose, tests and grader are not in this repo and are not covered by
it.
