# Site

Static landing page at <https://jdylanmc.github.io/pr-sniper/>. Plain HTML, CSS and JS in `site/`; no build step, no runtime third-party requests.

## Files

| Path                                     | Holds                                                    |
| ---------------------------------------- | -------------------------------------------------------- |
| `site/index.html`, `style.css`, `app.js` | Chrome, tabs, themes, boot, sound, changelog renderer    |
| `site/demo.js`                           | Scripted tray-panel demo; fictional repos and people     |
| `site/fonts/`                            | VT323 Termlink (OFL, `OFL.txt`), built by the script     |
| `scripts/site-font.py`                   | Adds box-drawing, block and arrow glyphs to VT323        |
| `tests/site/`                            | Playwright suite and a server for the artifact layout    |
| `.github/workflows/pages.yml`            | Tests on PRs; tests and deploys on `main` and on request |

## Preview

| Command                                          | Serves                                                         |
| ------------------------------------------------ | -------------------------------------------------------------- |
| `node tests/site/serve.mjs`                      | Artifact layout on `127.0.0.1:1431` (`SITE_TEST_PORT`)         |
| `python3 -m http.server 8000` then open `/site/` | Repository root; the changelog falls back to `../CHANGELOG.md` |

## Test

`npm run test:site` runs Chromium and WebKit. Install browsers once with `npm exec playwright install chromium webkit`.

## Deploy

- `pages.yml` copies `site/` and `CHANGELOG.md` into one artifact and deploys it on pushes to `main` that touch `site/**`, `CHANGELOG.md`, `tests/site/**` or the workflow, and on manual runs.
- Needs **Settings › Pages › Source: GitHub Actions** once. Until then the deploy job fails at `configure-pages`.
- The shown version is hardcoded in `site/index.html` and `site/demo.js`; a test fails when it drifts from `package.json`.

## Font

Upstream VT323 has no box-drawing glyphs. Rebuild after editing the script:

```sh
python3 -m pip install fonttools brotli
curl -LO https://github.com/google/fonts/raw/eb8781e516576b414603df8cd267c0f21c9b4ee2/ofl/vt323/VT323-Regular.ttf
python3 scripts/site-font.py VT323-Regular.ttf site/fonts/vt323-termlink.woff2
```
