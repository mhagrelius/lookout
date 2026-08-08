# Lookout

A Synology DiskStation monitor for the GNOME desktop, in Rust with GTK 4 and
libadwaita.

Shows what the box is doing — system health, resource use with recorded
trends, storage pools and per-drive S.M.A.R.T., containers, shared folders and
the system log — without opening DSM in a browser.

## Building

Needs GTK 4.22, libadwaita 1.9 and Rust 1.80 or newer.

```sh
./test.sh              # fmt, clippy -D warnings, tests
./test.sh --headless   # the same under Xvfb, for the widget tests
cargo run              # run it
./install.sh           # release build into ~/.local
```

## Connecting

The app asks for an address, an account and a password. On a DiskStation with
two-factor enabled it then asks for a code — **once**. That login also claims a
device token, which is stored and used in place of the code from then on.

The password is never written to disk and the session id never leaves memory.
What is persisted, in `~/.config/lookout/config.json`, is the address, the
account name and that device token.

Certificate verification is on by default. A DiskStation using the self-signed
certificate it ships with needs "Verify certificate" turned off in the connect
dialog; a host reachable over Tailscale at a `ts.net` name has a real
certificate and should leave it on.

## Layout

```
core/    lookout-core   DSM client, records, recorded history. No GTK.
src/     lookout        widgets, application, charts.
docs/    dsm-api-notes.md — the API as measured, not as documented.
```

`core/` links no UI toolkit. That is deliberate: it is the half a Windows or
macOS shell would keep, and the rule is that anything a second frontend would
also need goes there rather than in `src/`.

## Trends are recorded, not fetched

DSM keeps no performance history that can be read back —
`SYNO.ResourceMonitor.Setting` reports `enable_history: false`,
`SYNO.ResourceMonitor.Log` is empty, and `SYNO.Core.System.Utilization` offers
only a point-in-time `get`. So Lookout records its own samples into four tiers
(5 s, 60 s, 5 min, 30 min) and draws them over the last hour, day, week or
month.

The consequence is worth knowing up front: **history covers only the time the
app was running.** A fresh install starts with empty charts, and a laptop
that was closed overnight has a gap. `DESIGN.md` covers the alternative — a
collector running on the NAS — and why it is a later milestone rather than
part of this.

## Looking at it

```sh
cargo run --example preview -- /tmp/preview          # light
cargo run --example preview -- /tmp/preview dark     # dark
```

Renders the real widget tree to PNG, seeded from replies captured off an
actual DS-series, so a layout change can be checked without a screenshot
prompt.

```sh
cargo run -p lookout-core --example discover -- HOST:PORT
```

Asks a DiskStation what it exposes. Needs no credentials — `SYNO.API.Info`
answers without a session — which makes it the quickest way to tell a wrong
address from a wrong password.

## A warning about the published API references

Several widely-cited references, and the design handoff this was built from,
are wrong in ways that render plausible but incorrect numbers:
`SYNO.Storage.CS.*` does not exist, `up_time` is hours rather than days, and
system temperature is `sys_temp` rather than `temperature`.

`docs/dsm-api-notes.md` records what a real box actually returns, including
the units that had to be derived by arithmetic rather than read anywhere. It
is worth reading before extending this.
