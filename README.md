# yap

A terminal client for [YiffSpot](https://www.yiffspot.com/), built with Rust and ratatui.
Users must be 18 or older.

It does everything the website does (preferences, partner matching, chat, typing
indicator, block, disconnect, themes, notifications), plus the following:

- **Profiles**: named preference sets, switchable with `Ctrl-P`. Export and import as TOML or
  JSON. Imports also accept the website's own `localStorage`, so you can bring your
  browser preferences across.
- **Drawer**: saved, labelled and tagged reference links (ref sheets, galleries) plus your
  snippets. Filter by tag, drop a link into the chat from the side panel, and export or
  import the whole drawer (`E`/`I`) to move it between machines.
- **Chat logs**: every partner gets their own conversation in the Logs tab, which you can
  pin, name, and search (including what was said). Logs can be saved to disk (off by
  default). Optionally, the chat view starts fresh for each partner instead of scrolling
  forever.
- **Notifications**: title flash, bell, desktop notification and an optional sound while
  you're away; keywords (say, your character's name) always notify. While you're in the
  app but looking elsewhere, you get a toast instead.
- **Several chats at once**: `alt+n` opens another chat (its own connection, so the server
  sees a separate person, just like a second browser tab). Chats show as tabs above the
  transcript with unread counts; `ctrl+pgup`/`ctrl+pgdn` or a click switches. Each chat
  has its own profile: switch profile (`Ctrl-P`) in one tab and the others keep theirs,
  including when they search again in the background.
- **Snippets**: saved text (your intro, your limits, a polite goodbye) inserted with `^G`
  or `/snip <name>`, or type `;intro` and press `Tab`. `{species}`, `{partner_species}`
  and friends are filled in.
- **Fast re-rolls**: `^N` skips to a new partner without asking, and an optional
  auto-requeue searches again a few seconds after a partner leaves.
- **Auto-skip rules**: limits per profile (kinks you never want), a minimum number of
  shared kinks, and a language check. The server doesn't know about these; yap just
  leaves and searches again.
- **Write in your editor**: `^X` opens `$EDITOR` for long posts; blank lines become a
  paragraph separator you choose, since the site only takes one line per message.
- **Roleplay-friendly display**: `*actions*` show in italics and `((asides))` dimmed. Set
  your character's name per profile (`/name`) and nickname each partner (`/nick`); both
  replace "you" and "partner" on screen and in logs. Nothing about what's sent changes.
- **Select and search**: `alt+m` selects a message to quote into your reply, copy, open,
  save its link, or keep as a snippet. `alt+/` searches the current chat with
  highlighted matches.
- **Command palette**: `ctrl+k` fuzzy-searches every action, command, setting, profile,
  theme, snippet and open chat.
- **Kinks, explained**: every kink on the site has a one-line definition, shown under
  the highlighted kink in Preferences. `alt+k` (or `/kinks`, or a click on the sidebar's
  list) sets your partner's kinks against yours: shared, theirs only, and yours they
  didn't list, each with what it means. The definitions ship with yap, so looking one
  up never touches the network.
- **Partner timers**: the sidebar shows how long you've been chatting, when your partner
  last said something (in amber after five quiet minutes), how long they've been typing,
  and how long you've been searching. Shared kinks are listed first.
- **Post length**: the sidebar compares your partner's average words per message with
  yours, and the message box counts words as you type.
- **"New" divider**: messages that arrived while you were in another tab, window or chat
  sit under a *new* line until you reply.
- **Partner history**: `/history` (or `h` in `/stats`) lists everyone you've met: who they
  were, how long it lasted, who spoke how much, and how it ended. Patterns sit on top:
  how chats tend to end, which species you meet most and for how long, whether shared
  kinks mean longer chats, and which auto-skip rule fires most. Only these facts are
  kept, never messages. Turn it off in *Settings → Keep partner history*; `x` in the list
  forgets it.
- **Spellcheck**: misspelled words are underlined as you type; `alt+s` offers fixes or adds
  the word to your dictionary. Species, kink names and your characters' names are
  known already. yap uses a system Hunspell dictionary if you have one
  (`spell_language` in `config.toml`, e.g. `en_GB`), else its built-in US English one.
- **Lost message warnings**: the server silently drops messages to a partner who has
  already gone. Anything you sent in the two seconds before they left is marked *may
  not have arrived*, so you know to copy it before starting over.
- **Emoji shortcodes**: `:smile:` is sent as 😄 (GitHub/Slack names). While typing
  `:smi…`, suggestions appear above the input and `Tab` takes the first one. Turn it
  off in *Settings → Emoji shortcodes*.
- **Stats**: `/stats` shows partners met, auto-skips, messages and time chatting, for
  this session and all time. Kept locally.
- **Writing comfort**: `Alt-Enter` (or `Shift-Enter` where the terminal reports it)
  starts a new paragraph right in the message box; paragraphs are joined with your
  separator when sent. `Ctrl-Z` / `Ctrl-Y` undo and redo, a word at a time.
- **More Tab completion**: after a command, Tab fills in its argument: `/theme n` →
  `/theme nord`, and likewise profiles, snippets, chat numbers, trusted hosts and file
  paths (`/log ~/Doc` → `/log ~/Documents/`).
- **Emoji picker**: `Alt-E` (or `/emoji`) opens a searchable grid; Enter inserts.
- **Link hints**: `Alt-L` puts a letter on every link on screen. Press the letter to
  open it (images preview), or Shift plus the letter to copy it.
- **Split view**: `Alt-V` puts another chat, or the traffic log, beside this one;
  `Alt-O` (or a click) switches which chat you're typing in.
- **Tabs come back**: the chat tabs you had open, each with its profile, reopen next
  time (*Settings → Reopen chat tabs at startup*).
- **When to look**: `/stats` charts how busy the site is by hour and which hours you
  match quickest, from your own searches.
- **A buddy**: a little companion by the message box (a fox, unless you pick another)
  that reacts to matches, messages, your name, hearts, partners leaving and more, with
  the odd comment. Click to pet it. `/buddy cat`, `/buddy off`, or *Settings → Buddy*.
  You can draw your own (see below).
- **Small touches**: a line marks long pauses and new days in the chat; your
  character's name is highlighted when your partner uses it; the window title counts
  unseen messages; errors stay up until `Esc` or a click; footer hints are clickable;
  in a narrow terminal `Ctrl-S` shows the sidebar over the chat; the first start walks
  you to your profile.
- **Image viewer**: `←`/`→` step through every image in the chat, with where it came
  from, its size and a button row along the bottom. Untrusted hosts ask right there.
- **Mouse**: click tabs, chats, list rows (double-click to activate), messages to open
  their links, and inline images to preview them. The image viewer has buttons to open,
  copy the link, save to the drawer, or save the image file to your Downloads.
- **Three chat layouts**: cozy (name above each message), compact (one line each), and
  messages (bubbles, received on the left, sent on the right).
- **Inline images** from trusted domains, using the kitty graphics protocol (sixel, iTerm2
  and half-block fallbacks are detected automatically).
- **Themes**: the site's `dark` / `oled-dark` / `light`, a `terminal` theme that follows your
  terminal palette, several popular palettes, and your own TOML themes. A transparent
  background option lets your terminal's own background show through.
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
yap mcp                          # MCP server for AI apps (they start it; see below)
yap backup FILE [--with-logs]    # settings, profiles, themes, buddies, drawer, stats, history
yap restore FILE                 # replaced files are kept as <name>.before-restore
yap themes | yap paths
```

`--no-mouse` gives you the terminal's own text selection back. In kitty you can also hold
Shift while dragging.

In kitty and Ghostty yap knows the image protocol already. Elsewhere it asks the
terminal at startup, and a terminal that ignores the question can swallow your first
keypress (a ratatui-image limitation). Turning off *Settings → Image previews* skips it.

## Keys

| Key | Action |
| --- | --- |
| `F1` | help (all keys and commands) |
| `F2`–`F7` / `Alt-1`–`6` | Chat, Preferences, Drawer, Logs, Traffic, Settings |
| `Ctrl-F` | find a partner (asks first if you already have one) |
| `Ctrl-N` | skip to the next partner, no questions asked |
| `Ctrl-G` | insert a snippet |
| `Ctrl-X` | write the message in your editor |
| `Ctrl-K` | command palette |
| `Alt-M` | select a message (quote, copy, save) |
| `Alt-/` | search this chat |
| `Alt-K` | your partner's kinks next to yours, explained |
| `Alt-S` | fix the misspelled word at the cursor |
| `Alt-E` | emoji picker |
| `Alt-L` | link hints: open a link by letter |
| `Alt-V` / `Alt-O` | split view / switch pane |
| `Alt-Enter` | new paragraph in the message box |
| `Ctrl-Z` / `Ctrl-Y` | undo / redo in the message box |
| `Alt-N` / `Alt-W` | open / close a chat |
| `Ctrl-PgUp` / `Ctrl-PgDn` | previous / next chat |
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

All of these except `Ctrl-C` can be rebound: *Settings → Keys*, press `Enter` on an action,
then press the new key. `Backspace` restores the default and `x` unbinds. Rebinds are saved
as overrides in the config:

```toml
[settings.keys]
find = "alt+f"
leave = ["ctrl+l", "f9"]
block = []          # unbound
```

Keys are written like `ctrl+f`, `alt+shift+x`, `f5`, `pgup`, `shift+up` or `ctrl+plus`.
Keys needed for typing and editing (plain letters, Enter, arrows, Esc, Tab, `Ctrl-W`,
`Ctrl-U`) can't be taken. An unknown action or key in the file is reported at startup,
and the rest of the config still loads.

The chat box also takes commands: `/find`, `/leave`, `/block`, `/save <url> [label]`,
`/profile [name]`, `/theme [name]`, `/export <path>`, `/import <path>`, `/trust <domain>`,
`/log <path>`, `/kinks`, `/raw <json>`, and more (`/help`). `/log` and the Logs tab's
export pick the format from the extension: `.txt`, `.md`, or `.html` (a standalone page
that keeps the roleplay formatting and shows images from trusted hosts). Start a message with `//` to send a
literal `/`. While you type a command, matching ones are listed above the input, and
`Tab` completes the first: `/lo` becomes `/log `.

## Files

`yap paths` prints the locations. They are usually:

- `~/.config/yap/config.toml`: settings and profiles. Saved automatically. A broken
  file is reported, not overwritten.
- `~/.config/yap/themes/*.toml`: custom themes.
- `~/.local/share/yap/drawer.toml`: the drawer.
- `~/Downloads` (or your XDG download folder): images saved from the viewer.
- `~/.local/share/yap/stats.toml`: your chat statistics.
- `~/.local/share/yap/history.jsonl`: partner history (private, 0600).
- `~/.config/yap/dictionaries/<lang>.aff` / `.dic`: extra spellcheck dictionaries.
  `/dict get en-GB` downloads one from wooorm/dictionaries and switches to it;
  `/dict use <lang>` switches between those you have.
- `~/.config/yap/buddies/*.toml`: your own buddies.
- `~/.local/share/yap/tabs.toml`: the tabs to reopen.
- `~/.local/share/yap/activity.json`: how busy the site is and how long searches take,
  by hour.
- `~/.local/share/yap/logs/*.jsonl`: chat logs, one file per partner, only when *Save chat
  logs to disk* is on. The folder is private (0700) and the files are 0600. Deleting a chat
  in the Logs tab deletes its file.

`config.toml` and the themes folder are watched while yap runs, so edits made in
another editor apply within a second. A config file with a mistake in it is reported,
and the current settings are kept.

### Snippets

In the Drawer tab press `s` to switch to snippets, then `a` to add one (leave the text
empty to write it in your editor). Or from the chat box: `/snip-add intro Hi! I'm a
{species} looking for a {partner_species}...`. These placeholders are filled in when you
insert: `{profile}`, `{gender}`, `{species}`, `{role}`, `{partner_gender}`,
`{partner_species}`, `{partner_role}`, `{partner_language}`.

### Drawer tags

Any `#word` in a label becomes a tag: `/save https://… ref sheet #ref #nsfw`. In the Drawer
tab, `t` edits tags and `[` / `]` (or `Tab`) step through tag filters. Searching with `/`
accepts `#tag` terms too.

### Importing from the website

In the browser console on yiffspot.com, run `copy(JSON.stringify(localStorage))`. Paste
the result into a `.json` file, then `/import` it (or run `yap import file.json`).

### Your own buddy

A buddy is up to 3 lines tall and 12 columns wide. Each mood is a list of frames that
loop; any mood you leave out uses `idle`. `extends` borrows everything else from
another buddy, and `[says]` replaces what it says for an event (`{partner_species}` and
the other snippet placeholders work). Files are picked up as soon as you save them.

```toml
# ~/.config/yap/buddies/bun.toml
extends = "cat"        # optional
color = "#f7768e"      # optional; the theme's accent otherwise
speed = 600            # ms per frame

[moods]
idle = ['''
 (\_/)
 ( •.•)
 / >♥ ''', '''
 (\_/)
 ( -.-)
 / >♥ ''']
happy = ['''
 (\_/)
 ( ^.^)
 / >♥ ''']

[says]
matched = ["a {partner_species}! hi!", "*hops over*"]
petted = ["*nose wiggle*"]
```

Moods: `idle happy excited love sad surprised sleepy curious searching proud dizzy`.
Events for `[says]`: `matched shared_kinks message mentioned heart typing sent
long_post left dropped skipped blocked searching connection_lost reconnected petted`.

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
*Settings → Popup style* sets how popups look: `outline` (a border, filled inside
it), `solid` (a filled card with no line, which suits transparent terminals), or
`clear` (see-through).

A custom theme whose text would be hard to read on its background gets a warning when
it loads.

## AI tools (MCP)

yap can lend its chats to an AI app (Claude Desktop, Claude Code, or anything else
that speaks MCP), which can then translate, summarise a long chat, suggest a reply,
polish your draft, or look through your logs and history for you. yap doesn't include
an AI or hold any keys; it offers tools, and the AI app you already use does the rest.

1. In yap: *Settings → AI tools (yap mcp)*: `read` lets the AI read your chats, logs,
   history and stats. `full` also lets it draft into your message box, find, skip,
   leave and block partners, switch profiles, nickname partners, and save snippets and
   links. Each change shows a toast, and `· ai` appears in the header while it's active.
2. Tell your AI app to run `yap mcp`. For Claude Code: `claude mcp add yap -- yap mcp`.
   For Claude Desktop, add this to its config:

   ```json
   { "mcpServers": { "yap": { "command": "yap", "args": ["mcp"] } } }
   ```

The AI works with the yap you have open (over a private local socket only you can use).
**It never sends a message on its own**: when it asks to, yap shows you the message,
and `y` sends it, `e` puts it in your message box to edit, and `n` declines. Keep in
mind that whatever the AI reads, your partner's messages included, goes to that AI's
provider, and your partner hasn't agreed to that. A local model avoids it, and hosted
models may decline explicit content anyway.

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
- Like the website, "Any / All" can be combined with specific picks. For kinks that means
  "match anyone, but show my partner what I'm into". Unticking everything falls back to
  Any, because the server rejects empty lists.
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
