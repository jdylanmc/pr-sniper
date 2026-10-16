# Site

Static landing page at <https://jdylanmc.github.io/pr-sniper/>. Plain HTML, CSS and JS in `site/`; no build step, no runtime third-party requests.

## Files

| Path                                     | Holds                                                      |
| ---------------------------------------- | ---------------------------------------------------------- |
| `site/index.html`, `style.css`, `app.js` | Chrome, tabs, themes, boot, sound, changelog renderer      |
| `site/frame.js`                          | Box-drawing borders on the character grid                  |
| `site/mini.js`                           | Hero mini panel loop (thematic off)                        |
| `site/demo.js`                           | 16:9 desktop fit and full screen, shared by both pages     |
| `site/demo.html`, `demo-page.js`         | Plain demo page: header and the full desktop               |
| `site/img/sniper-mark.png`               | Copy of `prototypes/v2/sniper-mark.png` for the mini panel |
| `prototypes/v2/`                         | Design prototype embedded as the DEMO tab at `/demo/`      |
| `site/fonts/`                            | VT323 Termlink (OFL, `OFL.txt`), built by the script       |
| `site/fonts/jetbrains-mono-*.woff2`      | JetBrains Mono 400/700 (OFL, `OFL-JetBrainsMono.txt`)      |
| `scripts/site-font.py`                   | Adds box-drawing, block and arrow glyphs to VT323          |
| `tests/site/`                            | Playwright suite and a server for the artifact layout      |
| `.github/workflows/pages.yml`            | Tests on PRs; tests and deploys on `main` and on request   |

## Thematic

| State         | Look                                                                                                                      |
| ------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Off (default) | Nord landing page like <https://jdylanmc.github.io/>: JetBrains Mono, one scrolling page, no tabs, sidebar, boot or sound |
| On            | CRT terminal: VT323, tabs, sidebar, phosphor themes (T), boot once per session, sound (M), redraw on tab switch           |

- Header button `[THEMATIC ON]` / `[THEMATIC OFF]` (on) or "thematic" switch (off), `aria-pressed`, or H. Saved in `localStorage` `termlink-thematic` (`on` / `off`).
- `<html data-thematic>` is `off` in the markup. The inline `<head>` script sets the saved state, preloads that look's font and sets `theme-color` before first paint. A Content Security Policy would need its `sha256` hash.
- Turning it on never boots mid-session; the boot can run next session.
- Off page order: header (home, anchors, demo page, GitHub, toggle), hero, what, why, how, install (`#install`), changelog (latest release + full link), footer.
- Off hero: text left and the mini panel right at ≥1024px, stacked below. The mini panel is hand-built from the v2 prototype's values (408px, scaled), not an iframe. Its ~11 s loop: a pull request starts reviewing, the scope locks on, it turns Ready for you, the count goes up, a notification shows, the oldest row leaves. It pauses off-screen or in a hidden tab and shows a still under reduced motion. `data-step` / `data-cycle` expose the loop to tests.
- `demo.html`: always plain; the desktop fits the window below the header. Its thematic switch (or H) saves on and opens `index.html#demo`.
- One set of sections serves both looks. The markup is in page order; `applyMode()` in `app.js` adds tab roles, `hidden` and moves `#install` into WHAT when on, and removes them when off. CSS shows every section before `app.js` runs.
- Off: keys 1–5 and arrows keep browser behaviour; anchors scroll smoothly unless reduced motion.
- DEMO is only the 16:9 desktop. Full screen is a small button on the frame's bottom edge (`[FULL]` on, icon off; name "Full screen" / "Exit full screen"); Esc exits.
- Both looks: header link home to <https://jdylanmc.github.io/> ("dylan mccurry").
- Markup that exists in one look only uses `.t-on` / `.t-off`; off-look rules live at the end of `style.css` under `[data-thematic="off"]`.

## Preview

| Command                                          | Serves                                                         |
| ------------------------------------------------ | -------------------------------------------------------------- |
| `node tests/site/serve.mjs`                      | Artifact layout on `127.0.0.1:1431` (`SITE_TEST_PORT`)         |
| `python3 -m http.server 8000` then open `/site/` | Repository root; the changelog falls back to `../CHANGELOG.md` |

## Test

`npm run test:site` runs Chromium and WebKit. Install browsers once with `npm exec playwright install chromium webkit`.

## Deploy

- `pages.yml` copies `site/`, `CHANGELOG.md` and the v2 prototype's runtime files (to `demo/`) into one artifact and deploys it on pushes to `main` that touch `site/**`, `prototypes/v2/**`, `CHANGELOG.md`, `tests/site/**` or the workflow, and on manual runs.
- Needs **Settings › Pages › Source: GitHub Actions** once. Until then the deploy job fails at `configure-pages`.
- The shown version is hardcoded in `site/index.html`; a test fails when it drifts from `package.json`.

## Fonts

JetBrains Mono: `JetBrainsMono-Regular.woff2` and `JetBrainsMono-Bold.woff2` from `fonts/webfonts/` and `OFL.txt` in [JetBrains/JetBrainsMono v2.304](https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip) (zip SHA-256 `6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`). Fonts unmodified; `OFL-JetBrainsMono.txt` has trailing spaces trimmed.

Upstream VT323 has no box-drawing glyphs. Rebuild after editing the script:

```sh
python3 -m pip install fonttools brotli
curl -LO https://github.com/google/fonts/raw/eb8781e516576b414603df8cd267c0f21c9b4ee2/ofl/vt323/VT323-Regular.ttf
python3 scripts/site-font.py VT323-Regular.ttf site/fonts/vt323-termlink.woff2
```
