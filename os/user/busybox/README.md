# `busybox/` — busybox 1.37.0, built by the course (lab 30 on)

The second program here the course did not write, and the first one a learner keeps using: from
lab 30 `/bin/sh` is busybox's `ash`, and every name in `applets` is a hard link to `/bin/busybox`
(CONTRACT §A.7, §A.8).

**Upstream:** busybox 1.37.0, the release tarball
`https://busybox.net/downloads/busybox-1.37.0.tar.bz2` (2,565,764 bytes), sha256
`3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4` — the value busybox.net
publishes beside it (`busybox-1.37.0.tar.bz2.sha256`), checked 2026-10-23. Licence: GPL-2.0-only
(the tarball's `LICENSE`). **The tarball is not in this workspace** (the course hosts the same
file, at the URL in the offer below): `build.sh` reads it from `target/assets/busybox-1.37.0.tar.bz2` (or the path in
`BUSYBOX_TARBALL`) and refuses, before it touches `bin/`, when it is missing or its sha256 is not
the one above. Nothing in it is patched.

**What is here:** `busybox.config` (the whole `.config` the build uses, 1,210 lines: 55 options
on) and `applets` (the names `/bin` links to busybox, one per line, sorted). `build.sh` checks
that `make oldconfig` leaves `busybox.config` unchanged and that the build's `busybox.links`
names exactly `applets`, so the two files cannot drift apart.

**The applets, and why each is in:**

- `ash` and `sh` (`SH_IS_ASH`): the shell, with `ASH_ECHO`, `ASH_PRINTF` and `ASH_TEST` as
  builtins, `$((…))` (`FEATURE_SH_MATH`) and job control (`ASH_JOB_CONTROL`, which lab 31's
  signals meet). `cd`, `kill`, `trap`, `wait` and `$(…)` are ash's own.
- `mkdir`, `cat`, `grep`, `wc`, `ls`, `head`, `printf`, `sort`, `sed`, `cp`, `mv`, `rm`: the
  twelve file applets of the busybox-surface report's session (2026-10-18); `ktest-30` runs each.
  `ls` has `FEATURE_LS_FILETYPES`, `_SORTFILES`, `_TIMESTAMPS` and `_USERNAME` (the `ls -l`
  columns).
- `sleep`, `yes`, `kill`: what lab 31 and 32 run (a process to stop, a stream to interrupt).
- `vi`, with `FEATURE_EDITING` (ash's line editing) and the `FEATURE_VI_*` set for colon
  commands, search, yank/mark, `.`, `:set`, read-only and window resize: lab 33's editor.

**Off, deliberately:** `FEATURE_SH_STANDALONE`, `FEATURE_PREFER_APPLETS` and `FEATURE_SH_NOFORK`
— every applet is reached as a file in `/bin` through the learner's `execve`, never by ash calling
it in-process, so the hard links are what a lab exercises (LADDER Q3; the fallback stays unused).

**Build** (`build.sh`, layout `busybox`): the tarball is unpacked in a temporary directory, the
config copied in as `.config`, then

    make -C <src> O=<out> HOSTCC=cc CC=<zcc> AR="zig ar" SKIP_STRIP=y KCONFIG_NOTIMESTAMP=1 oldconfig
    make -C <src> O=<out> HOSTCC=cc CC=<zcc> AR="zig ar" SKIP_STRIP=y KCONFIG_NOTIMESTAMP=1 busybox busybox.links

where `<zcc>` is a two-line wrapper that runs `zig cc -target x86_64-linux-musl` (the pinned
zig 0.17.0, its bundled musl and zig's own `malloc`, TOOLCHAIN.md) and drops the three linker
flags lld does not take (`-Wl,--warn-common`, `-Wl,--verbose`, `-Wl,-Map,…`). The host's own
`cc` builds only Kconfig's helpers. `SKIP_STRIP=y` because zig at busybox's `-Oz` already
strips, and there is no `llvm-strip` to call; `KCONFIG_NOTIMESTAMP=1` keeps the build date out
(the banner reads `BusyBox v1.37.0 ()`). GNU make 3.81 (macOS's) builds it. Then every `musl`
check: `ET_EXEC`, at most one `PT_TLS`, no AVX.

**The binary** (2026-10-23, the mini): `bin/busybox`, 241,160 bytes, sha256
`927cffcd9c58f49ea9524e4edc200658ec2fd1b9f9e9e9fa2895a2f421f22c24`, the same bytes built in two
different directories. `ET_EXEC`, entry `0x10094f0`; `LOAD` R at `0x1000000`, R-E at
`0x10094f0`, RW at `0x103c878` (`filesz 0x320`, `memsz 0x9790`); one `PT_TLS` (`memsz 4`);
`GNU_STACK` 16 MiB; 0 AVX instructions.

**What it executes on the reference** (the re-trace, 2026-10-23: a per-number counter in a work
copy of `syscall::dispatch`, `ktest-30`'s seven groups): `read`, `write`, `open`, `close`,
`stat`, `lstat`, `lseek`, `mmap`, `munmap`, `rt_sigaction`, `rt_sigprocmask`, `ioctl`,
`writev`, `pipe`, `dup2`, `getpid`, `fork`, `wait4`,
`fcntl`, `getcwd`, `chdir`, `rename`, `mkdir`, `rmdir`, `unlink`, `geteuid`, `getppid`,
`arch_prctl`, `sched_getaffinity`, `getdents64`, `set_tid_address`, `clock_gettime` (and
`execve` and `exit_group`, which never return through the counter) — and two
numbers with no arm, both of which busybox survives: `sendfile` (40; its copy loop falls back to
`read`/`write`) and `access` (21; `rm` asks `W_OK` and goes on). Never `brk`, `mprotect`,
`uname`, or an `mmap` with `MAP_FIXED` or `PROT_NONE`: zig's allocator maps 124 KiB with a hint
and unmaps the unaligned 60 KiB. See CONTRACT §B.2-30 and the slice-8 report for the counts.

**The source offer (GPL-2.0 §3):** busybox ships in the learner's workspace as a binary
(`bin/busybox`, and inside every lab-30-on disk image), and the course offers the complete
corresponding source at the URL below; `busybox.config` and `os/user/build.sh` are in this
workspace. The URL goes live with the same deploy that first ships the binary (*placed
2026-10-24*). The offer:

> This workspace contains BusyBox 1.37.0 (`os/user/bin/busybox`), © Erik Andersen, Rob Landley,
> Denys Vlasenko and others, licensed under the GNU General Public License, version 2. It was
> built unmodified from `busybox-1.37.0.tar.bz2` (sha256
> `3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4`) with the configuration in
> `os/user/busybox/busybox.config` and the build script `os/user/build.sh`. The complete source
> is available at https://sourceboot.com/third-party/busybox-1.37.0.tar.bz2 (that same file, sha256
> `3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4`) for at least three years from
> the date you received this workspace.
