# Plan

Where this is, and what is left. `DESIGN.md` carries the reasoning; this is
the state of the work.

## Done

**1. Core.** `lookout-core` links no UI toolkit and is where the work is.

- Transport over `entry.cgi`: form encoding, `_sid`, `X-SYNO-TOKEN`, TLS with
  an explicit opt-out for a DiskStation's self-signed certificate.
- The envelope seam. DSM reports failure in a `200` body, so `envelope.rs`
  turns `{"success": false, "error": {"code": N}}` into a typed `Result` in
  one place, and nothing above it inspects a status code.
- `SYNO.API.Auth` v7 with OTP and device-token persistence.
- `SYNO.API.Info` capability discovery, used both to gate sections and as a
  no-credentials reachability probe.
- `SYNO.Entry.Request` compound batching, with per-call failures isolated.
- Readers for system, utilization, storage/S.M.A.R.T., containers and
  projects, shares and the system log — written against replies captured
  verbatim from a DS-series. `SYNO.Docker.Project` is gated on its own
  capability rather than on the container API: a box can run containers with
  no compose project on it at all.
- The container reader takes health, exit code, OOM kill and start time from
  the capitalised `State` object rather than the flat fields beside it —
  measured, because `up_time` is `null` on every compose-managed container and
  reading only that left the uptime blank for a whole real NAS.
- The trend store: four tiers, bounded at ~5,600 samples, atomic writes.
- The poll plan and `Snapshot`, kept in core because a second frontend needs
  the same calls and the same result shape.
- Config that holds no secret.

**2–4. Shell, Overview, charts.**

- `AdwApplicationWindow` + `AdwNavigationView`, header bar with refresh and
  the "polled N s ago" indicator, `AdwStatusPage` for disconnected, toasts.
- Connect dialog that asks for a two-factor code only when DSM says so.
- Polling on a GLib timer, on a worker via `gio::spawn_blocking`, one in
  flight at a time, three failures before declaring disconnection, session
  expiry sending the user back to the dialog.
- The Overview: host banner, four stat tiles, three trend cards, storage,
  containers, shares, recent log. A container's pill carries its health check
  rather than its state word where the two disagree — a container failing its
  probe still reports `running`, and this screen answers "is everything OK?".
- Storage is grouped the way DSM models it — one card per pool, holding the
  volumes on it and the drives it is made of, with anything unallocated shown
  separately. A volume carries `pool_path` and a pool carries its `disks`, so
  the three flat lists this started as were throwing away a hierarchy that was
  already in the reply.
- Cairo charts with a fixed per-type Y axis, peak-preserving downsampling and
  colours from the style manager.
- `examples/preview.rs` renders it to PNG in both schemes.

**5. Detail pages — ten of the handoff's eleven.**

- **Pools & drives**: stats strip, then the same pool grouping as the Overview
  — but with a `GtkColumnView` of that pool's drives in place of the bay tiles,
  since model, serial and capacity are what the drill-in is for. Cards are
  rebuilt only when the set of pools changes, so a poll every five seconds
  costs neither the column widths the user dragged nor their scroll position.
- **Resource monitor**: the four full-width charts, the `AdwToggleGroup` range
  switcher, five time ticks per plot ending in "now", and four stat tiles.
  The chosen range persists.
- **Container Manager**: grouped by Container Manager Project, because that is
  how they were deployed — a compose file owns a set of containers that stop
  and start together, and one flat list threw that away. A card per project
  carries its compose path and status over a `GtkColumnView` of its
  containers; anything started with plain `docker run` lands in a "Not in a
  project" section. Each row carries CPU, memory, uptime, the state pill and
  under it the health check's verdict — a container can be Running and
  Unhealthy, which is the case the pill alone hides — or, when it is stopped,
  why. Per-row Start / Stop / Restart, a confirmation dialog for the two that
  interrupt a service, and a toast plus an immediate re-poll afterwards.
  Package-managed containers get a note instead of buttons.
- **Logs & security**: counts, a linked severity filter applied client-side,
  and the entries.
- **Six generic table pages** from one `TablePage` template, reached from the
  primary menu: System information, Packages, Shared folders, Users &
  sessions, Network, Temperature & power.

**7. Light actions.** Container start/stop/restart, in `core/src/action.rs`
so a second frontend gets the same verbs and the same JSON-quoting rule.

206 tests, `clippy -D warnings` clean.

## Next

**The eleventh page: Backup & snapshots.** Not built because it has no
verified endpoint — `SYNO.Backup.Task` answers 103 to `list`, `get` and
`list_info` on the target box, which has no Hyper Backup installed. Building
it against a guessed method would be exactly the mistake the rest of this
avoided. It needs a DiskStation with Hyper Backup to measure against first.

**Per-drive S.M.A.R.T. attributes.** Power-on hours and reallocated sectors
need `SYNO.Storage.CGI.Smart`, which answers 121 to a bare `list` — it wants a
disk parameter that has not been worked out. The overall S.M.A.R.T. verdict
shown today comes from `load_info` and is correct.

**Published ports per container.** Measured as reachable but not cheap: they
are absent from `SYNO.Docker.Container`/`list` and need
`SYNO.Docker.Container`/`get` with a name, one call per container per poll.
Worth doing only lazily — on the Container Manager page while it is open —
rather than adding N calls to the five-second Overview poll.

**Unfocused rate-limiting.** The design asks for a longer poll interval when
the window is not focused. The interval is currently constant.

**The secret store.** The session never touches disk, which is safe, but means
a password on every launch. Belongs behind a trait in `core` rather than in
the GTK half, since it is per-platform.

**Packaging.** `data/` and `install.sh` are done; the deb and Flatpak scripts
the siblings have are not written.

## Known gaps, stated rather than hidden

- **History only covers uptime of the app.** The structural consequence of
  DSM exposing no readable history. A collector container on the NAS would fix
  it and is deliberately deferred — see `DESIGN.md`.
- **The session is not kept in a secret store.** It never touches disk, which
  is safe, but it means a password on every launch. The platform secret store
  is the fix and is per-platform work, which is exactly the kind of thing that
  belongs behind a trait in `core` rather than in the GTK half.
- **The Overview's volume tile reads the first volume only.** Correct for
  this NAS and wrong for a multi-volume one; the storage list shows them all.
- **The trend charts draw the range chosen on the resource page.** The
  Overview's three sparklines follow it rather than having their own.
- **The banner pill does not count containers.** It is storage health plus a
  temperature override, deliberately — but that means an unhealthy container
  shows red in the containers panel while the banner above it still reads
  Normal.
