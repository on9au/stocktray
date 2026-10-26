# stocktray

A Windows 11 tray icon that shows a stock price. It defaults to WiseTech Global (ASX: WTC).

The icon shows the price as dollars above cents (`40` over `57`) on a coloured square: green
when the stock is up on the day, red when down, slate when unchanged and grey when stale.
Hovering shows the exact price, change and quote time:

```
WTC.AX  40.57 AUD
▲ +0.45 (+1.12%)
as of 3:57 pm AEDT
```

Right-click for Refresh now, Open quote page, Edit settings…, Start with Windows, updates and
Exit. Double-click to open the quote page.

## Install

Download `stocktray-<version>-setup.exe` from the
[latest release](https://github.com/on9au/stocktray/releases/latest) and run it. It installs
for your account only, to `%LOCALAPPDATA%\Programs\stocktray`, with no admin prompt. The
installer isn't code-signed, so SmartScreen may warn about an unrecognised app: choose
**More info → Run anyway**.

Windows 11 hides new tray icons behind `^`. To keep it visible, go to
**Settings → Personalization → Taskbar → Other system tray icons** and turn on stocktray.

Uninstall from **Settings → Apps → Installed apps**. Your settings file is kept.

## Updates

stocktray checks GitHub for a newer release at startup, every 12 hours, and when you click the
update item in its menu. When there is one, the tooltip says so and the menu offers
**Install update vX.Y.Z**. Clicking it downloads the installer, which replaces the app and
restarts it, keeping your Start with Windows choice.

## Settings

On first run stocktray writes `%APPDATA%\stocktray\config.toml`. Edit settings… opens it in
Notepad, and saved changes apply within a few seconds without a restart.

```toml
# Yahoo Finance symbol. ASX listings end in .AX (WTC.AX, CBA.AX, BHP.AX) and
# "ASX:WTC" works too. US listings have no suffix (MSFT, AAPL).
symbol = "WTC.AX"

# Seconds between refreshes while the market is open, and while it is closed.
poll_open_secs = 60
poll_closed_secs = 900
```

An invalid file is ignored; the previous settings stay in use and the tooltip says why.

## Data and polling

Quotes come from Yahoo Finance's chart API. For `.AX` symbols the ASX's own API is the
fallback. Neither API is documented, and both lag the market by about 20 minutes.

stocktray polls every `poll_open_secs` from the open until 30 minutes after the close, to
catch the delayed final price. Outside those hours it polls every `poll_closed_secs`, and it
wakes just after the next open. It refreshes straight away after the machine resumes from sleep.

Prices on the icon are truncated, not rounded, so the dollars always match the tooltip. From
$10,000 there's no room for cents, so 12,345.67 shows as `12k`.

## Development

```
cargo test                            # unit tests
cargo test -- --include-ignored       # also hit the live APIs
cargo run --example preview -- p.png  # render every icon variant, magnified
cargo run --example make_icon         # regenerate assets/stocktray.ico
.\package.ps1                         # build dist\stocktray-<version>-setup.exe
```

`package.ps1` needs [Inno Setup](https://jrsoftware.org/isinfo.php):
`winget install JRSoftware.InnoSetup`. Debug builds keep a console window; release builds don't.

| Module | Role |
|---|---|
| `quote` | `Symbol`, `Price`, `Quote`; the `QuoteSource` trait with `Yahoo`, `Asx` and `Fallback` |
| `config` | `Config` (validated from the file's `RawConfig`) and `ConfigFile`, which reloads on edit |
| `poller` | Background thread fetching from any `QuoteSource` on a market-hours schedule |
| `update` | The `ReleaseFeed` trait with `GitHub`, the update thread, and the menu item's `State` |
| `view` | `Feed` (loading / live / failed) and `View`: icon lines, colour, tooltip, menu header |
| `icon` | Renders text rows onto the coloured square |
| `main`, `menu`, `win` | The tray, its menu `Action`s, and Win32 helpers |

## Releases

Commits follow [Conventional Commits](https://www.conventionalcommits.org): `feat:` bumps the
minor version, `fix:` the patch. On every push to `main`, CI lints, tests and builds the
installer, and [release-please](https://github.com/googleapis/release-please) keeps a release PR
open with the next version and changelog. Merging that PR tags the release, and CI attaches the
installer to it, which is what installed copies update to.
