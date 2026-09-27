# winforge

Single-binary Windows 10/11 tweak and debloat tool. One static Rust exe — PowerShell scripts, app catalogs, wallpaper asset and the activation payload are embedded at compile time. No runtime dependencies beyond Windows itself. Floor: Windows 10 1809.

## What it does

- 12 packs / 143 tweaks / ~500 actions: registry, services, scheduled tasks, AppX removal, file ops, command execution
- Every non-oneway tweak ships stock-value resets — apply and revert are both first-class paths
- Per-tweak state detection: `A` applied, `D` default, `M` mixed, `U` unknown, `X` absent

## TUI

Run the exe (auto-elevates via UAC). Checkbox interface:

- `h`/`l`/`Tab` switch panes, `j`/`k` move, `space` toggle, `a`/`A` select category/all, `x`/`X` clear
- `1`/`2`/`3` apply the min/std/max preset to the selection
- `enter` apply, `u` undo, `c` clear everything, `q` quit
- Apps pane installs selected software through winget; activate pane runs the bundled MAS in its own window
- Bottom pane streams a live log of everything the tool runs

## CLI

For scripted/VM use without the TUI:

```
winforge --list
winforge --preset std --apply --yes
winforge --select privacy:all --select ceip --reset --yes
winforge --dump state.json
```

`--select` takes tweak ids, `pack:all`, or `all`. `--preset min|std|max` expands to its tweak set. `--yes` skips confirmation. The exe relaunches itself elevated, preserving the argument list.

## Presets

| preset | scope |
|---|---|
| `min` | privacy, background noise, update control |
| `std` | min + performance, power plan, explorer QoL |
| `max` | std + aggressive tweaks (defender suspend, smartscreen, pagefile, search indexing) |

Personal/one-shot tweaks (theme, accent, wallpaper, WSL/SSH setup) and irreversible AppX removals are never part of a preset.

## Repository layout

- `packs/` — tweak definitions (TOML), validated at build time by `build.rs`
- `apps/` — winget app catalog (TOML)
- `scripts/` — PowerShell helpers embedded into the exe
- `src/` — the tool: engine, TUI, CLI, winget, activation
- `mas/` — payload, embedded verbatim

## Build

```
cargo build --release
```

Targets `x86_64-pc-windows-gnu` with static CRT (`.cargo/config.toml`); the result is a fully self-contained exe.

## Credits

Activation is the unmodified [MAS](https://github.com/massgravel/Microsoft-Activation-Scripts) script by massgravel, embedded and launched as-is.
