# yap

A terminal client for [YiffSpot](https://www.yiffspot.com/), built with Rust and ratatui.
Users must be 18 or older.

It does everything the website does (preferences, partner matching, chat, typing
indicator, block, disconnect, themes, notifications), plus the following:

- **Profiles**: named preference sets, switchable with `Ctrl-P`. Export and import as TOML or
  JSON. Imports also accept the website's own `localStorage`, so you can bring your
  browser preferences across.
- **Drawer**: saved, labelled reference links (ref sheets, galleries). Drop one into the chat
  from the side panel in two keystrokes.
- **Inline images** from trusted domains, using the kitty graphics protocol (sixel, iTerm2
  and half-block fallbacks are detected automatically).
- **Themes**: the site's `dark` / `oled-dark` / `light`, a `terminal` theme that follows your
  terminal palette, several popular palettes, and your own TOML themes.
- **Traffic viewer**: every websocket frame in both directions, including handshake
  headers and ping/pong. Supports filtering, pretty JSON, sending raw frames and JSONL export.

## Running

```sh
cargo run --release
```

```
yap [--profile NAME] [--theme NAME] [--server wss://…] [--traffic-log FILE] [--no-mouse]
yap probe [SECONDS]              # connect, print raw traffic, disconnect (never searches)
yap export FILE [--profile NAME] # .toml or .json
yap import FILE [--with-settings]
yap themes | yap paths
```

`--no-mouse` gives you the terminal's own text selection back. In kitty you can also hold
Shift while dragging.

## Keys

| Key | Action |
| --- | --- |
| `F1` | help (all keys and commands) |
| `F2`–`F6` / `Alt-1`–`5` | Chat, Preferences, Drawer, Traffic, Settings |
| `Ctrl-F` | find a partner (asks first if you already have one) |
| `Ctrl-D` | leave partner; while searching, stop searching |
| `Ctrl-B` | block current or previous partner |
| `Ctrl-O` | links in the chat: open, preview, save to drawer, insert, copy, trust host |
| `Ctrl-E` | drawer side panel (`Tab` focuses it, `Enter` inserts the link) |
| `Ctrl-P` / `Ctrl-T` | switch profile / theme (the theme picker previews live) |
| `Ctrl-S` | toggle sidebar |
| `Ctrl-R` | reconnect |
| `PgUp`/`PgDn`, `Shift-↑/↓`, wheel | scroll; `Esc` jumps to the newest message |
| `↑`/`↓` | message history |
| `Ctrl-Q` / `Ctrl-C` | quit |

The chat box also takes commands: `/find`, `/leave`, `/block`, `/save <url> [label]`,
`/profile [name]`, `/theme [name]`, `/export <path>`, `/import <path>`, `/trust <domain>`,
`/log <path>`, `/raw <json>`, and more (`/help`). Start a message with `//` to send a
literal `/`.

## Files

`yap paths` prints the locations. They are usually:

- `~/.config/yap/config.toml`: settings and profiles. Saved automatically. A broken
  file is reported, not overwritten.
- `~/.config/yap/themes/*.toml`: custom themes.
- `~/.local/share/yap/drawer.toml`: the drawer.

### Importing from the website

In the browser console on yiffspot.com, run `copy(JSON.stringify(localStorage))`. Paste
the result into a `.json` file, then `/import` it (or run `yap import file.json`).

### Custom themes

```toml
# ~/.config/yap/themes/mine.toml
extends = "catppuccin-mocha"   # any built-in or earlier custom theme
[colors]
accent = "#ff79c6"
bg = "default"                 # terminal background
```

Colour slots: `bg fg muted surface accent you partner system link highlight selection_bg
selection_fg success warning error traffic_in traffic_out`. Values can be `#rrggbb`,
`#rgb`, names like `lightblue`, palette indexes `0`–`255`, or `default`.

## Images and privacy

Loading an image reveals your IP address to the server hosting it. Because of that:

- Only domains on the trusted list are ever loaded automatically. Subdomains count, so
  `e621.net` also trusts `static1.e621.net`. Lookalike hosts such as `evile621.net` do not.
- Every redirect is re-checked against the list, so a trusted host can't bounce you
  to a tracker.
- Only HTTPS is used by default. Downloads are capped at 10 MiB and 8192 px per side.
- Previewing an untrusted link asks first. That approval covers a single load; press `t`
  in the link picker to trust the host permanently.

Previews appear inline under the message. `p` opens a full-screen viewer. GIFs show
their first frame.

## Security notes

- Partner messages are stripped of control characters and bidi overrides before they
  reach the terminal. Otherwise a message could contain escape sequences that write to
  your clipboard, change the window title, or disguise where a link points.
- Desktop notification and clipboard escapes are built from sanitised text.

## Differences from the website

- The website sends "typing" on every keystroke and never turns it off. yap sends it once,
  then clears it when the box is emptied or after 5 idle seconds.
- The website sends `find_partner` *before* asking "find a new partner?". yap asks first.
- "Any / All" can't be combined with specific picks. The server treats `any` as a
  wildcard, so a combination would mean the same as Any on its own.
- The protocol can't cancel a search. `Ctrl-D` while searching reconnects, which removes
  you from the queue.
- On a dropped connection, yap reconnects with backoff (1 s up to 30 s) instead of asking
  you to refresh.
- The live site doesn't have the repo's language field yet. yap sends it anyway. The server
  only validates the first field of that object, so this is harmless. If it ever
  isn't, turn off *Settings → Send language preference*.

## Development

```sh
cargo test            # unit, UI snapshot, net and end-to-end tests
cargo clippy --all-targets
```

- `tests/support` is an in-process mock of the YiffSpot server. `tests/end_to_end.rs`
  runs two complete clients against it with scripted keystrokes.
- UI snapshots use [insta](https://insta.rs): `cargo insta review` after intentional
  UI changes.
- `vendor/yiffspot` is the upstream source (git submodule). `src/catalog.rs` must match
  its `src/models` lists exactly. See `THIRD_PARTY_NOTICES.md`.
