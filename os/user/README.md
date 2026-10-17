# `user/` — the course's programs

Course-owned (E1): the learner builds the kernel that runs these, never these. Every program is
freestanding C built by the pinned `zig cc` (**[TOOLCHAIN.md](TOOLCHAIN.md)**), run by hand —
no CI workflow runs `build.sh` yet — and the binaries ship under `bin/` beside their sources
(CONTRACT §A.8).

| file | what it is |
|---|---|
| `sys.h` | the raw x86-64 Linux syscall stubs (`syscall0`–`syscall6`), the `SYS_*` numbers (CONTRACT §A.4's), the errno numbers, `read_cs()`, `derive()` (§A.2), and the output helpers — no libc, no SSE |
| `crt0.c` | `_start`: argc/argv/envp off the initial stack, `main`, then `exit(main's return)`; plus `memset`/`memcpy`/`memmove`/`memcmp`, which the compiler may call on its own |
| `user.ld` | the link: `ET_EXEC` at `0x1000000`, entry `_start` |
| `hi/hi.c` | lab 09's program (below) |
| `build.sh` | builds every program into `bin/`, checks each binary, writes `bin/SHA256SUMS` |
| `bin/` | the built programs and `SHA256SUMS` — committed, shipped |

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
a `PT_GNU_STACK` (`RW`, `memsz` 0, so §A.5's 16 MiB default applies).

## The protocol every program follows (CONTRACT §A.3)

- **`argv = [name, nonce_hex, index]`.** `nonce_hex` is the boot's 32 hex characters; `index` is
  the KTAP number the kernel half assigned. `envp` and auxv are not read before lab 13.
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

## Adding a program (a later slice)

1. A directory `<name>/` with its `.c` file(s); `#include "../sys.h"`; `int main(int argc, char
   **argv, char **envp)`. New syscall numbers go into `sys.h`'s `SYS_*` list with §A.4's lab;
   nothing there is renumbered.
2. One line in `build.sh`'s `PROGRAMS`: `"<name>|<name>/<name>.c|flat"` (or `elf` from lab 13,
   when the ELF loader may place a separate `RW` segment for `.data`/`.bss`).
3. Run `./build.sh` with the pinned zig and commit `bin/` with the sources.
4. If the image writer places it on the disk, one row in `FIXTURES.md` (a later slice).
