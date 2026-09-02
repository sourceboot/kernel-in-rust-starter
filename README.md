# kernel-in-rust-starter

The starter workspace for **Kernel in Rust: Own the Machine** — a
[SourceBoot](https://sourceboot.com) course where you build a real x86-64
kernel instead of watching someone else build one.

This repo is exactly what a learner's workspace starts as: a minimal Rust
workspace that boots via a vendored, pinned [Limine](https://limine-bootloader.org)
and hands your kernel a framebuffer. The lessons, the per-lab tests and the
grader are deliberately **not** in here — they live on
[sourceboot.com](https://sourceboot.com) and arrive through the `sboot` CLI,
into a separate cache directory. A repo created from this template stays your
code and nothing else, which is what makes it worth showing people.

> Renamed 2026-09-01 (was `os2-starter`, when the course id was `os2-rust`). GitHub redirects
> renamed repos, so a template link you already have keeps working.

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
export SBOOT_TOKEN=...        # from https://sourceboot.com/account
sboot test 01-first-light     # fetches the lab's tests + grader, builds, boots, grades
```

`sboot` recognises the repo by its `sboot.toml` and downloads each lab's tests
on first use (`sboot where` prints where they live — outside this repo). Note:
don't run `sboot start` inside the clone — that command creates a fresh
`./kernel-in-rust/` directory and refuses to write into a non-empty one. With the
template you already have the tree, so you don't need it.

**Without GitHub:**

```sh
sboot start kernel-in-rust
```

materialises this same tree into `./kernel-in-rust/`, no `gh` and no template
involved — make it a git repo whenever you like.

## What's in the tree

```
os/                     the workspace you own
  kernel/               your kernel (lib crate) — gfx.rs and serial.rs are
                        yours to write; fb.rs, ktap.rs, bootinfo.rs, exit.rs
                        are provided instruments
  boot/                 thin boot binary: Limine (vendored + pinned), boot
                        shim, linker script
.cargo/                 build config
rust-toolchain.toml     pinned stable Rust + the x86_64-unknown-none target
sboot.toml              tells the sboot CLI which course this repo is for
```

The tree compiles as-is on stable Rust, offline, with zero external crates
(the stubs are `todo!()`-style — they build, they just don't do anything yet):

```sh
cd os && cargo build --release --target x86_64-unknown-none
```

Producing a bootable image and running it in QEMU is `sboot`'s job — the
course's build tooling is fetched with each lab, versioned, so this repo never
goes stale underneath you.

## The course

https://sourceboot.com — lessons, labs and grading. This template is just the
starting tree.

## License

MIT — see [LICENSE](LICENSE). The scaffold is yours to build on and publish.
The course prose, tests and grader are not in this repo and are not covered by
it.
