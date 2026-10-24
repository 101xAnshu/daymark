# Tour media

The README tour is rendered with [Remotion](https://www.remotion.dev/) from the
real UI screenshots in `public/`.

Regenerate after a UI change:

1. Render fresh screenshots (see the main README) and copy them here:
   `docs/daymark-light.png` → `public/dashboard.png`,
   `docs/daymark-history.png` → `public/history.png`,
   `docs/daymark-settings.png` → `public/settings.png`.
2. `npm install`
3. `./render.sh`

`render.sh` writes the 1080p MP4 and a 15 fps, 800 px GIF to `../docs/media/`.
