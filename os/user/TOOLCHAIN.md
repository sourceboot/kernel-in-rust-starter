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
