# Lookout — design

A GTK 4 / libadwaita monitor for a Synology DiskStation, in Rust. One host,
read-mostly, desktop-only. Built the way `planner` and `brain` are built: a
GTK-free crate that `cargo test` exercises with no display, an imperative
widget half of `glib::wrapper!` subclasses, no blueprint, no `.ui` XML, no
meson, no async runtime.

The UI follows the design handoff in
`~/Downloads/Synology NAS information viewer.zip` — an Overview page and
eleven drill-in detail pages, mapped 1:1 onto stock libadwaita widgets.

## The split, which is the point

```
core/     lookout-core   DSM client, records, recorded history. No GTK.
src/      lookout        widgets, application, charts. GTK 4 + libadwaita.
```

`core/` is the backend. It knows how to reach a DiskStation, how to survive
DSM's inconsistencies, what a volume and a container and a log entry are, and
what the box was doing an hour ago. It links no UI toolkit and holds no GTK
type, which means **a Windows or macOS shell keeps it whole and replaces
`src/` entirely**. That is the requirement this layout exists to satisfy, and
the rule that enforces it is simple: anything a second frontend would also
need belongs in `core/`.

There is deliberately **no `server/` crate**, which is where this departs from
`planner` and `brain`. Those need a server because several machines edit one
document and something has to arbitrate. Nothing here is shared and nothing is
written — each machine talks to the DiskStation directly and reads. A server
would be a second thing to deploy in exchange for nothing.

The one honest cost is under **Recorded history** below, and the milestone
that would pay it is listed rather than built.

## Recorded history, and why the app keeps it

The design asks for 1 h / 24 h / 7 d / 30 d trends.
**DSM cannot supply them.** Measured on the target box:

- `SYNO.ResourceMonitor.Setting` reports `enable_history: false`
- `SYNO.ResourceMonitor.Log` returns `total: 0`
- `SYNO.Core.System.Utilization` has only `get` — `list`, `get_history` and
  `past` all answer 103

So `SYNO.Core.System.Utilization` is a snapshot and there is no series behind
it. The app records its own, in `core/src/trend.rs`.

Storing every poll for 30 days would be half a million samples at a
five-second interval. Instead a sample is offered to four tiers — 5 s, 60 s,
5 min, 30 min — and each accepts one only when its own interval has elapsed.
That bounds the store at about 5,600 points, which is more than any chart has
pixels, and small enough to write out as JSON on a tick.

**What this costs, plainly: history only covers time the app was running.** A
laptop that is closed overnight has a gap in its 7-day chart, and a fresh
install has no history at all — the 30-day view starts empty and fills in over
a month. Every alternative was worse:

- Enabling DSM's own history does not help: nothing exposes it for reading.
- A collector container on the NAS would fix it, and is the deferred milestone
  below. It is real work — an image, a Project, a schema, a read API — to
  improve four charts, and it should follow a working app rather than block
  one.

## Data flow

Polling is a GLib timer on the main thread, default 5 s, configurable 1–60 s.
Each tick builds the set of calls the **visible page** needs, sends them as one
`SYNO.Entry.Request` compound request, and applies the results.

Compound batching earns its place: per-call failures are isolated, so a
DiskStation without Container Manager fails exactly the containers card and
nothing else. That is the handoff's "one failing endpoint greys out its card,
it does not blank the page", and it falls out of the transport rather than
needing per-card error plumbing.

Backoff on failure, and a longer interval when the window is unfocused — a
monitor nobody is looking at should not keep a NAS's disks awake.

**Nothing async.** The HTTP client is blocking; a poll runs on a worker thread
and hands its result back with `glib::spawn_future_local`, which is the house
pattern and needs no runtime.

## Capability gating

`SYNO.API.Info` answers without a session and lists all 728 APIs the box
exposes. It runs once at connect, and a section whose namespace is absent is
**not built at all** rather than built and shown broken. No Container Manager
means no Containers card and no drill-in.

It doubles as the reachability check: if it answers, the address and port are
right and the thing on the end is a DiskStation, which is a much better
first-run error than a failed login.

## Authentication

`SYNO.API.Auth` v7. First login sends `otp_code` and `enable_device_token=yes`
and keeps the returned `device_id`; later logins send that instead of a code.
So a second factor is typed **once, at setup** — which matters given the
account this will run against uses a hardware token.

The `sid` and the device token go in the platform secret store, never in
GSettings. Host, port, TLS choice, poll interval, range and colour scheme are
GSettings.

TLS verification is on by default with an explicit opt-out. A DiskStation out
of the box serves a self-signed certificate and refusing it outright would
lock out most installations; the realistic alternative to an opt-out is people
using plain HTTP instead.

## Testing

Same layers as the siblings. Unit tests inline per `core` module, which is
where the bulk of them are and should be — the parsers are pure functions over
replies captured verbatim from a real box, and that is where the bugs live.
`tests/widgets.rs` with a hand-rolled case runner for the widget half (GTK is
thread-affine). `./test.sh` runs fmt, clippy `-D warnings` and tests, with
`--headless` under Xvfb.

An `examples/preview.rs` renders the cards and charts to PNG, so "does this
look right?" is answerable without a Wayland screenshot prompt.

The DSM reader tests are written against **captured real replies**, not
invented ones, and several of them exist specifically to hold a measured fact
shut: that `up_time` is hours rather than days, that load averages are
hundredths, that share sizes are MiB. Each of those was wrong in a published
reference and would render a plausible, wrong number.

## Milestones

1. Core: transport, auth, capability discovery, compound requests, the domain
   readers, the trend store. No UI. All tested.
2. Shell: window, navigation view, header bar, preferences, connect flow.
3. Overview: banner, stat tiles, storage, drives, containers, logs.
4. Charts: Cairo drawing area, four series, the range switcher.
5. Detail pages: storage and drives, resource monitor, containers, logs.
6. The remaining seven table pages.
7. Light actions: container start/stop/restart, backup run, S.M.A.R.T. test.
8. Packaging: deb, Flatpak, icons, metainfo.

## Deliberately not in v1

- **A collector on the NAS.** See **Recorded history**.
- **More than one host.** The design is single-host and the window has no
  affordance for a second one.
- **Photos.** 46 `SYNO.Foto` APIs and a genuinely useful surface — indexed
  counts, EXIF, faces, per-device backup folders — but it is a second app's
  worth of UI, and a monitor is not a photo browser. The measurements are in
  `docs/dsm-api-notes.md` so it does not have to be rediscovered.
- **Writing anything DSM would call configuration.** The light actions are a
  fixed, short list. This is not a Control Panel.
