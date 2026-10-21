# Vendored Limine (pinned)

- Binaries: `limine-bootloader/limine` branch `v10.x-binary`, commit
  `7a9013f305de0bea1f6310e76e7baba30499fef0`
  (BOOTX64.EFI, limine-bios.sys, limine-bios-hdd.h, limine.c, LICENSE).
- 2026-10-17: `limine-bios-cd.bin` and `limine-uefi-cd.bin` removed — the ISO
  path was retired by D7 (the raw image, Appendix (9)); the vendored set is
  what `xtask image` reads.
- Protocol constants in `os/boot/src/main.rs` were transcribed from
  `limine-bootloader/limine-protocol` commit
  `4e1587972c148d43b2f397e4e5983bdd6c2a55a0` (`include/limine.h`).

Vendored, not downloaded at build time, so the first build is hermetic and
offline. Bump deliberately, updating both this file and the transcribed
constants together.
