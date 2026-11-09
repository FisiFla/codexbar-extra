# codexbar-cosmic-focus

Small helper that lists and activates toplevel windows on the COSMIC desktop,
used by `codexbar-linux` for the session "Focus" button.

## Why it is a separate program

The only Rust bindings for the COSMIC Wayland protocols (`cosmic-protocols`)
are licensed GPL-3.0-only. `codexbar-linux` is MIT, so those bindings must not
be linked into it. This helper is therefore its own cargo package
(`license = "GPL-3.0-only"`, full text in `LICENSE`) and its own binary.

`codexbar-linux` talks to it only by running it as a separate process and
reading its stdout (argv in, JSON and exit code out). Nothing is linked and no
code is shared.

## Usage

```sh
codexbar-cosmic-focus list              # JSON: [{"id","app_id","title"}, ...]
codexbar-cosmic-focus activate <id>     # id from `list`; exit 0, or 2 + stderr message
```

`id` is the `ext_foreign_toplevel_handle_v1` identifier. Activation uses
`zcosmic_toplevel_manager_v1.activate`; no input is ever synthesized.

Build: `cargo build --release --locked`.
