# MAI vt100 patch

Source: crates.io vt100 0.16.2, MIT license, https://github.com/doy/vt100-rust.
The source and license are vendored from Cargo's installed registry package.

Only `src/grid.rs::scroll_up` differs from upstream: retain removed rows when
`scroll_top == 0`, even if the bottom margin is above the last screen row.
Codex 0.153.4 inline output uses `CSI 1;Nr` to scroll its conversation while
keeping the composer below it. Upstream discards these lines. Interior regions
and alternate-screen grids (history capacity zero) remain unchanged.

Regression coverage is in MAI `src/pty.rs::terminal_regressions` and main mouse
routing tests. When upgrading vt100, preserve this patch or verify the upstream
release includes equivalent behavior before removing the path dependency.
