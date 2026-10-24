# Tour media

The README tour is a 26 second, 60 fps Remotion composition that rebuilds the
Daymark interface from its design tokens and real application icons, so the
numbers, bars, search and rows can animate.

- `src/Tour.tsx` — the composition
- `public/apps/` — application icons used in the app list
- `public/score.m4a` — the original ambient score (`score.py` regenerates it)
- `render.sh` — renders the 1080p MP4 with sound, then the 720 px README GIF

```bash
npm install
./render.sh
```
