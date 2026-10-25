#!/usr/bin/env bash
# user/build.sh -- builds every course program into bin/ with the pinned
# toolchain (CONTRACT §A.8; the pin is TOOLCHAIN.md). A maintainer runs this
# by hand (no CI workflow runs it yet); a learner never does: the binaries ship
# built, beside their sources.
#
#   ./build.sh            build everything, check every binary, write bin/boot.archive
#                         (CONTRACT §A.7), read it back, write bin/SHA256SUMS
#   ZIG=/path/to/zig ./build.sh     use a zig that is not first on PATH
#   OBJDUMP=llvm-objdump ./build.sh use another disassembler for the SSE check
#   BUSYBOX_TARBALL=/path/busybox-1.37.0.tar.bz2 ./build.sh
#                         take busybox's pinned source from there, not from the
#                         workspace's target/assets/ (it is not in the workspace; checked by sha256;
#                         where to get it: busybox/README.md)
#
# Adding a program: one directory <name>/ with its .c file(s), one line in
# PROGRAMS below. Nothing else in this file changes.
set -euo pipefail

PIN=0.17.0
ZIG=${ZIG:-zig}
OBJDUMP=${OBJDUMP:-objdump}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BIN=$HERE/bin

# name | sources (relative to user/; crt0.c is linked into every layout but "musl") | layout
#   layout "flat": exactly one PT_LOAD, file offset 0 -> 0x1000000, filesz == memsz,
#   so labs 09-12's vehicle may copy the file's bytes to 0x1000000 and enter at
#   e_entry. "elf": loaded by lab 13's ELF loader; may carry a RW segment.
#   "flat-sse": "flat", and the SSE check allows exactly the movq lines between a
#   general register or memory and xmm0/xmm1 (hostile-spin's inline asm, on purpose:
#   lab 12's regs.survive.preempt row is the FXSAVE check -- CONTRACT §A.8 as amended
#   2026-10-17). Any other SSE line in such a program still fails the build.
#   "musl" (lab 25, CONTRACT §A.8 as built 2026-10-22): a static program built against
#   zig's own musl with no crt0 and no linker script -- the toolchain's default static
#   layout (first PT_LOAD r-- at offset 0 -> 0x1000000, then r-x, then rw-), at most
#   one PT_TLS, any SSE (musl's own memcpy and printf use it), never AVX: no ymm/zmm
#   and no VEX-encoded xmm instruction. Stripped (-s): zig's cached libc objects carry
#   DWARF with the absolute path of the machine's zig install, and -g0 does not reach them.
#   "port" (lab 27, CONTRACT §A.8 as built 2026-10-22): an upstream program built
#   UNMODIFIED from its own makefile's file list. The sources field names that makefile;
#   its `SRC_DOOM = …` line is the list of objects, each built from the .c beside it, all
#   in one zig cc call with PORT_CFLAGS. Then every "musl" check. doomgeneric's own
#   sources, licence and makefile sit under doomgeneric/ byte for byte as the pinned
#   upstream commit has them (user/README.md, "doomgeneric").
#   "busybox" (lab 30, CONTRACT §A.8 as built 2026-10-23): upstream busybox, UNMODIFIED, from
#   its pinned release tarball (BUSYBOX_* below; not in the repo), configured by the committed
#   .config the sources field names, built by busybox's own makefiles with zig cc as CC. Then
#   every "musl" check, and its applet list must equal busybox/applets, which the image writer
#   reads to make /bin's hard links from lab 30 (xtask/src/ext2.rs `busybox_bin`).
PROGRAMS=(
  "hi|hi/hi.c|flat"
  "hostile-kwrite|hostile-kwrite/hostile-kwrite.c|flat"
  "hostile-badnum|hostile-badnum/hostile-badnum.c|flat"
  "hostile-badptr|hostile-badptr/hostile-badptr.c|flat"
  "hostile-spin|hostile-spin/hostile-spin.c|flat-sse"
  "args|args/args.c|elf"
  "init|init/init.c|elf"
  "forker|forker/forker.c|elf"
  "producer|producer/producer.c|elf"
  "consumer|consumer/consumer.c|elf"
  # labs 17-23 (the W3 programs): not in ARCHIVE -- the image writer copies bin/ into ext2's /bin
  "ls|ls/ls.c|elf"
  "cat|cat/cat.c|elf"
  "stat|stat/stat.c|elf"
  "mkfile|mkfile/mkfile.c|elf"
  "rm|rm/rm.c|elf"
  "mv|mv/mv.c|elf"
  "mkdir|mkdir/mkdir.c|elf"
  "rmdir|rmdir/rmdir.c|elf"
  "mounts|mounts/mounts.c|elf"
  "sh|sh/sh.c|elf"
  # lab 24 on (the W4 programs): not in ARCHIVE; ext2's /bin carries them from lab 24's image on
  # (xtask/src/ext2.rs W4_PROGRAMS)
  "cowtest|cowtest/cowtest.c|elf"
  # lab 25 (the W4 musl programs, layout musl): the same rule
  "hello|hello/hello.c|musl"
  "tls|tls/tls.c|musl"
  "mem|mem/mem.c|musl"
  # lab 26 (the devices: /dev/fb0 and /dev/input/event0), layout musl
  "fbtest|fbtest/fbtest.c|musl"
  "evtest|evtest/evtest.c|musl"
  # lab 27 (the port): upstream doomgeneric, the linuxvt backend, layout port
  "doomgeneric|doomgeneric/doomgeneric/Makefile.linuxvt|port"
  # lab 30: busybox 1.37.0, ash and the applets labs 30-32 run (busybox/README.md), layout busybox
  "busybox|busybox/busybox.config|busybox"
  # lab 29 (the learner's shell's script): not in ARCHIVE; ext2's /bin carries it from lab 29's
  # image on (xtask/src/ext2.rs SHELL_PROGRAMS)
  "mark|mark/mark.c|elf"
  # lab 32 (handlers: the registers a handler may not disturb, SA_RESTART, rt_sigsuspend),
  # layout musl; not in ARCHIVE, and kept out of ext2's /bin before 32 (HANDLER_PROGRAMS)
  "sigregs|sigregs/sigregs.c|musl"
  # lab 33 (the terminal: termios, raw input, access, ftruncate), layout musl; not in ARCHIVE,
  # and kept out of ext2's /bin before 33 (TERMIOS_PROGRAMS)
  "ttyprobe|ttyprobe/ttyprobe.c|musl"
)

# The boot archive (CONTRACT §A.7): what labs 13-16 mount at / and 17 on at /boot.
# The W3 programs (ls ... sh, labs 17-23) are not in it: they ride ext2's /bin (2026-10-17).
ARCHIVE=(init hi args hostile-kwrite hostile-badnum hostile-badptr hostile-spin forker producer consumer)

# CONTRACT §A.8 (labs 09-24), plus -fno-builtin and the warnings, plus two measured additions:
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
# The "musl" layout's flags: libc and its start-up code are the toolchain's, SSE is allowed.
MUSL_CFLAGS=(-target x86_64-linux-musl -static -O2 -Wall -Wextra -g0 -s)
# The "port" layout's flags: CONTRACT Appendix (1)'s build line (`make -f Makefile.linuxvt
# CC="zig cc -target x86_64-linux-musl" CFLAGS="-static -O2 -DNORMALUNIX -DLINUX -DSNDSERV
# -D_DEFAULT_SOURCE" LDFLAGS="-static"`), plus -g0 -s as for "musl": without them the bytes
# carry DWARF with the build directory's absolute path (measured 2026-10-22: the Appendix
# line's binary differs by size and sha256 from one directory to another), and
# -ffile-prefix-map below for the same reason (`__FILE__`).
PORT_CFLAGS=(-target x86_64-linux-musl -static -O2 -DNORMALUNIX -DLINUX -DSNDSERV -D_DEFAULT_SOURCE -g0 -s)
# The "busybox" layout's source: busybox.net's release tarball, pinned by sha256 (busybox/README.md).
BUSYBOX_VERSION=1.37.0
BUSYBOX_SHA256=3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4
BUSYBOX_URL=https://busybox.net/downloads/busybox-$BUSYBOX_VERSION.tar.bz2
BUSYBOX_TARBALL=${BUSYBOX_TARBALL:-$HERE/../../target/assets/busybox-$BUSYBOX_VERSION.tar.bz2}

have=$("$ZIG" version 2>/dev/null || true)
if [[ "$have" != "$PIN" ]]; then
  echo "build.sh: zig $PIN is pinned (user/TOOLCHAIN.md); '$ZIG version' says '${have:-nothing}'" >&2
  exit 1
fi
command -v python3 >/dev/null || { echo "build.sh: python3 is needed for the ELF checks" >&2; exit 1; }
command -v "$OBJDUMP" >/dev/null || { echo "build.sh: $OBJDUMP is needed for the SSE check (set OBJDUMP)" >&2; exit 1; }
if command -v sha256sum >/dev/null; then sum=(sha256sum); else sum=(shasum -a 256); fi

# busybox's source, checked BEFORE bin/ is touched: a missing or different tarball is one
# named refusal and bin/ stays as it was.
if [[ " ${PROGRAMS[*]} " == *"|busybox "* ]]; then
  if [[ ! -f "$BUSYBOX_TARBALL" ]]; then
    echo "build.sh: busybox: no source tarball at $BUSYBOX_TARBALL; download $BUSYBOX_URL there" \
         "(sha256 $BUSYBOX_SHA256), or name it with BUSYBOX_TARBALL=" >&2
    exit 1
  fi
  got=$("${sum[@]}" "$BUSYBOX_TARBALL" | cut -d' ' -f1)
  if [[ "$got" != "$BUSYBOX_SHA256" ]]; then
    echo "build.sh: busybox: $BUSYBOX_TARBALL has sha256 $got, not busybox $BUSYBOX_VERSION's $BUSYBOX_SHA256" >&2
    exit 1
  fi
  for t in make tar bzip2 cc; do
    command -v "$t" >/dev/null || { echo "build.sh: busybox: '$t' is needed to build busybox" >&2; exit 1; }
  done
fi

# The "busybox" layout: unpack the pinned tarball and build out of tree, both in a temporary
# directory, with the committed .config. CC is a wrapper around zig cc that drops the three
# linker flags busybox's trylink adds for its own report (--warn-common, -Map, --verbose), which
# zig's linker refuses; nothing else changes. KCONFIG_NOTIMESTAMP: no build date in the banner
# ("BusyBox v1.37.0 ()"). SKIP_STRIP: zig's link at -Oz already leaves no symbols or debug info.
build_busybox() { # name config-path
  local tmp src out cfg="$HERE/$2" jobs
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/sboot-busybox.XXXXXX")
  BUSYBOX_TMP=$tmp
  trap 'rm -rf "$BUSYBOX_TMP"' EXIT
  tar -xjf "$BUSYBOX_TARBALL" -C "$tmp"
  src="$tmp/busybox-$BUSYBOX_VERSION"; out="$tmp/out"; mkdir -p "$out" "$tmp/cc"
  [[ -f "$src/Makefile" ]] || { echo "build.sh: $1: the tarball has no busybox-$BUSYBOX_VERSION/Makefile" >&2; exit 1; }
  cat > "$tmp/cc/zcc" <<EOF
#!/bin/sh
n=\$#
for a in "\$@"; do
  case "\$a" in -Wl,--warn-common|-Wl,--verbose|-Wl,-Map,*) ;; *) set -- "\$@" "\$a" ;; esac
done
shift \$n
exec "$ZIG" cc -target x86_64-linux-musl "\$@"
EOF
  chmod +x "$tmp/cc/zcc"
  cp "$cfg" "$out/.config"
  jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
  local bb_make=(make -C "$src" O="$out" HOSTCC=cc CC="$tmp/cc/zcc" AR="$ZIG ar" SKIP_STRIP=y KCONFIG_NOTIMESTAMP=1)
  # stdin is /dev/null: a config the release does not accept whole stops Kconfig, never waits.
  if ! "${bb_make[@]}" oldconfig </dev/null >"$tmp/config.log" 2>&1 || ! cmp -s "$cfg" "$out/.config"; then
    echo "build.sh: $1: busybox $BUSYBOX_VERSION's Kconfig does not take $2 unchanged (diff below)" >&2
    diff "$cfg" "$out/.config" >&2 || true
    exit 1
  fi
  if ! "${bb_make[@]}" -j"$jobs" busybox busybox.links </dev/null >"$tmp/build.log" 2>&1; then
    tail -30 "$tmp/build.log" >&2
    echo "build.sh: $1: busybox's build failed (the log's tail above)" >&2
    exit 1
  fi
  local links want
  links=$(sed 's|.*/||' "$out/busybox.links" | LC_ALL=C sort)
  want=$(grep -v -e '^#' -e '^$' "$HERE/busybox/applets")
  if [[ "$links" != "$want" ]]; then
    echo "build.sh: $1: the build's applets differ from busybox/applets:" >&2
    diff <(echo "$want") <(echo "$links") >&2 || true
    exit 1
  fi
  cp "$out/busybox" "$BIN/$1"
  rm -rf "$tmp"
}

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
for bad in ("DYNAMIC", "INTERP") + (() if layout == "musl" else ("TLS",)):
    if any(p[0] == bad for p in phs): die(f"has PT_{bad}")
if sum(p[0] == "TLS" for p in phs) > 1: die("has more than one PT_TLS")
loads = [p for p in phs if p[0] == "LOAD"]
if not loads or loads[0][3] != 0x1000000 or loads[0][2] != 0:
    die("first PT_LOAD is not file offset 0 at vaddr 0x1000000")
entry_in = [p for p in loads if p[3] <= e_entry < p[3] + p[4] and p[1] & 1]
if layout == "musl":
    if not entry_in: die("e_entry is not inside an executable PT_LOAD")
elif not entry_in or entry_in[0] is not loads[0]:
    die("e_entry is not inside the executable first PT_LOAD")
if layout in ("flat", "flat-sse") and (len(loads) != 1 or loads[0][4] != loads[0][5]):
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
  if [[ "$layout" == musl || "$layout" == port || "$layout" == busybox ]]; then
    src=()
    if [[ "$layout" == busybox ]]; then
      build_busybox "$name" "$srcs"
    elif [[ "$layout" == port ]]; then
      mk="$HERE/$srcs"
      objs=$(sed -n 's/^SRC_DOOM *= *//p' "$mk")
      [[ -n "$objs" ]] || { echo "build.sh: $name: no SRC_DOOM line in $srcs" >&2; exit 1; }
      for o in $objs; do src+=("$(dirname "$mk")/${o%.o}.c"); done
      # -ffile-prefix-map: `__FILE__` (z_zone.h's Z_ChangeTag) is then "w_wad.c", not the
      # checkout's absolute path (measured 2026-10-22: two directories, two sha256s, until this;
      # zig cc makes a relative source name absolute, so compiling from the directory is not enough).
      "$ZIG" cc "${PORT_CFLAGS[@]}" -ffile-prefix-map="$(dirname "$mk")/=" -o "$BIN/$name" "${src[@]}"
    else
      for s in $srcs; do src+=("$HERE/$s"); done
      "$ZIG" cc "${MUSL_CFLAGS[@]}" -o "$BIN/$name" "${src[@]}"
    fi
    elfcheck "$BIN/$name" musl
    avx=$("$OBJDUMP" -d "$BIN/$name" | grep -c -E '%ymm|%zmm|[[:space:]]v[a-z0-9]+[[:space:]]+[^#]*%xmm' || true)
    if [[ "$avx" != 0 ]]; then
      echo "build.sh: $name: $avx AVX instruction lines (ymm/zmm or VEX-encoded xmm); must be 0" >&2
      exit 1
    fi
    echo "  $name: 0 AVX lines (layout $layout: SSE allowed)"
    continue
  fi
  src=("$HERE/crt0.c")
  for s in $srcs; do src+=("$HERE/$s"); done
  "$ZIG" cc "${CFLAGS[@]}" "${LDFLAGS[@]}" -o "$BIN/$name" "${src[@]}"
  elfcheck "$BIN/$name" "$layout"
  # No SSE in a pre-25 program (CONTRACT §A.5, §A.8): the disassembly names no vector register or op.
  sse=$("$OBJDUMP" -d "$BIN/$name" | grep -E 'xmm|ymm|movaps|movups|movdqa|movdqu|fxsave|cvt' || true)
  n=$(printf '%s' "$sse" | grep -c . || true)
  if [[ "$layout" == flat-sse ]]; then
    other=$(printf '%s' "$sse" | grep -v -E 'movq[[:space:]]+([^,]+,[[:space:]]*%xmm[01]|%xmm[01],[[:space:]]*%r[a-z0-9]+)[[:space:]]*$' | grep -c . || true)
    if [[ "$other" != 0 ]]; then
      echo "build.sh: $name: $other SSE lines besides the allowed movq to/from xmm0/xmm1; must be 0" >&2
      exit 1
    fi
    echo "  $name: $n allowed SSE lines (layout flat-sse: movq to/from xmm0/xmm1 only)"
  elif [[ "$n" != 0 ]]; then
    echo "build.sh: $name: $n SSE/x87-state instruction lines in the disassembly; must be 0" >&2
    exit 1
  fi
done

# bin/boot.archive (CONTRACT §A.7): magic 0x5B007B00A2C4 (u64 LE), count (u32 LE), then per
# entry {name: 56 bytes NUL-padded, offset: u64 LE, size: u64 LE}; offsets from the file's
# start; each file's data 8-aligned, zero padding between. Entries sorted by name (bytewise);
# no timestamps, so the bytes follow from the binaries alone.
python3 -I - "$BIN" "${ARCHIVE[@]}" <<'PY'
import struct, sys
bindir, names = sys.argv[1], sorted(sys.argv[2:])
HDR, ENT = 12, 72
out = bytearray(struct.pack("<QI", 0x5B007B00A2C4, len(names)))
off = (HDR + ENT * len(names) + 7) & ~7
data = bytearray()
for n in names:
    enc, b = n.encode(), open(f"{bindir}/{n}", "rb").read()
    if not 0 < len(enc) < 56: sys.exit(f"build.sh: archive name {n!r} must be 1-55 bytes")
    out += enc.ljust(56, b"\0") + struct.pack("<QQ", off, len(b))
    pad = (-len(b)) % 8
    data += b + bytes(pad)
    off += len(b) + pad
out += bytes((-len(out)) % 8) + data
open(f"{bindir}/boot.archive", "wb").write(out)
PY

# Read the archive back, independently of the writer, and match every entry to bin/<name>.
python3 -I - "$BIN" "${ARCHIVE[@]}" <<'PY'
import struct, sys
bindir, want = sys.argv[1], sorted(sys.argv[2:])
d = open(f"{bindir}/boot.archive", "rb").read()
def die(m): print(f"build.sh: boot.archive: {m}", file=sys.stderr); sys.exit(1)
magic, count = struct.unpack_from("<QI", d, 0)
if magic != 0x5B007B00A2C4: die(f"magic {magic:#x}")
if count != len(want): die(f"count {count}, want {len(want)}")
seen = []
for i in range(count):
    raw, off, size = struct.unpack_from("<56sQQ", d, 12 + 72 * i)
    name = raw.rstrip(b"\0").decode()
    if b"\0" in raw.rstrip(b"\0") or off % 8 or off + size > len(d): die(f"entry {i} ({name!r}) malformed")
    if d[off:off + size] != open(f"{bindir}/{name}", "rb").read(): die(f"{name}: bytes differ from bin/{name}")
    seen.append(name)
    print(f"  boot.archive[{i}] {name:<15} offset={off:#07x} size={size}")
if seen != want: die(f"names {seen}, want {want} (sorted)")
print(f"  boot.archive: {len(d)} bytes, {count} entries, each equal to bin/<name>")
PY

# SHA256SUMS lists every file in bin/ except itself (and its .tmp), sorted, in sha256sum's format.
( cd "$BIN" && find . -maxdepth 1 -type f ! -name 'SHA256SUMS*' | sed 's|^\./||' | LC_ALL=C sort \
    | while read -r f; do "${sum[@]}" "$f"; done ) > "$BIN/SHA256SUMS.tmp"
mv "$BIN/SHA256SUMS.tmp" "$BIN/SHA256SUMS"
echo "build.sh: zig $have; wrote $(wc -l < "$BIN/SHA256SUMS" | tr -d " ") file(s) to bin/ (${#PROGRAMS[@]} programs and boot.archive)"
cat "$BIN/SHA256SUMS"
