"""Run with the Goofi Python environment: python tests/python/test_cookbook_nodes.py."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import numpy as np

ROOT = Path(__file__).resolve().parents[2]

def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

fm = load('fm_signal', 'node-bundles/signal/fm_signal.py')
palette = load('palette_view', 'node-bundles/biotuner/palette_view.py')

class CookbookNodes(unittest.TestCase):
    def source(self, index=1.2, rate=1024.):
        node = SimpleNamespace(params=SimpleNamespace(
            fm=SimpleNamespace(carrier=18., modulator=5., index=index),
            output=SimpleNamespace(sfreq=rate)))
        with patch.object(fm.time, 'monotonic', return_value=0.):
            fm.FmSignal.setup(node)
        return node

    def emit(self, node, now):
        with patch.object(fm.time, 'monotonic', return_value=now):
            return fm.FmSignal.process(node)

    def test_irregular_blocks_match_continuous_signal(self):
        node = self.source()
        self.assertEqual(self.emit(node, .0001), {})
        blocks = [self.emit(node, t)['out'] for t in [.013, .061, .12, .5, 1.]]
        signal = np.concatenate([values for values, _ in blocks])
        self.assertEqual(len(signal), 1024)
        self.assertTrue(all(meta['sfreq'] == 1024 for _, meta in blocks))
        t = np.arange(1024)/1024
        np.testing.assert_allclose(signal, np.sin(2*np.pi*18*t+1.2*np.sin(2*np.pi*5*t)), atol=1e-6)

    def test_parameter_change_keeps_oscillator_phase(self):
        node = self.source(index=0.)
        self.emit(node, .125)
        phase = node.carrier_phase
        node.params.fm.carrier = 27.
        values, _ = self.emit(node, .25)['out']
        np.testing.assert_allclose(values, np.sin(phase+2*np.pi*27*np.arange(128)/1024), atol=1e-6)

    def test_modulation_adds_sidebands(self):
        plain = self.emit(self.source(index=0.), 1.)['out'][0]
        modulated = self.emit(self.source(), 1.)['out'][0]
        a,b = np.abs(np.fft.rfft(plain)), np.abs(np.fft.rfft(modulated))
        self.assertGreater(a[18], 500)
        self.assertLess(a[13], .001)
        self.assertGreater(b[13], 100)
        self.assertGreater(b[23], 100)

    def test_palette_preserves_swatch_colors(self):
        image = palette.render_palette([[1,0,0],[0,1,0],[0,0,1]], 'RGB')
        self.assertEqual(image.getpixel((50,120)), (255,0,0))
        self.assertEqual(image.getpixel((270,120)), (0,255,0))
        self.assertEqual(image.getpixel((500,120)), (0,0,255))
        self.assertEqual(palette.render_palette(np.empty((0,3)), 'Empty').size, (720,520))

    def test_palette_node_validates_shape_and_normalizes_image(self):
        node=SimpleNamespace(params=SimpleNamespace(display=SimpleNamespace(title='Test')))
        with self.assertRaises(ValueError):
            palette.PaletteView.process(node, SimpleNamespace(data=np.zeros((3,4))))
        result=palette.PaletteView.process(node, SimpleNamespace(data=np.array([[1.,0.,0.]])))['image']
        self.assertEqual(result.shape, (520,720,3))
        self.assertEqual(result.dtype, np.float32)
        self.assertTrue(np.isfinite(result).all())
        self.assertTrue(np.logical_and(result>=0,result<=1).all())

if __name__ == '__main__':
    unittest.main()
