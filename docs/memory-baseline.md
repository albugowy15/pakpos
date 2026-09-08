# Memory baseline

This is the first measurement of the GTK implementation. It is an engineering
baseline, not the complete release memory report required by `PRODUCT.md`.

## Environment

- Date: 2026-09-07
- Revision: `f220994` with the uncommitted GTK implementation
- Distribution: Arch Linux
- Kernel: Linux 7.2.3-arch1-2, x86_64
- Desktop session: Hyprland on Wayland
- GTK: 4.22.4
- Rust: 1.98.1
- CPU: Intel Core 5 210H, 12 logical CPUs
- Build: `cargo build --release`

## Idle measurement

Each run launched `target/release/pakpos` with an empty editor, waited 10 seconds,
then read `/proc/<pid>/smaps_rollup`. No application-owned child processes were
present. RSS includes resident shared library pages; PSS is included as diagnostic
context and is not substituted for the product's RSS criterion.

| Run | RSS | PSS | Private clean | Private dirty |
| --- | ---: | ---: | ---: | ---: |
| 1 | 176,276 KiB | 80,930 KiB | 18,344 KiB | 37,480 KiB |
| 2 | 176,516 KiB | 80,944 KiB | 18,388 KiB | 37,444 KiB |
| 3 | 176,792 KiB | 80,999 KiB | 18,364 KiB | 37,460 KiB |
| Mean | 176,528 KiB (172.4 MiB) | 80,958 KiB (79.1 MiB) | 18,365 KiB | 37,461 KiB |

The current settled RSS is about 27.6 MiB below the owner-approved 200 MiB idle
target. The narrow spread across runs suggests a stable baseline rather than
continuing idle growth. Before release, profile mapped and allocated memory, measure
peak RSS, and run the everyday-use, transfer, and repeated-request scenarios.
