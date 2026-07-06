# riddle — Tom Riddle's diary for macOS

Write into a replica-style diary window. After a short pause, the page drinks
your ink, sends the handwriting to a vision LLM, and writes a reply back in an
animated hand.

This fork is being shaped primarily as a native macOS diary app:

- Apple Silicon desktop build as the default development target.
- A Tom Riddle diary-inspired window frame instead of a generic canvas.
- A quill-pen cursor and grey paper page.
- OpenAI-compatible HTTP oracle support with local `oracle.env` loading.
- Same-language replies, including Chinese/CJK rendering through a bundled
  fallback font.

The original reMarkable Paper Pro backend is still present, but it is now a
secondary/legacy target while the macOS experience is developed.

## Quick Start On macOS

```sh
cd riddle
cp oracle.env.example oracle.env
# edit oracle.env and set RIDDLE_OPENAI_KEY
cargo build --release
./target/release/riddle
```

Left-click and drag to write. Right-click and drag to erase. Press Escape or
close the window to quit.

Optional window size overrides:

```sh
RIDDLE_DESKTOP_W=720 RIDDLE_DESKTOP_H=960 ./target/release/riddle
```

The app keeps the original 1620x2160 internal page, so the handwriting PNG sent
to the model follows the same rendering path on desktop and device builds.

## Oracle Setup

The macOS build uses the OpenAI-compatible HTTP backend. Put credentials in
`riddle/oracle.env`, or export them in your shell. Real credential files are
ignored by git.

```sh
RIDDLE_OPENAI_KEY=your-api-key-here
RIDDLE_OPENAI_BASE=https://api.openai.com/v1
RIDDLE_OPENAI_MODEL=gpt-4o-mini
```

Any compatible vision model can work as long as it accepts image input through
`/chat/completions`. Some gateways require a base URL that ends in `/v1`; riddle
will also retry with `/v1` if the configured endpoint returns an HTML page.

Verify your endpoint without launching the diary:

```sh
./target/release/riddle --oracle-test path/to/handwriting.png
```

## Character Prompt

The response character is configured in `riddle/src/oracle.rs`. The current
persona is Tom Marvolo Riddle's memory preserved in the diary: intimate,
courteous, curious, subtly probing, short replies, no mention of AI/images, and
answers in the same language and script as the handwriting.

## How It Works

```text
mouse/stylus ink
  -> idle commit
  -> cropped handwriting PNG
  -> OpenAI-compatible vision LLM
  -> streamed reply chunks
  -> font rasterization
  -> skeletonized pen paths
  -> animated ink reply
```

Main components:

- `riddle/src/desktop.rs` — macOS window backend, diary frame, quill cursor.
- `riddle/src/oracle.rs` — endpoint setup, env loading, streaming SSE parser,
  persona prompt.
- `riddle/src/script.rs` — reply text rasterization, CJK fallback font stack,
  wrapping, thinning, tracing.
- `riddle/src/ink.rs` — user ink capture, erasing, dissolve effect, PNG export.
- `riddle/src/display.rs` — runtime display backend selection.

## Fonts

The reply hand uses
[Dancing Script](https://github.com/googlefonts/DancingScript) for Latin text
and falls back to [LXGW WenKai](https://github.com/lxgw/LxgwWenKai) for
Chinese/CJK glyphs. Both are SIL OFL 1.1; see `riddle/fonts/OFL.txt` and
`riddle/fonts/LXGWWenKai-OFL.txt`.

The CJK fallback font is large, so the release binary is larger than the
original reMarkable-only build.

## reMarkable Status

The original reMarkable Paper Pro paths are still in the tree:

- qtfb/AppLoad windowed backend.
- quill takeover backend.
- raw evdev pen, touch, and power-button support.

Those paths are kept for reference and possible future deployment, but this fork
is currently optimized and documented around macOS first.

### AppLoad/qtfb Build

```sh
cd riddle
cargo build --release --target aarch64-unknown-linux-gnu
```

### Takeover Build

Requires the reMarkable SDK toolchain and `libqsgepaper.so` from your own
device/SDK:

```sh
cd quill && ./build.sh
cd ../riddle && ./build-takeover.sh
```

Vendor libraries are not included in this repository.

## Safety

Do not commit real API keys. The repo ignores:

- `oracle.env`
- `riddle/oracle.env`
- build outputs under `riddle/target/`, `quill/build/`, and `quill/vendor/`

Use `riddle/oracle.env.example` as the template.

## License

MIT for this repository's code. Bundled fonts keep their own SIL OFL 1.1
licenses. reMarkable vendor libraries are not included and must come from your
own device/SDK.
