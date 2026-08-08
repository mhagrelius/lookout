# lookout

A Synology DiskStation monitor.

## Stack

GTK 4.22 + libadwaita 1.9 via gtk4-rs 0.11 / libadwaita-rs 0.9, Rust edition
2021 (MSRV 1.80). `gio` is a direct dependency purely to raise the API level to
v2_80 — leave it.

Crate is a lib + bin so integration tests and `examples/` can drive the real
application rather than a copy of it.

## Commands

- `./test.sh` — fmt check, clippy with `-D warnings`, then
  `cargo test --workspace --all-targets`. Add `--headless` to run under Xvfb
  and a private D-Bus session. This is the gate; run it, not bare `cargo test`.
- **Never run `dbus-run-session` or `xvfb-run -a dbus-run-session` directly** —
  use `isolated-bus [--headless] -- CMD`. A private bus activates its own
  `xdg-document-portal`, which mounts over `/run/user/$UID/doc` and takes the
  login session's portal down with it when the bus exits.
- `cargo run -p lookout-core --example discover -- HOST:PORT` — asks a real
  DiskStation what it exposes. Needs no credentials.
- `./install.sh` — release build, installs under `~/.local`. `./uninstall.sh`
  reverses it.

## Layout

`core/` is `lookout-core`: the DSM client, the records and the recorded
history. **It links no UI toolkit and holds no GTK types.** That is the whole
frontend/backend split — a Windows or macOS shell keeps this crate whole and
replaces `src/` entirely. Anything added to `src/` that a second frontend would
also need belongs in `core/` instead.

`src/` is widgets and the application.

## Working on this

- **`docs/dsm-api-notes.md` is measured, not documented.** Every field name,
  unit and method in it was read off a real DS-series. Several published
  references and the original design handoff are wrong in ways that are
  invisible until the number renders — `up_time` is hours, not days;
  `share_quota_used` is MiB; the namespace is `SYNO.Storage.CGI`, not `.CS`.
  **Verify a new endpoint against a real box before building on it**; the
  `discover` example and error 103 as a method oracle make that cheap.
- DSM reports failure in a 200 body, never a status code. That conversion
  happens in `core/src/dsm/envelope.rs` and nowhere else. Above that seam
  everything is a typed `Result`.
- `from_json` readers rather than `Deserialize` derives, deliberately: DSM
  sends the same value as a number in one version and a quoted string in the
  next, and omits fields without warning. A derive turns that into a parse
  failure and a blank page.
- Use the `developing-gtk-apps` and `designing-gnome-ui` skills for widget,
  threading, and HIG decisions rather than deriving them again.
- Edit files with the Edit tool. Do not rewrite Rust sources through
  `python3 - <<PY` heredocs or `sed -i`.
- The sibling apps (brain, planner, familiar, stickies) share this layout and
  these scripts; a pattern established in one is the pattern here.
