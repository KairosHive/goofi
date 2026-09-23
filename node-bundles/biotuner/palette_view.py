"""Display any RGB palette as labeled swatches and a continuous color ribbon."""
import numpy as np
import goofi
from PIL import Image, ImageDraw, ImageFont


def render_palette(rgb, title):
    colors = np.asarray(rgb, dtype=float).reshape(-1, 3)
    colors = colors[np.isfinite(colors).all(axis=1)]
    colors = np.clip(colors, 0, 1)[:16]
    image = Image.new('RGB', (720, 520), '#111111')
    draw = ImageDraw.Draw(image)
    title_font = ImageFont.load_default(size=27)
    small = ImageFont.load_default(size=16)
    draw.text((24, 20), title, font=title_font, fill='#eeeeee')
    draw.text((24, 60), 'One swatch per tuning degree', font=small, fill='#aaaaaa')
    if not len(colors):
        draw.text((24, 110), 'Waiting for a palette', font=small, fill='#aaaaaa')
        return image
    n = len(colors)
    cols = min(n, 4)
    rows = (n + cols - 1) // cols
    cell_w = 672 / cols
    cell_h = 285 / rows
    for i, c in enumerate(colors):
        rgb8 = tuple(np.round(c * 255).astype(int))
        x = 24 + (i % cols) * cell_w
        y = 101 + (i // cols) * cell_h
        draw.rounded_rectangle((x, y, x+cell_w-10, y+cell_h-40), radius=8, fill=rgb8)
        label = f'{i+1}  #' + ''.join(f'{v:02X}' for v in rgb8)
        draw.text((x, y+cell_h-32), label, font=small, fill='#cccccc')
    # Interpolation is explicitly labeled; the swatches above remain exact values.
    xx = np.linspace(0, n-1, 672)
    strip = np.stack([np.interp(xx, np.arange(n), colors[:, k]) for k in range(3)], axis=-1)
    ribbon = Image.fromarray(np.round(strip[None, :, :] * 255).astype(np.uint8)).resize((672, 62))
    image.paste(ribbon, (24, 414))
    draw.text((24, 488), 'Blended ribbon / interpolated between swatches', font=small, fill='#aaaaaa')
    return image


class PaletteView(goofi.Node):
    """Show an RGB palette as exact swatches, hex labels, and a blended ribbon.

    Connect BioColors.rgb, or any normalized [colors, 3] RGB array, to input.
    The display shows up to 16 finite colors. The ribbon interpolates between
    them; it does not change the swatches. Output is a normalized RGB image.
    """
    TAGS = ['image', 'transform']
    INPUTS = {'input': goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {'image': goofi.DataType.ARRAY}
    PARAMS = {'display': {'title': goofi.StringParam('Palette')}}

    def process(self, input):
        rgb = np.asarray(input.data)
        if rgb.ndim < 2 or rgb.shape[-1] != 3:
            raise ValueError('Use an RGB palette shaped [colors, 3].')
        return {'image': np.asarray(render_palette(rgb, self.params.display.title), dtype=np.float32) / 255.}
