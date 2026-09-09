"""Convert frames from the harmonic_geometry public tests into cookbook images.

Run the test with --features embed first to include the nine patch previews.
The test writes raw float32 frames under target/harmonic-geometry/frames.
Pass frame stems to convert selected pictures without rebuilding the atlas.
"""

from pathlib import Path
import argparse
import json
import numpy as np
from PIL import Image, ImageDraw


HERE = Path(__file__).resolve().parent
FRAMES = HERE.parents[1] / 'target/harmonic-geometry/frames'
OUT = HERE / 'assets'
OUT.mkdir(parents=True, exist_ok=True)
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('frames', nargs='*', help='Frame stems to convert; omit for all frames and the atlas.')
selected = parser.parse_args().frames
for stem in selected:
    if not (FRAMES/(stem+'.f32')).is_file():
        parser.error(f'No rendered frame: {stem}')

tiles = []
for path in sorted(FRAMES.glob('*.f32')):
    if selected and path.stem not in selected:
        continue
    shape = json.loads(path.with_suffix('.json').read_text())
    data = np.fromfile(path, dtype=np.float32).reshape(shape)
    image = Image.fromarray(np.uint8(np.clip(data, 0, 1)*255))
    image.save(OUT / (path.stem+'.png'))
    if path.stem.startswith(('gpu-', 'relief-', '0')):
        continue
    tile = Image.new('RGB', (320, 245), (11, 20, 32))
    tile.paste(image.resize((320, 220)), (0, 0))
    ImageDraw.Draw(tile).text((12, 223), path.stem, fill=(220, 230, 220))
    tiles.append(tile)

if selected:
    raise SystemExit(0)
if not tiles:
    raise SystemExit('No geometry frames found. Run the harmonic_geometry public tests first.')
sheet = Image.new('RGB', (320*6, 245*((len(tiles)+5)//6)), (11, 20, 32))
for i, tile in enumerate(tiles):
    sheet.paste(tile, (i%6*320, i//6*245))
sheet.save(OUT / 'atlas.png')
