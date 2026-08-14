# Vendored Limine (pinned)

- Binaries: `limine-bootloader/limine` branch `v10.x-binary`, commit
  `7a9013f305de0bea1f6310e76e7baba30499fef0`
  (limine-bios.sys, limine-bios-cd.bin, limine-uefi-cd.bin, limine.c, LICENSE).
- Protocol constants in `os/boot/src/main.rs` were transcribed from
  `limine-bootloader/limine-protocol` commit
  `4e1587972c148d43b2f397e4e5983bdd6c2a55a0` (`include/limine.h`).

Vendored, not downloaded at build time, so the first build is hermetic and
offline (docs/course-v2-design.md, repo-layout resolution). Bump deliberately,
with the spec version, updating both this file and the transcribed constants
together.
