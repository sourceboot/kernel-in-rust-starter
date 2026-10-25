# The course programs' toolchain — the pin

**`zig cc` 0.17.0.** One pinned toolchain builds every program under `user/` (CONTRACT §A.8).
**The learner never builds these.** A maintainer runs `user/build.sh` by hand (no CI workflow
runs it yet), and the binaries ship built under
`user/bin/`, course-owned, with their sources beside them. Nothing on a learner's machine needs
zig, a C compiler or this file.

## The official 0.17.0 release

From `https://ziglang.org/download/index.json`, key `"0.17.0"`, fetched 2026-10-16 18:24 PDT
(the file's own sha256 at that fetch:
`a6559ba8883382d85d338a3d018dcb9860c91fae5ac3a7a9256cb491f6f12632`). The entry's `"date"` is
**`2026-10-01`**. Quoted fields, `tarball` / `shasum` (sha256) / `size` in bytes:

| host | `tarball` | `shasum` | `size` |
|---|---|---|---|
| `x86_64-linux` | `https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz` | `1cbe9df9f27e6b78d14ccbca43b6703a404ef79ef1c463de901d7f088d4e2026` | 57332648 |
| `aarch64-linux` | `https://ziglang.org/download/0.17.0/zig-aarch64-linux-0.17.0.tar.xz` | `9e8d11661d4ae3bd57702a3832781e23ad151dde5798e16a5ccd503f65234ff8` | 52877280 |
| `aarch64-macos` | `https://ziglang.org/download/0.17.0/zig-aarch64-macos-0.17.0.tar.xz` | `b607e9b9234790a008116ae5bdb71c6243b84b9fb42a53a9e70fde41c06c536a` | 53985220 |
| `x86_64-macos` | `https://ziglang.org/download/0.17.0/zig-x86_64-macos-0.17.0.tar.xz` | `4f9a1c5269aa17ebda5e6d3c2b89d6cbf36f7d2b22a0306e9ab98f25f95529c6` | 59317572 |
| `x86_64-windows` | `https://ziglang.org/download/0.17.0/zig-x86_64-windows-0.17.0.zip` | `b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428` | 100268137 |

No CI workflow runs `build.sh` yet. One that does is to download the tarball for its runner
(`x86_64-linux` on `ubuntu-latest`), refuse it unless its sha256 is the one above, and put it
first on `PATH`. `build.sh` refuses to run under any
`zig version` other than `0.17.0`.

## The build machine as of this pin

The reference build was made on Puneet's Mac mini (M4 Pro, arm64, macOS) with Homebrew's
`zig` 0.17.0 bottle at `/opt/homebrew/bin/zig` (→ `../Cellar/zig/0.17.0/bin/zig`), whose
`zig version` prints `0.17.0`. That is Homebrew's build, not the official `aarch64-macos`
tarball above. The same tree built in two directories on that machine gives the same bytes;
whether the official `x86_64-linux` tarball gives the same bytes has **not** been measured yet. The
first run of `build.sh` on it is that measurement: its `bin/SHA256SUMS` against the committed one, where a difference is
a finding, not a re-commit.

## The flags, and why each one is there

`build.sh` holds the one copy. Measured on the pin, 2026-10-16:

- **`-target x86_64-linux-musl -static -O2 -nostdlib -ffreestanding -mgeneral-regs-only
  -fno-stack-protector`**: CONTRACT §A.8's line for labs 09–24. Its own `_start` (`crt0.c`), its
  own syscall stubs (`sys.h`), no libc.
- **`-mno-sse -mno-sse2 -mno-mmx -msoft-float`**: §A.8's named fallback, because **`zig cc
  0.17.0` does not honour `-mgeneral-regs-only` on x86-64**. Under `-###` it lowers the flag to
  `-target-feature -x87 -mmx -sse` and then appends its own CPU model's `+mmx +sse +sse2 +x87`,
  which win. Built with the flag alone, `hi` disassembled to 31 lines naming `xmm` (the
  vectorised `memset`/`memcpy` loops) and had the same sha256 as a build without the flag. With
  the `-mno-*` flags added — they reach zig's own CPU model — the count is 0. The ineffective
  flag stays on the line so it still reads as the contract's.
- **`-fno-builtin -Wall -Wextra`**: no compiler-recognised loop is turned into a call to the
  `memset` that `crt0.c` itself defines.
- **`-g0`**: zig cc emits DWARF by default, and the DWARF carries the build directory's
  absolute path (5 occurrences in a measured debug build), so the bytes would depend on where
  the tree was checked out.
- **`-T user.ld`**: `ET_EXEC` at `0x1000000`, entry `_start`, headers inside the first `PT_LOAD`.
  `zig cc 0.17.0` rejects `-Wl,--no-rosegment`, `-Wl,-Ttext-segment=…` and `-Wl,-N` ("unsupported
  linker arg"); `-Wl,--image-base=0x1000000` is accepted but leaves two `PT_LOAD`s, the second at
  file offset `0x340` mapped to `0x1001340`, which a flat copy of the file cannot reproduce. A
  linker script passed with `-T` is honoured.

## The `musl` layout (lab 25 on, 2026-10-22)

`hello`, `tls` and `mem` are ordinary static Linux programs: §A.8's lab 25 line, built against
the musl that ships inside zig 0.17.0 (`lib/zig/libc/musl`, no download) with zig's own
`malloc` (`lib/zig/c/malloc.zig`, over `std.heap.SmpAllocator` and `PageAllocator`). The flags,
measured on the pin 2026-10-22:

- **`-target x86_64-linux-musl -static -O2 -Wall -Wextra`**: no `-nostdlib`, no
  `-ffreestanding`, no `crt0.c`, no `-T user.ld` — musl's `_start` and the toolchain's default
  static layout (first `PT_LOAD` `R` at file offset 0 → `0x1000000`; `GNU_STACK` 16 MiB). SSE
  stays on (musl's string and number routines use it); AVX does not appear, and `build.sh`
  refuses a binary in which it does.
- **`-g0 -s`**: `-g0` covers only the program's own object. zig's cached libc objects carry
  DWARF naming the absolute path of the zig install (`/opt/homebrew/Cellar/zig/0.17.0/…`) — and,
  measured, the path of whichever directory first populated the cache — so an unstripped binary
  depends on the machine. `-s` drops `.debug_*` and `.symtab`: each binary then holds 0 strings
  matching `/opt|/Users|/private|homebrew|Cellar`, and the same tree built in a second directory
  gives the same three sha256s (2026-10-22). `-Wl,--strip-debug` gives the same bytes as `-s`
  (`hello`: both 28,008 bytes, sha256 `4f57bed8…`); `-g0` alone leaves 1,403,968 bytes holding 294
  such strings.

## The `port` layout (lab 27, 2026-10-22)

`doomgeneric` is the `musl` layout with somebody else's source: the files `SRC_DOOM` names in
upstream's `Makefile.linuxvt`, compiled in one `zig cc` call with that Makefile's defines
(`-static -O2 -DNORMALUNIX -DLINUX -DSNDSERV -D_DEFAULT_SOURCE`, CONTRACT Appendix (1)), plus
`-g0 -s` for the reasons above and **`-ffile-prefix-map=<source dir>/=`**: upstream's
`Z_ChangeTag` passes `__FILE__`, and zig cc makes every source name absolute, so without it the
binary holds the checkout's path (measured 2026-10-22: 2 such strings; the same tree in a second
directory gave another sha256; with it, 0 strings and one sha256, `2c39dbdf…`, 513,776 bytes).
The Appendix's own line (no `-g0 -s`) gives 3,343,656 bytes here and a sha256 that changes with the
directory, which is why its `9402aedd…` cannot be reproduced elsewhere.

## The `busybox` layout (lab 30, 2026-10-23)

`busybox` is upstream's own build (Kconfig and Makefiles) driven with the pinned zig as its
compiler: `make … CC=<zcc> AR="zig ar" HOSTCC=cc SKIP_STRIP=y KCONFIG_NOTIMESTAMP=1`, where
`<zcc>` runs `zig cc -target x86_64-linux-musl` and drops `-Wl,--warn-common`, `-Wl,--verbose`
and `-Wl,-Map,…` (lld refuses the first; the other two would write files beside the build).
`CONFIG_STATIC=y` gives `-static`; busybox's own release flags (`-Oz`, no `-g`) stand, and
zig strips at that level, so `SKIP_STRIP=y` replaces a `strip` the machine does not have. No
`-ffile-prefix-map` is needed: the source tree is unpacked under a fresh `mktemp -d` each run, and
the binary holds no path of it — measured 2026-10-23, the same 241,160 bytes (sha256
`927cffcd…2c24`) from the trial directory and from `build.sh`'s. `KCONFIG_NOTIMESTAMP=1` is what
keeps the build date out of the banner. The source is the release tarball, checked by sha256
before anything else runs (`busybox/README.md`); then every `musl` check.
