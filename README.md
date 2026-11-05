# PhotoWall

Windows-first portable photo wall. Open a folder and scroll an equal-width waterfall of its pictures. Built with Tauri 2, Rust, and a Vite frontend.

## What it does

- Open a folder, or drop one onto the window. Dropping a single photo opens the folder that contains it.
- Shows `jpg`, `jpeg`, `png`, `webp`, and `gif`. Hidden files and hidden folders are skipped. Subfolders are optional.
- Lays photos out in equal-width columns. Only tiles near the viewport are in the DOM.
- Each tile loads a small disk-cached JPEG first, then a sharper cached JPEG sized to the column.
- Click a photo for a fullscreen view. Left and right move between photos. Esc closes and puts the wall back at the same scroll position.
- Ctrl+wheel, `+` / `-`, or the column buttons change how many columns are on screen.
- The last folder, the subfolder toggle, and the column count are remembered.

Opening a few thousand photos stays scrollable because dimensions come from image headers and pixels are decoded only for tiles you can see. A folder of about ten thousand photos should open without creating a tile per file.

## Develop

Requirements: Node.js 20 or newer, and the latest stable Rust toolchain.

```bash
npm install
npm run tauri dev
```

On Linux, Tauri also needs the WebKitGTK development packages (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, and the usual Tauri build dependencies).

`npm test` checks the waterfall layout, the virtual window, and the thumbnail URL encoding.

## Windows portable build

The portable app is a single `PhotoWall.exe`. It is not an installer. WebView2 is required at runtime; current Windows 10 and Windows 11 already include it.

On a Windows machine with Node.js, the Rust MSVC toolchain, and the Visual Studio C++ build tools:

```bash
npm install
npm run tauri build -- --no-bundle
```

The executable is written to `src-tauri/target/release/PhotoWall.exe`. Zip that file as `PhotoWall-<tag>-windows-x64.zip` if you want the same archive the release workflow publishes.

Settings are stored in the OS config directory for `com.kalyan.photowall` (`settings.json`). JPEG thumbnails are cached under that app's cache directory in `thumbs/`. Both live in the user profile, so the exe folder stays portable and can be moved or deleted without an uninstall step.

## Releases

Pushing a tag that starts with `v` runs [`.github/workflows/windows-portable.yml`](.github/workflows/windows-portable.yml) on `windows-latest`. The workflow can also be started with `workflow_dispatch`. It builds `PhotoWall.exe` and uploads:

- `PhotoWall-<tag>-windows-x64.zip`
- `PhotoWall-<tag>-source.zip`

## Shortcuts

| Action | Keys |
| --- | --- |
| More or fewer columns | Ctrl+wheel, `+` / `-`, or the header buttons |
| Fullscreen | Click a photo |
| Previous / next | Left / Right |
| Leave fullscreen at the same scroll position | Esc |
