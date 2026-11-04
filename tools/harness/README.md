# Script harness

Use `THESIO_SCRIPT` to replay a line-by-line script that operates a window and captures screenshots. Each run copies `lib_dir` to `work_dir/lib` and points `THESIO_HOME` at the copy, leaving the user's document library untouched.

```sh
cargo build --release
python3 tools/harness/run.py tools/harness/examples/scroll.txt            # Defaults to lib_clean; output is in /tmp/thesio-run
python3 tools/harness/run.py my.txt /path/to/lib /tmp/my-run              # Custom library and output directory
THESIO_BIN=/other/thesio python3 tools/harness/run.py my.txt
```

- Screenshots: `<work_dir>/<name>.png`.
- Log: `<work_dir>/log`. Use `grep -a bench log` to view performance numbers.
- `lib_clean/` is a review library with an all-block styles review document and sample images.
- `winid.swift` lists the window IDs of Thesio windows on screen. Run `swift winid.swift` to use these IDs with system screenshots.

## Screenshot regression

```sh
python3 tools/harness/compare.py tools/harness/examples/menus.txt OLD_BINARY target/release/thesio
```

Replace `OLD_BINARY` with the path to the old binary.

- The comparison runs the same script against both versions and compares each screenshot pixel by pixel. For differences, it reports the number and bounds of differing pixels and writes a diff image to `/tmp/thesio-compare/diff-<name>.png`.
- For refactors that move code without changing appearance, such as splitting modules, changing tokens, or sharing components, every screenshot must be `identical`.
- `examples/menus.txt` opens the ellipsis menu and page width submenu, edit mode menu, slash menu, block menu, sidebar context menu, and help menu in sequence.
- The cursor doesn't blink during comparisons (`THESIO_NOBLINK`). Move the pointer to a blank area at the start of the script, or its actual position can appear as a hover state in screenshots.
- To build the old binary, run `git worktree add /tmp/old COMMIT`, replacing `COMMIT` with the desired commit. In that worktree, run `CARGO_TARGET_DIR=REPOSITORY/target cargo build --release`, replacing `REPOSITORY` with this repository's path. Then copy the binary elsewhere.

## Script commands

Use one command per line. Lines starting with `#` are comments. For complete details, see the start of `src/devtools.rs`.

| Command | Effect |
|---|---|
| `wait <ms>` | Wait and draw frames during the wait, including when the screen is locked |
| `key <key>` | Send a key, such as `key cmd-b` or `key escape` |
| `type <text>` | Enter text; trailing spaces are significant |
| `move x y` / `click x y` / `dclick x y` / `rclick x y` / `drag x1 y1 x2 y2` | Mouse operations in window coordinates |
| `wheel x y dy` | Send one wheel event |
| `shot <name>` / `burst <name> n` | Capture a screenshot or capture `n` screenshots about 8 ms apart |
| `mark <text>` / `commit <text>` | Compose or commit input method text |
| `clip html\|text <file>` | Set the simulated clipboard content for a subsequent `key cmd-v` |
| `pick <path>[\|<path>…]` | Supply paths for the next open or save dialog without showing a system panel |
| `resize w h` | Resize the window |
| `place x y` | Place the window's top-left corner at (x, y) on the primary screen, floating over other windows; use with `THESIO_NOPARK` for desktop screenshots |
| `deskshot <name> [margin]` | Capture the window and content behind it, with a margin in pt on each side (default 48) |
| `bench <n> <command>` | Run a command `n` times, drawing a frame synchronously each time, and report elapsed time |
| `record <file.mp4>` / `record stop` / `glide` / `scroll` / `caption` | Record a demonstration video |
| `activate` | Make the window the key window. **This takes focus; generally don't use it.** |
| `quit` | Exit; `run.py` appends this automatically |

Environment variables:

- `THESIO_SHOTS`: Screenshot directory; `run.py` sets this.
- `THESIO_TRACE`: Output an event trace.
- `THESIO_NOPARK`: Don't move the window offscreen.
- `THESIO_WINDOW_CAPTURE`: Use window server capture, including the system-drawn traffic lights.
- `THESIO_NOBLINK`: Disable cursor blinking; `compare.py` sets this.
- `THESIO_RECORD_FPS`: Recording frame rate, default 30.

## Usage rules

Scripts run on a computer the user is actively using. Follow these rules:

- Don't use the real system clipboard with ⌘C or ⌘V. Use `key cmd-v` only after `clip`.
- Don't take focus with `activate`. By default, the window moves offscreen for rendering.
- Don't select:
  - The **Show in Finder** button under Settings › Library.
  - Links, web search, or buttons that open links; these launch a browser.
  - Print; it opens the system print panel.
  - Quick Look or `qlmanage -p`.
- Don't use the `open` command.
- When you need to select files, use `pick a|b` instead of a system dialog.
- Don't use the user's real document library as `lib_dir`. If necessary, make a read-only copy and don't commit user content from the output.
