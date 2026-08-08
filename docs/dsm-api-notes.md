# DSM Web API, as measured

Everything here was read off the NAS — a **DS-series running DSM 7.2.2-72806
Update 4** — on 2026-08-06, not taken from documentation. Where a published
reference disagrees, that is noted, because in every case below the reference
is the one that is wrong.

## The shape of a call

Everything goes to `POST /webapi/entry.cgi` with a form body carrying `api`,
`version`, `method`, the call's own parameters, and `_sid`. The CSRF token
goes in an `X-SYNO-TOKEN` header.

Failure is **not** an HTTP status. Every call that reaches the CGI answers
`200` with `{"success": false, "error": {"code": N}}`.

String parameters must be **JSON-quoted**: `id="abc"`, not `id=abc`. An
unquoted id gets "not a json value", and one that looks like `8ec29f37…` is
parsed as a number in scientific notation.

## Discovery

`SYNO.API.Info` / `query` with `query=all` needs **no session**. It returned
**728 APIs**. That makes it a reachability probe before any password is typed,
and the source of truth for hiding sections whose namespace is absent.

## Corrections to the design handoff and to published references

| Claimed | Actually |
|---|---|
| `SYNO.Storage.CS.Storage`, `.CS.Volume`, `.CS.SMART` | **No `CS` namespace exists.** It is `SYNO.Storage.CGI.*` |
| `up_time` is `DD:HH:MM:SS` | It is `H:MM:SS`, hours unbounded. `64:48:7` is 2 days 16 hours, **not 64 days** |
| System temperature is `temperature` | DSM 7.3 sends `sys_temp`. `temperature` is absent |
| Utilization has `cpu`/`memory`/`network`/`disk`/`space` | Also `nfs`, `smb` and `lun`, which no community type covers |

Every other namespace the handoff names does exist.

## Verified units and encodings

- **Volume/pool byte counts arrive as quoted strings** — they exceed 2^53.
  A disk's capacity is a flat `size_total`; a volume's is nested under
  `size.total`. Different shapes, same reply.
- **Load averages are hundredths.** `"1min_load": 27` is 0.27. Rendered raw
  it reports a load of 27 on an idle six-bay NAS.
- **`share_quota_used` is MiB**, as a float. Established by summing it across
  all five shares — 9,994 GiB — against the volume's reported 10,001 GiB
  used. No other unit is consistent. `quota_value` is MiB too, and `0` means
  unlimited, not zero.
- **Memory is kilobytes** throughout `Utilization`.
- **Container `up_time` is seconds**, while system `up_time` is that colon
  string. Same idea, two encodings, one API apart.
- **`temperature_warning`, `sys_tempwarn` and `systempwarn` all ship in the
  same reply.** Any one being true is a warning.

## Compound requests

`SYNO.Entry.Request` / `request` (host offers v2; v1 verified working) takes
`compound` as a JSON array of `{api, method, version, …}` plus
`stop_when_error=false` and `mode="parallel"`. It answers:

```json
{"has_fail": true, "result": [{"api": …, "success": …, "data"/"error": …}]}
```

Per-call failures are isolated, which is what lets one dead endpoint grey out
one card rather than blanking the page. Inside `compound`, parameters are real
JSON values, not the stringified form the flat CGI takes.

## There is no performance history to fetch

`SYNO.ResourceMonitor.Setting` / `get` reports `enable_history: false`, and
`SYNO.ResourceMonitor.Log` / `list` returns `total: 0`.
`SYNO.Core.System.Utilization` has only `get` — no `list`, `get_history` or
`past` (all answer 103).

So the 1 h / 24 h / 7 d / 30 d ranges the design asks for **cannot be
retrieved**. The app records its own; see `core/src/trend.rs`. This is the
single biggest architectural consequence of measuring rather than assuming.

## Method names, confirmed by calling them

| API | Method | Notes |
|---|---|---|
| `SYNO.Core.System` | `info` | |
| `SYNO.Core.System.Utilization` | `get` | |
| `SYNO.Core.System.SystemHealth` | `get` | `{hostname, interfaces, rule, uptime}` |
| `SYNO.Storage.CGI.Storage` | `load_info` | volumes, disks, storagePools, ssdCaches |
| `SYNO.Core.Hardware.FanSpeed` | `get` | |
| `SYNO.Core.Service` | `get` | not `list` |
| `SYNO.Core.CurrentConnection` | `list` | `{items, systime, total}` |
| `SYNO.Core.Share` | `list` | takes an `additional` JSON array |
| `SYNO.Core.Package` | `list` | see the trap below |
| `SYNO.Core.SyslogClient.Log` | `list` | `{items, total, errorCount, warnCount, infoCount}` |
| `SYNO.Core.SecurityScan.Status` | `system_get` | not `get` |
| `SYNO.Core.User` | `list` | |
| `SYNO.Core.Network` | `get` | |
| `SYNO.Core.ExternalDevice.UPS` | `get` | |
| `SYNO.Docker.Container` | `list` | `limit=-1&offset=0&type=all` |
| `SYNO.Docker.Container.Resource` | `get` | |
| `SYNO.Docker.Project` | `list` | **keyed by id, not an array** |
| `SYNO.Storage.CGI.Smart` | — | `list` answers 121; needs a disk parameter |

Error **103** means "no such method", which makes it a reliable oracle for
discovering method names by trying them. **114** is the other half of that
oracle: the method exists and a required parameter is missing. Probing
`SYNO.Docker.Container` gives 114 for `get`, `list` and `export` — those are
real — while `SYNO.Docker.Container.Profile` gives 114 only for `export`, so
it is not a reader for container detail no matter how its name sounds.

## Container detail, measured on the NAS

**`up_time` is `null` on every compose-managed container.** The listing sends
`"up_time": null` alongside `"up_status": "Up 3 days (healthy)"`. Reading
`up_time` alone leaves the uptime blank for every container Container Manager
deployed, which on a NAS is usually all of them. The machine-readable start
time is `State.StartedAt`, RFC 3339 with nanoseconds:
`"2026-08-05T13:26:32.78343004Z"`.

**The listing already carries far more than the flat fields.** Each container
has a capitalised `State` object beside its lowercase `status`:

| Field | Meaning |
|---|---|
| `State.Status` | `running` — same word as the flat `status` |
| `State.StartedAt` / `FinishedAt` | RFC 3339; `0001-01-01T00:00:00Z` means never |
| `State.Health.Status` | `healthy` / `unhealthy` / `starting`, **absent when the image defines no healthcheck** |
| `State.Health.FailingStreak` | consecutive failed probes |
| `State.ExitCode` | why a stopped container stopped |
| `State.OOMKilled` | it was killed for memory, not by anyone |
| `State.Pid`, `Restarting`, `Paused`, `Dead`, `Error` | |

`Labels` carry the compose metadata directly —
`com.docker.compose.project`, `.service`, and `.project.config_files` — which
is a second, independent way to link a container to its project.

Published ports are **not** in the listing. They need
`SYNO.Docker.Container` / `get` with `name="..."`, one call per container,
which answers `NetworkSettings.Ports` as
`{"8083/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8083"}]}`. The listing gives
only the container's address on each network.

**`SYNO.Docker.Project` / `list` field names.** `status` is **upper case** —
`"RUNNING"`, not `"running"`. A project carries both `path`
(`/volume1/docker/web`, the real location) and `share_path` (`/docker/web`,
relative to the share); `path` is the one worth showing. `state` is present
and empty. Also `created_at`, `updated_at`, `is_package`, `version`.

## Two traps that fail silently

**`additional` keys are validated, and a wrong one empties the reply.**
`SYNO.Core.Package` / `list` with `additional=["status"]` returns 15 packages.
With `additional=["status","version"]` it returns **zero packages and
`success: true`** — because `version` is a top-level field, not an addition.
A three-element list earns `120 {"name":"additional","reason":"condition"}`.
So a wrong `additional` is not an error you can see; it is an empty page.

**A package's `status` is not where it looks.** The top level carries only
`id`, `name`, `version`, `timestamp`. `status` appears at
`additional.status`, and only when asked for. Reading a top-level `status`
yields `""` for everything and a page reporting that nothing is running.
The human name is `name`, not `dname`.

## Replies that are objects keyed by index

`SYNO.Core.Network.Interface` / `list` answers `{"0": {...}, "1": {...}}`,
not an array — as does `SYNO.Docker.Project` / `list`, which is keyed by
project id. `as_array` on either yields nothing and an empty page. Sort the
numeric keys as numbers, or "10" lands before "2".

Measured interface shape: `{ifname, ip, mask, speed, status, type, use_dhcp}`,
with `speed` in Mbit/s (`10000` for 10 GbE) and `ip`/`mask` empty strings when
the link is down.

## Cooling and UPS

`SYNO.Core.Hardware.FanSpeed` / `get` returns `all_disk_temp_fail` and
`cool_fan` as the **strings** `"yes"`/`"no"`, and the fan profile as
`dual_fan_speed` (`coolfan`, `quietfan`, …).

`SYNO.Core.ExternalDevice.UPS` / `get` carries `enable`, `manufacture`,
`model`, `charge`, `runtime` (seconds) and `mode`. Note it reports charge but
**not** whether the unit is discharging, so nothing here can honestly say "on
battery power".

## `SYNO.Backup.Task` is not usable as documented

`list`, `get` and `list_info` all answer **103** on this box. Hyper Backup is
not installed, so this may simply be its absence rather than a wrong method
name — but it means the Backup & snapshots page has no verified endpoint and
was not built.

## Photos, which is a large surface of its own

Two libraries with identical method names under different prefixes:
`SYNO.Foto.*` (personal, 5,962 items on this box) and `SYNO.FotoTeam.*`
(shared, 45,404). `Browse.Item` / `list` takes an `additional` array —
`exif`, `resolution`, `thumbnail`, `tag`, `person`, `gps`, `address`,
`video_convert`, `video_meta` all confirmed. `Search.Filter` / `list`
enumerates every filterable dimension. Personal-space folders are
`/MobileBackup/iPhone/YYYY/MM`.

Not modelled in v1; recorded here so it does not have to be rediscovered.

## Re-measuring

`core/examples/discover.rs` runs the unauthenticated half against any host:

```sh
cargo run -p lookout-core --example discover -- HOST:PORT [--insecure]
```
