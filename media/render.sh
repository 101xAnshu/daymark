#!/usr/bin/env bash
# Render the README tour from the real UI screenshots.
set -euo pipefail
cd "$(dirname "$0")"

npx remotion render src/index.ts DaymarkTour ../docs/media/daymark-tour.mp4 \
  --codec=h264 --crf=20

ffmpeg -y -v error -i ../docs/media/daymark-tour.mp4 \
  -vf "fps=15,scale=800:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128:stats_mode=diff[p];[s1][p]paletteuse=dither=none" \
  -loop 0 ../docs/media/daymark-tour.gif
