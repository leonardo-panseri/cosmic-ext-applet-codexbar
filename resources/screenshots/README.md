# Screenshots

`popup.png`, `credits.png` and `panel-page.png` are referenced by the metainfo (`resources/app.metainfo.xml`)
through their raw URLs on `main`, so keep the file names stable.

The screenshots are generated, not captured by hand: run `just screenshots` in a COSMIC (or any Wayland) session,
then check the result and commit it. `tools/screenshots.py` runs the applet's debug-only preview on
`snapshot.json` here, a made-up snapshot in the shape of the test fixtures, in COSMIC's default dark theme whatever
your own theme is. The panel is a mock: only the applet's button is drawn on it. The images come out at the
display's scale: 1× gives about 640 px wide, and a session at 200% gives twice that (libcosmic's own scale factor
doesn't change the saved pixels).

To add a screenshot, add an entry to `SHOTS` in the script with the preview's environment
(`CODEXBAR_PREVIEW_PROVIDER`, `CODEXBAR_PREVIEW_PANEL_PAGE`, and `CODEXBAR_PREVIEW_HEIGHT` to fit the window to the
popup), and a `<screenshot>` to the metainfo. Keep them at least 620 px wide, and keep email addresses,
organisation names, real plan names and real spending out of `snapshot.json`.
