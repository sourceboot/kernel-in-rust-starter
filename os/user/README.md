# `user/` — the course's programs

Course-owned (E1): the learner builds the kernel that runs these, never these. Every program is
freestanding C built by the pinned `zig cc` (**[TOOLCHAIN.md](TOOLCHAIN.md)**), run by hand —
no CI workflow runs `build.sh` yet — and the binaries ship under `bin/` beside their sources
(CONTRACT §A.8).

| file | what it is |
|---|---|
| `sys.h` | the raw x86-64 Linux syscall stubs (`syscall0`–`syscall6`), the `SYS_*` numbers (CONTRACT §A.4's), the errno numbers, `read_cs()`, `derive()` (§A.2), the output helpers, the lab 11–15 wrappers (`fork`, `execve`, `wait4`, `pipe2`, `dup`, `dup2`, `fcntl`, `sched_yield`, `getppid`) with their flags, and the protocol helpers (`dec_parse`, `line_row`) — no libc, no SSE |
| `crt0.c` | `_start`: argc/argv/envp off the initial stack, `main`, then `exit(main's return)`; plus `memset`/`memcpy`/`memmove`/`memcmp`, which the compiler may call on its own |
| `user.ld` | the link: `ET_EXEC` at `0x1000000`, entry `_start` |
| `hi/hi.c` | lab 09's program (below) |
| `hostile-kwrite/hostile-kwrite.c` | lab 12: stores one byte at a kernel address and is killed |
| `hostile-badnum/hostile-badnum.c` | lab 12: three syscall numbers the kernel does not serve → `sys.badnum.enosys` |
| `hostile-badptr/hostile-badptr.c` | lab 12: `write(1, p, 16)` with three pointers it may not pass → `sys.badptr.efault` |
| `hostile-spin/hostile-spin.c` | lab 12: spins with no syscall; nonce-derived GPR + SSE pattern → `regs.survive.preempt`; a `noise` instance |
| `args/args.c` | lab 13: argv, envp, auxv and the entry `rsp` → the five `exec.*` U rows |
| `init/init.c` | labs 14–15 (and on): the course's `init`, which runs each lab's programs (below); reports `fork.orphan.to_init` (14) and `dup2.shared.flags` (15) |
| `forker/forker.c`, `forker/forker.h` | lab 14: `fork.wait.codes`, `fork.child.write.invisible`, `wait.echild.no_hang`, and the orphan `init` reports; `forker.h` holds the codes both derive |
| `producer/producer.c`, `producer/stream.h` | lab 15: the writing end of `init`'s pipes; `stream.h` is the nonce-seeded stream both ends compute |
| `consumer/consumer.c` | lab 15: the reading end → `pipe.chain.checksum`, `pipe.eof.last_writer`, `pipe.cloexec`, `pipe.stress.tiny_writes` |
| `cat/cat.c`, `stat/stat.c` | lab 17: the read-side tools (below, "The W3 programs") |
| `ls/ls.c` | lab 20: it lists with `getdents64`, which is 20's since the split (2026-10-19) |
| `mkfile/mkfile.c`, `rm/rm.c`, `mv/mv.c`, `mkdir/mkdir.c`, `rmdir/rmdir.c` | labs 18–20: the write-side tools (the old 18, re-cut by the split on 2026-10-19 into 18, 19 and 20: `mkfile` 18, `rm` 19, `mv`, `mkdir`, `rmdir` 20) |
| `mounts/mounts.c` | lab 22: the mount table, with each mount point's `st_dev` |
| `sh/sh.c` | lab 23: the course's shell (RATIFY Q6) |
| `build.sh` | builds every program into `bin/`, checks each binary, writes and reads back `bin/boot.archive`, writes `bin/SHA256SUMS` |
| `bin/` | the built programs, `boot.archive` and `SHA256SUMS` — committed, shipped |

## Who builds it, and how

A maintainer runs `./build.sh` by hand with the pinned zig; no CI workflow runs it yet, so the
committed `bin/SHA256SUMS` is checked only by the next such run, against its own output. It refuses any `zig version` but `0.17.0`, compiles each program with
§A.8's flags plus the measured additions TOOLCHAIN.md explains (`-mno-sse -mno-sse2 -mno-mmx
-msoft-float`, `-fno-builtin`, `-g0`, `-T user.ld`), and refuses a binary that is not `ET_EXEC`
x86-64, whose first `PT_LOAD` is not file offset 0 at `0x1000000`, that has a `PT_DYNAMIC`,
`PT_INTERP` or `PT_TLS` or an undefined symbol, or whose disassembly names `xmm`, `ymm`,
`movaps`, `movups`, `movdqa`, `movdqu`, `fxsave` or `cvt`.

**Layout `flat`** (every lab 09–12 program): exactly one `PT_LOAD`, file offset 0 → vaddr
`0x1000000`, `filesz == memsz`, `R E`. The file's bytes *are* its memory image, so the lab 09–12
vehicle may copy the whole file to `0x1000000` (mapped `USER`) and enter at `e_entry`, with no
ELF loader. `hi` today: entry `0x10000f0`, `filesz = memsz = 0x189f`, 7,104 bytes on disk, plus
a `PT_GNU_STACK` (`RW`, `memsz` 0, so §A.5's 16 MiB default applies). Every program enters at
`0x10000f0`.

**Layout `flat-sse`** (`hostile-spin` only): `flat`, and the SSE check allows exactly the `movq`
lines between a general register or memory and `xmm0`/`xmm1` — 10 of them today, all in its
inline asm. Any other SSE line (`pxor`, a `movq` to `xmm2`, anything the compiler emits) fails the
build. It is the one deliberate exception to "no SSE before 23" (below, `hostile-spin`).

**Layout `elf`** (lab 13 on, placed by the ELF loader): one `R E` `PT_LOAD` at `0x1000000` as
above, plus a `RW` `PT_LOAD` at `0x1004000` (file offset `0x4000`) when the program has
`.data`/`.bss`: `forker` (8 bytes of `.data`, `filesz = memsz = 8`: the global the fork row
writes) and `init` (`filesz` 0, `memsz` 0x50: `.bss` only — its directory and nonce pointers).
`args`, `producer` and `consumer` have no writable data and stay one segment. The W3 programs
(labs 17–23) are all `elf`: `rm`, `mv`, `mkdir`, `rmdir` are one segment; the others have a
`.bss`-only `RW` segment (`filesz` 0) for their buffers.

| program | layout | bytes | `PT_LOAD`s (R E `filesz` · RW `filesz`/`memsz`) | sha256 |
|---|---|---|---|---|
| `hi` | flat | 7104 | `0x189f` | `8e44b87c522916b87caab8a13e3067c4ec5743bbac4aecfe7d927f5dff244dc7` |
| `hostile-kwrite` | flat | 3816 | `0xbbd` | `33d98e4f29bb4177acbc70d7ce812eca66bdc0b026abd668f40304fc53291047` |
| `hostile-badnum` | flat | 4104 | `0xcb8` | `0409f3891e13bfe353216af9eda5b13b184af947af1a1cde100581193ddf721a` |
| `hostile-badptr` | flat | 4760 | `0xf48` | `b6f1c0e0c12388ddbfb0da0e56d25c7a345cbfacaa7cb4414370e64d75f533ea` |
| `hostile-spin` | flat-sse | 8640 | `0x1e78` | `002ea10a3dfaf38a1dacb9283f4af1f440bd512ee9fa01d7a05e55554af976d4` |
| `args` | elf | 10064 | `0x240c` | `bf8a80598fc034baa81dd182805d4ae05f6e18721febcec79f14055fd397b660` |
| `init` | elf | 34016 | `0x713e` · `0`/`0x50` | `d2dd69b4228abb1843ce17b20b4a9b4829db071921004304ee0dfec8bf820732` |
| `forker` | elf | 17504 | `0x38f4` · `0x8`/`0x8` | `150cdfb5efd7075e12f5cb1439d2f73a7eb01ea725a41f6f91d20e40f2264741` |
| `producer` | elf | 7640 | `0x1acc` | `fdce8ead361a6d7f884f53332fde169524450ee45f55057b2044963b04bbf6f6` |
| `consumer` | elf | 15384 | `0x38cd` | `ee14e6fed940c9327b9bd013fea6ee56ec28391046f9caa3e451b8792d136414` |
| `ls` | elf | 9112 | `0x1420` · `0`/`0x7020` | `72fc4e4253d54c90f6862cef1d7f3fbb2fea11732565f0d4ed8ba62bbddab5d5` |
| `cat` | elf | 4960 | `0x9c0` · `0`/`0x1000` | `16dc8cb29d6ee55c6dd09f47566712095336258ad9d1fd69fd5d23782fc80df6` |
| `stat` | elf | 9088 | `0x11c8` · `0`/`0x1018` | `d536f5badbd609d97e7fc32c814833e01c8cd01a3032d92e5679f7742f9f1324` |
| `mkfile` | elf | 4960 | `0xba8` · `0`/`0x1018` | `d7190253ce295de336ccc6fe229ebae6f0b27fa4491888d62e3d375640ebb5bb` |
| `rm` | elf | 2960 | `0x8b8` | `1e9d2019670786f81c4f2ac3c9fb75f75c6e1d8848228cc8ed7e8b7ee3ad3590` |
| `mv` | elf | 2912 | `0x888` | `742be4dba00ded7753254200afc7760f789ca631dfe00a72e5c55fefd8bf4b69` |
| `mkdir` | elf | 3016 | `0x8e8` | `91aa188d68099258e21d0e88ecb68905e14c1264489a59d8fd6f5f6fda38244e` |
| `rmdir` | elf | 3016 | `0x8e8` | `0efa7a37a73a6da460d13cafa14151a8875959bc21b84f0e1d6e5e11222fdace` |
| `mounts` | elf | 9184 | `0x1500` · `0`/`0x1430` | `af2e602b7867acdd8eef455878e788be1088ba0bafb8c20b7d3451aac8ffb9af` |
| `sh` | elf | 9312 | `0x1b30` · `0`/`0x2cf8` | `abcd6570a3d0fe4517afacb45e4db305b574a1c253955d548c5a508c0a882eaa` |
| `boot.archive` | — | 113768 | — | `80bcecb3f8b685e18939656639c59e32bc1746498bacac6aac5c60b29c20841b` |

`bin/SHA256SUMS` is the authority; this table is its copy as of 2026-10-19 (forker rebuilt for lab 14's `getppid` and `sched_yield` checks; then init and consumer for lab 15's fix round 1: `dup2.shared.flags` reads `F_GETFL` before, during and after the set and on a second open file, and `pipe.cloexec` checks an unmarked fd above 2 when argv names one; then both again for its fix round 2: that fd is drawn from the nonce, `dup2.shared.flags` uses eight nonce-shuffled fds and reads `F_GETFL` on another pipe's read end; then both again for its fix round 3: `pipe.cloexec` checks eight nonce-drawn fds, some marked with `F_SETFD` and some not, and `dup2.shared.flags` holds a different flag on two open files at once).

## The protocol every program follows (CONTRACT §A.3)

- **`argv = [name, nonce_hex, index, ...]`.** `nonce_hex` is the boot's 32 hex characters; `index`
  is the KTAP number the kernel half (or `init`) assigned to the program's **first** row. A program
  that reports several rows numbers them `index`, `index+1`, … in the order its own header
  comment lists (the number is cosmetic — the judge keys by id — but the plan line counts them).
  Arguments after the third are per program (below). `envp` and auxv are not read before lab 13.
- **One result line per row, through `write(1)`:** `ok <index> - <id>` or `not ok <index> - <id>`.
  Evidence goes on its own `# …` line *before* the result. Each line leaves in one `write` call
  (`struct line` in `sys.h`), so a line arrives whole when two programs share the console.
- **A malformed argv** (fewer than three entries, a non-decimal index) prints
  `not ok 0 - <id>` and exits 2. A nonce that is not 32 hex characters makes the row `not ok`
  (§A.2: no nonce, no derived row).
- **`derive(nonce, id)`** is §A.2's function, the same as the cockpit's `ktap::derive`:
  `splitmix64(lo ^ hi ^ fnv1a64(id))`, `lo`/`hi` = the nonce's bytes 0–7 and 8–15 read
  little-endian. A program needing a small value takes `derive(...) % n`.

### `derive` test vectors

Computed by `sys.h`'s `derive()` (host build) and by an independent Python implementation of
§A.2; they agree. The Rust `ktap::derive` must print the same.

| nonce | id | `derive` |
|---|---|---|
| `00000000000000000000000000000000` | `ring3.cs.rpl` | `1a6ff971a946251f` |
| `0123456789abcdef0123456789abcdef` | `ring3.cs.rpl` | `1a6ff971a946251f` |
| `3f2a9c01d84e7b6655aa10ee93c7f402` | `ring3.cs.rpl` | `6573f0772b76d5cb` |
| `0123456789abcdef0000000000000000` | `ring3.cs.rpl` | `c1b7c1ce5670bfdd` |
| `3f2a9c01d84e7b6655aa10ee93c7f402` | `proc.two.aspaces.order` | `82f093abb625ad9b` |
| `3f2a9c01d84e7b6655aa10ee93c7f402` | `a` | `2a93111269eb06c5` |

The first two are equal on purpose: a nonce whose halves are equal has `lo ^ hi = 0`, so those
two rows cannot tell a little-endian read from a big-endian one. Rows 3–6 can (a big-endian read
gives `1a52f30bca27a7f9` and `38f09bf023357451` for rows 3 and 4). Known answers the parts must
also meet: `fnv1a64("a") = af63dc4c8601ec8c`, `fnv1a64("") = cbf29ce484222325`,
`splitmix64(0) = e220a8397b1dcdaf`.

## `hi` (lab 09)

Run as `["hi", <nonce>, "1"]`. It reads `CS` (`mov %cs`), and prints, one `write` each:

```
hi: cs=0x2b rpl=3 nonce=<argv[1]>
# derive=<16 hex: derive(nonce, "ring3.cs.rpl")>
# getpid=<what getpid returned>
# ioctl(TIOCGWINSZ)=<return> rows=<ws_row> cols=<ws_col>
ok 1 - ring3.cs.rpl
```

The first line carries the `CS` and RPL it read (`0x%x`, decimal), so a ring-0 run prints
`cs=0x8 rpl=0` and `not ok`. The result is `ok` only for `CS == 0x2b`, RPL 3 and a 32-hex nonce.
Then `exit(0)`. Syscalls used: `write` (1), `getpid` (39), `ioctl` (16), `exit` (60) — §A.4's lab
09 arms. It needs **336 bytes of stack** (measured from its disassembly: the deepest chain is
`_start` → `_start_c` → `main` → `line_sdec` → `line_flush`), against the vehicle's 16 pages. It
touches no memory outside its own segment, its stack and the argv strings.

## Lab 12: the hostile programs

Each is run by the kernel half as `[name, nonce, index]`, flat, like `hi`. A program reports
exactly the U rows CONTRACT §B.2-12 gives it; the K rows are the vehicle's.

| program | prints | rows it reports | syscalls (measured, strace 6.19 on Linux 7.0) |
|---|---|---|---|
| `hostile-kwrite` | `# hostile-kwrite: writing one byte to 0xffff800000001000`, then stores there | none while the kill works (`kill.kwrite` is the vehicle's, from §A.6's line, which a correct kernel prints as `reason=pf-kernel-addr … addr=0xffff800000001000`); if the store *returns* it prints `not ok <index> - kill.kwrite`, which §A.2 (d) makes a failed row | `write`×1, then the fault |
| `hostile-badnum` | one `#` line per call with what came back | `sys.badnum.enosys`: `syscall(1000)`, `syscall(-1)` and `syscall(158)` (`arch_prctl(ARCH_GET_FS, &v)`: in §A.4's table, its arm is lab 25's) all return `-38` | 1000, -1, 158, `write`×4, `exit` |
| `hostile-badptr` | one `#` line per call | `sys.badptr.efault`: `write(1, p, 16)` returns `-14` for `p = 0`, `0x0000100000000000` (user half, unmapped) and `0xffff800000001000` (kernel half) | `write`×7, `exit` |
| `hostile-spin` | `# hostile-spin: seed=<16 hex>, checking rbx r12-r15 xmm0 xmm1`, then after the checks `# regs.survive.preempt: 64 checks x 1048576 iterations; tsc per check min=… max=…` | `regs.survive.preempt` (derived), then spins forever with no syscall | `write`×3, then none |

**`hostile-spin`, the reporter and the noise.** The reporter loads seven values,
`splitmix64(derive(nonce, "regs.survive.preempt") + k)` for k = 0..6, into `rbx`, `r12`–`r15`,
`xmm0`, `xmm1`; spins 64 × 2²⁰ iterations of a loop that touches none of them, comparing all
seven with memory after every 2²⁰; prints `ok` if all 64 comparisons matched (else a `#` line
naming the register and `not ok`); then spins on, checking, with no syscall. Run as
`[hostile-spin, nonce, index, "noise"]` it prints one `#` line and rewrites the same seven
registers with the complement seed's values on every iteration, forever, reporting nothing.
**The vehicle runs both** (reporter and noise) so that a kernel that does not save and restore
`xmm` (FXSAVE/FXRSTOR, §A.5) or a GPR fails the reporter's row: the kernel is soft-float and every
other pre-23 program has no SSE, so without a second SSE writer nothing would disturb `xmm0`.
`proc.two.aspaces.order` is a K row (the vehicle runs `hi` twice); no program here reports it.

## Lab 13: `args`

Run by the vehicle's `execve` as

```
argv = ["args", <nonce>, <index>, <derive(nonce, "exec.argc_argv") as 16 lowercase hex>]
envp = ["SBOOT_NONCE=<nonce>"]      (other entries are allowed and printed)
```

It prints `# argc=…`, every `# argv[i]=…`, every `# envp[i]=…`, then five rows, `index` …
`index+4`: `exec.argc_argv` (argc is 4, `argv[1]` is 32 hex, `argv[3]` equals the derived hex),
`exec.envp` (an entry is exactly `SBOOT_NONCE=` + `argv[1]`), `exec.rsp.aligned` (the `rsp` at
`_start`, computed as `(u64)argv - 8`, is 16-aligned — `crt0.c` is unchanged, so `hi`'s bytes
are too), `exec.auxv.phdr_pagesz` (each auxv pair is printed; `AT_PAGESZ` = 4096, `AT_PHENT` = 56,
and among the `AT_PHNUM` headers at `AT_PHDR` is an executable `PT_LOAD` at `0x1000000` — the
headers live inside that segment, at `0x1000040`), `exec.auxv.random.readable` (the 16 bytes at
`AT_RANDOM`, printed in hex). The two auxv rows come last because they dereference the kernel's
pointers. Syscalls: `write`, `exit`. The `exec.reject.*` rows are the vehicle's.

## `init` (lab 14 on)

```
argv = [<the path the vehicle exec'd init from>, <nonce>, <index>, <lab>]
envp = whatever the vehicle passes; init hands it to every program unchanged
fds 0, 1, 2 = the console
```

`init` runs each program from the directory of its own `argv[0]` — `"/init"` (labs 14–16, the
archive at `/`) runs `/forker`; `"/boot/init"` (17 on) runs `/boot/forker`; `./init` on a Linux
host runs `./forker`. It reaps with `wait4(-1)` until `-ECHILD`, prints a `#` line per reaped
pid (its own child, or an orphan reparented to it), and exits 0 when the lab's programs are
done; the vehicle then prints `# sboot end`. An unknown lab or a malformed argv: one `#` line,
exit 2.

- **Lab 14:** `fork` + `execve` of `forker` as `[forker, nonce, index]` (forker's rows are
  `index`..`index+2`); `init`'s own row is `ok <index+3> - fork.orphan.to_init`, printed when an
  orphan it did not fork exits with `forker.h`'s `orphan_code(nonce)` (150..249). Syscalls:
  `fork`, `execve`, `wait4`, `getpid`, `write`, `exit`.
- **Lab 15:** `pipe2(p, 0)` and `pipe2(q, O_CLOEXEC)`; then `init`'s own row,
  `ok <index+4> - dup2.shared.flags` (below, "Lab 15"); then
  `producer [producer, nonce, index, chain]` with `p[1]` on fd 1 and `consumer [consumer, nonce,
  index, chain, <q[0]>, <q[1]>, keep]` with `p[0]` on fd 0 (each child `dup2`s and closes both `p`
  ends; `q` stays open in both across `execve`, which must close it). Between the two forks `init`
  makes the eight fds of `cloexec_fds(nonce)` (`producer/stream.h`: fds 11..22 shuffled by
  `derive(nonce, "pipe.cloexec.fds")`, the first eight) with `cloexec_make`: two more pipes
  (`pipe2(r, 0)`, `pipe2(s, 0)`), then slot *i*'s fd is `dup2` of `p[0]`, `p[0]`, `r[0]`, `r[0]`,
  `r[1]`, `s[0]`, `s[1]`, `r[1]` for *i* = 0..7, and `F_SETFD(FD_CLOEXEC)` marks slots 1, 2, 5 and 7
  (`cloexec_marked`); slots 3 and 6 are marked and then unmarked with `F_SETFD(0)`. So each kind has
  an fd that shares its open file with the other kind and an fd that shares it with nothing else
  of its kind. `init` closes `r` and `s`, so only the consumer is forked holding the eight, and
  closes them after (fix round 1, 2026-10-19, one fd; drawn from the nonce in fix round 2; eight,
  marked by `F_SETFD`, in fix round 3 — so an `execve` that closes fds by number, by range or by
  whether they are shared, instead of by the bit, fails; the numbers never reach the kernel in
  argv: the consumer derives the same eight). A child whose `dup2` onto fd 0 or 1 fails prints
  `# init: dup2(<from>, <to>) = <errno> in the child; <name> not run` and exits 127 before its
  `execve`, so no program writes its stream to the console. `init` closes all four ends
  and reaps both; then a new pipe with `producer … tiny` | `consumer [consumer, nonce, index+3,
  tiny]`. Syscalls: `pipe2`, `dup2`, `fcntl`, `close`, `fork`, `execve`, `wait4`, `getpid`,
  `write`, `exit`.

## Lab 14: `forker`

Rows `index`..`index+2`, in this order (the header comment of `forker.c` has the detail):
`fork.wait.codes` — N = `derive(nonce, "fork.wait.codes") % 4 + 2` children, child *i* exits with
`1 + ((derive(...) >> 8(i+1)) & 0xff) % 100`; N `wait4(-1)` calls return N distinct pids, each with
`WIFEXITED` and its child's code, and then `wait4(-1, WNOHANG)` is `-ECHILD`.
`fork.child.write.invisible` — a `.data` global holds A = `derive(nonce, id)`; a child writes ~A
and exits 0 when its `getppid()` is the pid forker read from `getpid()` before the fork (else
`PPID_WRONG` = 120); reaped, the parent still reads A. `wait.echild.no_hang` — `wait4(-1)` before any
fork is `-ECHILD`; `wait4(pid, WNOHANG)` on a child in a 2000-yield loop is 0; `wait4(pid, 0)`
then reaps it with code 7; a child that exits 9 at once is collected by `wait4(child, WNOHANG)`
within 20 `sched_yield()`s, each answering 0 (added 2026-10-19, lab 14 B9). Then the orphan setup for `init`'s row: a child forks a grandchild and
exits at once; the grandchild yields until `getppid()` is `init`'s pid (forker's own `getppid()`),
at most 200,000 times, and exits with `orphan_code` — or 3 if it never was reparented. Syscalls:
`fork`, `wait4`, `getpid`, `getppid`, `sched_yield`, `write`, `exit`.

## Lab 15: `producer` and `consumer`

The stream (`producer/stream.h`, one copy for both): seed = `derive(nonce,
"pipe.chain.checksum")`; length = `4096 + seed % 8192`; byte *i* = bits `8(i%8)`..`8(i%8)+7` of
`splitmix64(seed ^ (i/8))`; checksum = FNV-1a 64 over the bytes. `producer chain` writes all but
the last 64 bytes to fd 1 in `1 + splitmix64(seed + 2³² + j) % 1024`-byte writes with a
`sched_yield` after each, then `dup(1)`, `close(1)`, 20 yields, the last 64 bytes through the
dup, `close`. `producer tiny` makes 10,000 `write(1, &b, 1)` calls, byte *i* = `i % 251`. The
producer reports no row; its `#` lines go to fd 2.

`consumer chain` reads fd 0 to EOF in 512-byte reads, then once more: `pipe.chain.checksum`
(length and FNV-1a both match), `pipe.eof.last_writer` (the first 0 came after exactly the
stream's length — a kernel that signals EOF when *one* fd of the write end closes loses the
tail — and the read after it is 0 again), `pipe.cloexec` (`fcntl(F_GETFD)` is `-EBADF` on both
`q` numbers, not on fd 0, and, when argv[6] is the word `keep`, `-EBADF` on each marked fd of
`cloexec_fds(nonce)` and exactly 0 on each unmarked one: fds above 2 that fork carried, which
`execve` must close or keep by their bit alone; added 2026-10-19, fix round 1, against an `execve`
that closes every fd above 2, drawn from the nonce in fix round 2, against one that keeps only the
fd argv names, and eight fds in fix round 3, against one that keeps a range or the shared fds; argv[6] is optional, because lab 23's shell
types `consumer … chain 3 4` without it, and then no fd above 2 is checked). `consumer tiny` yields 200 times (`TINY_YIELDS`) before its first
read, so the producer meets a full ring and blocks, then reads: `pipe.stress.tiny_writes` (10,000
bytes, in order). Syscalls: producer `write`, `dup`, `close`, `sched_yield`, `exit`; consumer
`read`, `fcntl`, `sched_yield`, `write`, `exit`.

**`dup2.shared.flags`** (reported by `init`, row `index+4`; consumer's rows stay
`index`..`index+3`). Before forking anyone, `init` shuffles fds 20..31 with
`derive(nonce, "dup2.shared.flags.fds")` (`dup2_fds`; fix round 2, 2026-10-19 — the row used fd
20 alone before) and takes the first four as A and the next four as X. *Since fix round 3
(2026-10-19)* two open files hold different flags at once: group Q is `q[0]` and the four A, group
P is `p[0]` and the four X, and each dup is `dup2` of its group's first fd or of an earlier dup,
picked by `derive(nonce, "dup2.shared.flags.order")`. `init` then sets, in an order the same
value picks, `O_NONBLOCK` with `fcntl(F_SETFL)` through one member of Q, `O_APPEND` through one
member of P, and `O_APPEND` through `q[1]` (a third open file, on Q's pipe), and reads `F_GETFL`
through every fd. `ok` needs all of: every Q shows `O_NONBLOCK` and not `O_APPEND`, every P and
`q[1]` `O_APPEND` and not `O_NONBLOCK`; nothing showed either flag before the sets; after
`F_SETFL(0)` through another member of Q, every Q shows neither while every P and `q[1]` still
show `O_APPEND` alone (each open file keeps its own flags); after `F_SETFL(0)` through `p[0]` and
`q[1]`, neither shows a flag; and close-on-exec per fd: `F_GETFD` exactly `FD_CLOEXEC` on `q[0]` and
0 on every A (after `dup2(q[0], q[0])`, which must change nothing). Flags kept anywhere but in
each open file — one global, or one value tagged by the pipe, by an fd's `dup2` history or by the
last file set — cannot hold the three at once. `#` lines record every value, with the fd numbers
printed as used; every A and X is closed before the children are forked. On `not ok`, one `#`
line per failed part names its causes: `dup2` built a second `OpenFile`, or `F_SETFL` stored the
flag per fd instead of in `OpenFile.flags`; `F_GETFL` reports a flag the open file does not hold,
or `F_SETFL(0)` did not clear it; clearing one open file's flags changed another's; `pipe2` did not
mark its fds under `O_CLOEXEC`, or `dup2` copied the mark (or cleared it when both fds are one). A `dup2` or `fcntl` that answers -38 gets one line saying the arm is
missing instead. *Ruled 2026-10-17 (class B Q5, option b):* the
row replaces `dup2.shared.offset`, because a status flag set through one fd and read through its
dup *is* the shared open file, not a stand-in for it — and no object with an offset exists before
lab 17 (a pipe and the console have none; `lseek` is 17's). Offset coverage, if wanted, goes
inside lab 17's `ext2.lseek`.

## The W3 programs (labs 17–23)

Ordinary tools, not row reporters: they take the usual argv (no nonce, no index) and print no
KTAP line, so a vehicle or `sh` can run them on any lab from the one they arrive in. They share
`sys.h`'s lab 17–23 wrappers, `errno_name`, `err_line` and the buffered writer `struct obuf`.

- **Errors**, every program: one line on fd 2, `<prog>: <path>: <name>`, where `<name>` is the
  errno's name (`ENOENT`, `ENOTDIR`, `EISDIR`, `EEXIST`, `ENOTEMPTY`, `EXDEV`, … — `errno_name`
  in `sys.h`) or `-N` for one it does not name; the program goes on to its next operand and exits
  1. A usage error is `<prog>: usage: …` and exit 2. A refused write to fd 1 is
  `<prog>: stdout: <name>`.
- **Paths:** before lab 23 a relative path has no cwd to be relative to (§A.4 `open`), so a
  vehicle passes absolute ones; `ls` with no operand lists `.`.

| program | lab | usage | what it does | syscalls (measured, `strace -f -c`, lab-linux01) |
|---|---|---|---|---|
| `ls` | 20 | `ls [-a] [path…]` | `open(O_RDONLY\|O_DIRECTORY\|O_CLOEXEC)`, `getdents64` into a **512-byte** buffer until 0 (a directory of more than ~16 entries takes several calls), names sorted bytewise, one per line; without `-a` a name starting with `.` is left out. A non-directory operand (`ENOTDIR`, then `stat` finds it) is printed as given; with several operands each directory follows a `<path>:` line and a blank line separates outputs, in argument order. A record with `d_reclen` < 20, running past the returned count, or with no NUL is `EIO`; over 1,024 names or 16 KiB of names in one directory is `ENOMEM` | `open`, `getdents64`, `stat`, `close`, `write`, `exit` |
| `cat` | 17 | `cat [path…]` | each file (or fd 0 for none or `-`, named `<stdin>` in errors) to fd 1 in 4,096-byte `read`s, short writes retried; a directory is `read`'s `EISDIR` | `open`, `read`, `write`, `close`, `exit` |
| `stat` | 17 | `stat <path>…` | one `stat(2)` per path, then 7 fixed lines: `file=<path>`, `st_dev=`, `st_ino=`, `st_mode=0<octal>` (e.g. `0100644`, `040755`), `st_nlink=`, `st_size=`, `st_blocks=` (512-byte units), decimals from the 144-byte x86-64 `struct stat` | `stat`, `write`, `exit` |
| `mkfile` | 18 | `mkfile <path> [word…]` | `open(O_WRONLY\|O_CREAT\|O_TRUNC\|O_CLOEXEC, 0644)`, then the words joined by one space plus `\n` in **one** `write` while the line fits 4,096 bytes; no words: an empty file. At lab 18 a new path only: over a file that is there, `O_TRUNC` needs lab 19's `truncate` | `open`, `write`, `close`, `exit` |
| `rm` | 19 | `rm <path>…` | `unlink` each (a directory: `EISDIR`) | `unlink`, `write`, `exit` |
| `mv` | 20 | `mv <from> <to>` | one `rename` (across file systems: `EXDEV`; nothing is copied) | `rename`, `write`, `exit` |
| `mkdir` | 20 | `mkdir <path>…` | `mkdir(path, 0755)` each, no `-p` | `mkdir`, `write`, `exit` |
| `rmdir` | 20 | `rmdir <path>…` | `rmdir` each (not empty: `ENOTEMPTY`) | `rmdir`, `write`, `exit` |
| `mounts` | 22 | `mounts [file]` | reads `/proc/mounts` (or `file`) in 512-byte `read`s; each line is Linux's shape, `<source> <mount point> <fstype> …`, the mount point's `\NNN` escapes decoded; prints `<mount point> <fstype> st_dev=<decimal>` from a `stat` of it. A line with under three fields or over 1,023 bytes is `mounts: <file>: EINVAL`; a refused `stat` is `mounts: <mount point>: <name>` | `open`, `read`, `stat`, `close`, `write`, `exit` (all lab 17 calls) |
| `sh` | 23 | `sh` | below | `read`, `write`, `open`, `close`, `pipe2`, `dup2`, `fork`, `execve`, `wait4`, `chdir`, `getcwd`, `exit` |

`mkdir` and `rmdir` are not in §A.8's first list; they were added 2026-10-17 (§A.8) because
without them the `mkdir`/`rmdir` arms (lab 18's then, lab 20's since the split, 2026-10-19) have no caller a learner can run, and at lab 23's
shell `mkdir d` then `cd d` is the `chdir` demonstration. `mounts` reads the table rather than
probing paths because nothing in user space can list mounts without `/proc/mounts`; its one
line per mount carries both lab 22 facts a learner checks by hand — every mount is listed, and
each answers its own `st_dev`. It needs `/proc/mounts` in Linux's field order (`source`,
`mount point`, `fstype` first).

### `sh` (lab 23; `sh/sh.c`, 283 lines)

- **The loop:** prompt **`$ `** (dollar, space) on fd 1, no newline; then fd 0 is read **one
  byte per `read` call** up to `\n`, so a command that reads fd 0 gets the bytes after its own
  line and never ones the shell took. On the console this means line mode must hand one line out
  across several reads, as Linux's canonical mode does. `read` = 0 with nothing typed ends the
  shell with the last status (a partial line before EOF runs first); a refused `read` prints
  `sh: <stdin>: <name>` and exits 1. An empty line keeps `$?`.
- **A line:** words split on spaces and tabs (`\r` counts as a space); `>` (fd 1 to a file,
  `O_WRONLY|O_CREAT|O_TRUNC`, 0644), `<` (fd 0 from a file, `O_RDONLY`) and `|` (up to 8
  commands; `pipe2(O_CLOEXEC)`) need no spaces around them; a second `>` (or `<`) in one command
  replaces the first, which is never opened. `"…"` quotes spaces and the operators and may join a
  word (`a"b c"d` is `ab cd`; `""` is an empty word); there is no other quoting, no globbing, no
  job control, no assignment. `$?` (the last status, decimal) and `$NAME` (NAME's value in the
  shell's environment; empty if unset; NAME = `[A-Za-z_][A-Za-z0-9_]*`) expand inside and outside
  quotes; any other `$` is itself.
- **Builtins:** `cd [dir]` (no dir: `/`), `exit [n]` (no n: the last status; `n & 0xff`),
  `pwd`, `echo [word…]` (words joined by one space, then `\n`; no options). `cd` and `exit` alone
  on a line run in the shell itself and open no redirection file; in a pipeline, and `pwd` and
  `echo` always, a builtin runs in a forked child that takes the pipes and redirections like any
  command (so `pwd` exercises the child's inherited cwd).
- **Running:** a word with no `/` runs as `/bin/<word>`; one with a `/` runs as given (relative
  to the cwd). Children get the shell's `envp` unchanged. The parent closes its pipe ends after
  each fork and `wait4`s each command's pid in order; the status is the last command's exit code,
  or 128 + the signal for a kill (§A.6: `SIGSEGV` → 139).
- **Messages** (fd 2; the status `$?` gets in brackets): `sh: <name>: ENOENT` no such command
  [127; any other `execve` refusal 126] · `sh: <path>: <name>` a redirection's `open` refused [1;
  the command does not run] · `sh: syntax: <tok>` with `<tok>` one of `|`, `<`, `>`, `newline`,
  `"` [2; nothing runs] · `sh: line too long` over 1,023 bytes typed or 2,047 after expansion [2]
  · `sh: too many words` over 63 in one command [2] · `sh: too many commands` over 8 [2] ·
  `sh: fork: <name>`, `sh: pipe: <name>` [1] · `sh: cd: <dir>: <name>` [1] · `sh: pwd: <name>`
  [1] · `sh: exit: <arg>: EINVAL` [2; the shell stays].
- **Lab 23's rows** (§B.2-23, as amended 2026-10-17): `sh.exit_code.shown` is `echo $?` after a
  command that fails (`cat /nonexistent` → `1`, a missing name → `127`); `sh.remount.survives`
  writes its file with `echo <text> > <path>` through `sh`'s own redirection, then `exit`;
  `sh.hi.by_name` can type `hi $SBOOT_NONCE 1`, `$SBOOT_NONCE` coming from the environment `init`
  passes on.

## Native check on Linux

These are Linux-ABI binaries, so each runs under a real x86-64 Linux as an oracle before a
course kernel exists (2026-10-17, lab-linux01, Linux 7.0, nonce
`3f2a9c01d84e7b6655aa10ee93c7f402`): `args` 5/5, `hostile-badptr` ok, `hostile-spin` ok (also
with the noise instance on the same CPU), `init … 14` 4/4 when run under a child subreaper
(`prctl(PR_SET_CHILD_SUBREAPER)`, a host-only wrapper, never shipped; without it Linux reparents
the orphan to its own init and the row is `not ok`), `init … 15` 5/5 (re-run 2026-10-17 with
nonce `c273b542791488f55d571423227d6f66` after `dup2.shared.flags` landed). Expected differences:
`hi` prints `cs=0x33` (Linux's user code selector) and `not ok`; `hostile-badnum` prints `not ok`
because Linux serves `arch_prctl`; `hostile-kwrite` dies of `SIGSEGV` at
`0xffff800000001000`.

**The W3 programs** (2026-10-17, lab-linux01, Linux 7.0.0-34, strace 6.19, in one `mktemp -d`
directory on tmpfs, nonce `c69d739d5653668a40845dc501d20036`): `ls many` and `ls -a many` on a
300-entry directory are byte-identical to `LC_ALL=C ls -1` / `ls -1a` (20 `getdents64` calls of
512 bytes); `cat` of a 20,000-byte file is byte-identical through a path and through fd 0 (5
data reads + the 0); `stat` equals `stat -c '%d %i %f %h %s %b'` on a file, a directory, `.`,
`/proc` and `/dev/null`; `mkfile`/`mv`/`rm`/`mkdir`/`rmdir` give the bytes and errnos expected
(`EXDEV` into `/proc`, `EISDIR`, `ENOTEMPTY`, `EEXIST`, `ENOENT`); `mounts` lists 39 of
`/proc/mounts`' 43 lines and names the other 4 (`EACCES`, snapd's namespaces, not root). `sh`
ran a script on fd 0 through every builtin, `>`, `<`, a 4-stage pipe, `producer | consumer`
(3/3 `ok`), `$?` (0, 1, 127, 143 for a `SIGTERM`), quoting, `$NAME`, each syntax message, a
missing command and EOF. Every syscall each program executed is in §A.4 at or before its lab.
`hi` under `sh` prints `cs=0x33` and `not ok` there, as expected on Linux.

## The boot archive (`bin/boot.archive`, CONTRACT §A.7)

What labs 13–16 mount at `/` and 17 on at `/boot`. `build.sh` writes it after the programs and
reads it back with a separate reader that matches every entry to `bin/<name>`:

```
offset 0   u64 LE  magic 0x5B007B00A2C4
offset 8   u32 LE  count
offset 12  count × { name: 56 bytes, UTF-8, NUL-padded; offset: u64 LE; size: u64 LE }   (72 bytes each)
           zero padding to a multiple of 8, then each file's bytes at its offset, 8-aligned, zero-padded
```

Entries are sorted by name; there are no timestamps, so the archive's bytes follow from the
binaries alone. Today: 10 entries, 113,768 bytes, the table ends at byte 732, the first data at
`0x2e0`:

| # | name | offset | size |
|---|---|---|---|
| 0 | `args` | `0x002e0` | 10064 |
| 1 | `consumer` | `0x02a30` | 15384 |
| 2 | `forker` | `0x06648` | 17504 |
| 3 | `hi` | `0x0aaa8` | 7104 |
| 4 | `hostile-badnum` | `0x0c668` | 4104 |
| 5 | `hostile-badptr` | `0x0d670` | 4760 |
| 6 | `hostile-kwrite` | `0x0e908` | 3816 |
| 7 | `hostile-spin` | `0x0f7f0` | 8640 |
| 8 | `init` | `0x119b0` | 34016 |
| 9 | `producer` | `0x19e90` | 7640 |

Names carry no leading `/`: mounted at `/`, entry `init` is `/init`. The W3 programs (`ls` …
`sh`) are **not** in the archive (2026-10-17): the image writer copies `bin/` into ext2's `/bin`
(labs 16 on), which is where `sh` looks for a bare name, so the ten entries and the archive's
bytes did not move when they were added.

## Adding a program (a later slice)

1. A directory `<name>/` with its `.c` file(s); `#include "../sys.h"`; `int main(int argc, char
   **argv, char **envp)`. New syscall numbers go into `sys.h`'s `SYS_*` list with §A.4's lab;
   nothing there is renumbered.
2. One line in `build.sh`'s `PROGRAMS`: `"<name>|<name>/<name>.c|flat"` (or `elf` from lab 13,
   when the ELF loader may place a separate `RW` segment for `.data`/`.bss`); and its name in
   `ARCHIVE` if the boot archive carries it (from lab 17 a program rides ext2's `/bin` instead,
   and adding it leaves `boot.archive` byte-identical).
3. Run `./build.sh` with the pinned zig and commit `bin/` with the sources.
4. If the image writer places it on the disk, one row in `FIXTURES.md` (a later slice).
