#!/usr/bin/env bash
# user/build.sh -- builds every course program into bin/ with the pinned
# toolchain (CONTRACT §A.8; the pin is TOOLCHAIN.md). A maintainer runs this
# by hand (no CI workflow runs it yet); a learner never does: the binaries ship
# built, beside their sources.
#
#   ./build.sh            build everything, check every binary, write bin/SHA256SUMS
#   ZIG=/path/to/zig ./build.sh     use a zig that is not first on PATH
#   OBJDUMP=llvm-objdump ./build.sh use another disassembler for the SSE check
#
# Adding a program: one directory <name>/ with its .c file(s), one line in
# PROGRAMS below. Nothing else in this file changes.
set -euo pipefail

PIN=0.17.0
ZIG=${ZIG:-zig}
OBJDUMP=${OBJDUMP:-objdump}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BIN=$HERE/bin

# name | sources (relative to user/, crt0.c is always linked) | layout
#   layout "flat": exactly one PT_LOAD, file offset 0 -> 0x1000000, filesz == memsz,
#   so labs 09-12's vehicle may copy the file's bytes to 0x1000000 and enter at
#   e_entry. "elf": loaded by lab 13's ELF loader; may carry a RW segment.
PROGRAMS=(
  "hi|hi/hi.c|flat"
)

# CONTRACT §A.8 (labs 09-22), plus -fno-builtin and the warnings, plus two measured additions:
#  * -mno-sse -mno-sse2 -mno-mmx -msoft-float, §A.8's named fallback: zig cc 0.17.0 turns
#    -mgeneral-regs-only into -sse/-mmx/-x87 and then appends its own baseline
#    +sse/+sse2/+mmx/+x87, so the flag alone changes no byte (hi kept 31 SSE lines, the
#    same sha256 as without it); the -mno-* flags reach zig's own CPU model and leave 0.
#    The ineffective flag stays so the line still reads as the contract's.
#  * -g0: zig cc emits DWARF by default, and DWARF carries the build directory's
#    absolute path, which would make the bytes depend on where the tree was checked out.
CFLAGS=(
  -target x86_64-linux-musl -static -O2 -nostdlib -ffreestanding
  -mgeneral-regs-only -fno-stack-protector -fno-builtin -Wall -Wextra
  -mno-sse -mno-sse2 -mno-mmx -msoft-float
  -g0
)
LDFLAGS=(-T "$HERE/user.ld")

have=$("$ZIG" version 2>/dev/null || true)
if [[ "$have" != "$PIN" ]]; then
  echo "build.sh: zig $PIN is pinned (user/TOOLCHAIN.md); '$ZIG version' says '${have:-nothing}'" >&2
  exit 1
fi
command -v python3 >/dev/null || { echo "build.sh: python3 is needed for the ELF checks" >&2; exit 1; }
command -v "$OBJDUMP" >/dev/null || { echo "build.sh: $OBJDUMP is needed for the SSE check (set OBJDUMP)" >&2; exit 1; }

# The ELF checks every course binary passes (CONTRACT §A.5, §A.8).
elfcheck() { # file layout
  python3 -I - "$1" "$2" <<'PY'
import struct, sys
path, layout = sys.argv[1], sys.argv[2]
d = open(path, "rb").read()
name = path.rsplit("/", 1)[-1]
def die(msg):
    print(f"build.sh: {name}: {msg}", file=sys.stderr); sys.exit(1)
if d[:4] != b"\x7fELF" or d[4] != 2 or d[5] != 1: die("not a 64-bit little-endian ELF")
e_type, e_machine = struct.unpack_from("<HH", d, 16)
e_entry, e_phoff, e_shoff = struct.unpack_from("<QQQ", d, 24)
e_phentsize, e_phnum, e_shentsize, e_shnum = struct.unpack_from("<HHHH", d, 54)
if e_machine != 62: die(f"e_machine {e_machine}, not x86-64 (62)")
if e_type != 2: die(f"e_type {e_type}, not ET_EXEC (2)")
NAMES = {1: "LOAD", 2: "DYNAMIC", 3: "INTERP", 4: "NOTE", 6: "PHDR", 7: "TLS",
         0x6474e550: "GNU_EH_FRAME", 0x6474e551: "GNU_STACK", 0x6474e552: "GNU_RELRO"}
phs = []
for i in range(e_phnum):
    t, fl, off, va, pa, fsz, msz, al = struct.unpack_from("<IIQQQQQQ", d, e_phoff + i * e_phentsize)
    phs.append((NAMES.get(t, hex(t)), fl, off, va, fsz, msz))
for bad in ("DYNAMIC", "INTERP", "TLS"):
    if any(p[0] == bad for p in phs): die(f"has PT_{bad}")
loads = [p for p in phs if p[0] == "LOAD"]
if not loads or loads[0][3] != 0x1000000 or loads[0][2] != 0:
    die("first PT_LOAD is not file offset 0 at vaddr 0x1000000")
if not (loads[0][3] <= e_entry < loads[0][3] + loads[0][4]) or not (loads[0][1] & 1):
    die("e_entry is not inside the executable first PT_LOAD")
if layout == "flat" and (len(loads) != 1 or loads[0][4] != loads[0][5]):
    die("layout 'flat' needs exactly one PT_LOAD with filesz == memsz (writable data present?)")
# undefined symbols: a freestanding link must have none
for i in range(e_shnum):
    sname, stype, sflags, saddr, soff, ssize, slink, sinfo, salign, sent = \
        struct.unpack_from("<IIQQQQIIQQ", d, e_shoff + i * e_shentsize)
    if stype == 2:  # SHT_SYMTAB
        for j in range(1, ssize // 24):
            st_name, st_info, st_other, st_shndx = struct.unpack_from("<IBBH", d, soff + j * 24)
            if st_shndx == 0: die("has an undefined symbol")
flags = lambda f: ("R" if f & 4 else "-") + ("W" if f & 2 else "-") + ("E" if f & 1 else "-")
print(f"  {name}: ET_EXEC entry={e_entry:#x} size={len(d)} " +
      " ".join(f"{p[0]}[{flags(p[1])} off={p[2]:#x} va={p[3]:#x} filesz={p[4]:#x} memsz={p[5]:#x}]" for p in phs))
PY
}

# bin/ holds exactly what PROGRAMS builds: a program dropped from the list leaves no stale binary.
rm -rf "$BIN"
mkdir -p "$BIN"
for row in "${PROGRAMS[@]}"; do
  IFS='|' read -r name srcs layout <<<"$row"
  src=("$HERE/crt0.c")
  for s in $srcs; do src+=("$HERE/$s"); done
  "$ZIG" cc "${CFLAGS[@]}" "${LDFLAGS[@]}" -o "$BIN/$name" "${src[@]}"
  elfcheck "$BIN/$name" "$layout"
  # No SSE in a pre-23 program (CONTRACT §A.5, §A.8): the disassembly names no vector register or op.
  n=$("$OBJDUMP" -d "$BIN/$name" | grep -c -E 'xmm|ymm|movaps|movups|movdqa|movdqu|fxsave|cvt' || true)
  if [[ "$n" != 0 ]]; then
    echo "build.sh: $name: $n SSE/x87-state instruction lines in the disassembly; must be 0" >&2
    exit 1
  fi
done

# SHA256SUMS lists every file in bin/ except itself (and its .tmp), sorted, in sha256sum's format.
if command -v sha256sum >/dev/null; then sum=(sha256sum); else sum=(shasum -a 256); fi
( cd "$BIN" && find . -maxdepth 1 -type f ! -name 'SHA256SUMS*' | sed 's|^\./||' | LC_ALL=C sort \
    | while read -r f; do "${sum[@]}" "$f"; done ) > "$BIN/SHA256SUMS.tmp"
mv "$BIN/SHA256SUMS.tmp" "$BIN/SHA256SUMS"
echo "build.sh: zig $have; wrote $(wc -l < "$BIN/SHA256SUMS" | tr -d ' ') program(s) to bin/"
cat "$BIN/SHA256SUMS"
