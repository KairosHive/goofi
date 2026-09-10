"""Build the cookbook's deterministic .gfi archives from their recipe definitions.

Run from the repository root with .gfivenv/Scripts/python.exe on Windows,
or .gfivenv/bin/python on Linux/macOS. The public-session and browser tests
load these archives through goofi's session load operation.
"""

from pathlib import Path
from decimal import Decimal
import json
import tomllib
import zipfile
import yaml


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
VERSION = tomllib.loads((ROOT/'Cargo.toml').read_text())['workspace']['package']['version']
RELIEF_HEADER = json.loads((ROOT/'node-bundles/harmonic-geometry/HarmonicRelief.wgsl').read_text().split('/* goofi', 1)[1].split('*/', 1)[0])
RELIEF_TEXTURES = next(p['options'] for p in RELIEF_HEADER['params'] if p['name'] == 'texture_a')


class Patch:
    def __init__(self, slug, title, auto=True, mix=0.3):
        self.slug, self.title = slug, title
        self.nodes, self.links, self.names, self.globals = {}, [], {}, []
        self.views = []
        self.canvas = False
        self.monitor = None
        self.row = 0
        self.extra = {}
        for name, value in [('default_ufreq', 20.0), ('default_width', 256), ('default_height', 256)]:
            self.globals.append({'name': 'system.'+name, 'type': 'int' if isinstance(value, int) else 'float', 'value': value})
        self.control('auto', auto, kind='toggle')
        self.control('mix', mix, 0, 1)
        self.node('slow', 'signal:LFO', (-380, 0), lfo={'frequency': 0.018, 'phase': 0.75}, common={'max_frequency': 20.0})
        self.bind('slow', 'lfo', 'amplitude', '0.5 if globals.geometry.auto else 0.0')
        self.bind('slow', 'lfo', 'offset', '0.5 if globals.geometry.auto else globals.geometry.mix')

    def control(self, name, value, lo=0.0, hi=1.0, kind='slider', options=None, step=None):
        typ = 'bool' if isinstance(value, bool) else 'string' if isinstance(value, str) else 'float'
        control = {'kind': kind, 'x': 0.0, 'y': self.row, 'w': 16.0, 'h': 2.0 if kind != 'text' else 3.0}
        if typ == 'float':
            # Default steps reach both decimal endpoints; custom steps can also align the initial value.
            if step is None: step = float((Decimal(str(hi))-Decimal(str(lo)))/200)
            control.update(min=lo, max=hi, step=step)
        if options: control['options'] = options
        self.row += control['h']
        self.globals.append({'name': 'geometry.'+name, 'type': typ, 'value': value, 'control': control})

    def node(self, name, typ, pos, **params):
        if typ.startswith('graphics:'):
            for dimension in ('width', 'height'):
                params.setdefault('common', {}).setdefault(dimension, 256)
        if typ == 'signal:GeometryView':
            params.setdefault('display', {}).update(size=256)
        # Each archive has its own node identities, including in browser stream caches.
        uid = f'{int(self.slug[:2]):04x}{len(self.nodes)+1:08x}'
        self.names[name] = uid
        self.nodes[uid] = {'type': typ, 'name': name, 'pos': list(pos), 'params': params}
        return name

    def bind(self, name, group, param, expression=None, reference=None):
        source = {'group': group, 'name': param, 'mode': 'reference' if reference else 'expression',
                  'expression': expression or '', 'reference': reference or '', 'triggers': False}
        self.nodes[self.names[name]].setdefault('sources', []).append(source)

    def wire(self, a, out, b, inp):
        self.links.append([self.names[a], out, self.names[b], inp])

    def harmonic(self, name='chord', a='1, 5/4, 3/2', b='1, 6/5, 3/2, 7/4', pos=(0, 0), moving=True):
        self.node(name, 'signal:HarmonicMorph', pos, source={'ratios_a': a, 'ratios_b': b})
        if moving: self.wire('slow', 'out', name, 'mix')
        return name

    def geometry(self, name, method, source='chord', pos=(380, 0), **params):
        params.setdefault('geometry', {}).update(method=method)
        self.node(name, 'signal:HarmonicGeometry', pos, **params)
        self.wire(source, 'harmonic', name, 'input')

    def view(self, name, source, title, caption, harmonic='chord', pos=(760, 0), **params):
        params.setdefault('display', {}).update(title=title, caption=caption)
        self.node(name, 'signal:GeometryView', pos, **params)
        self.wire(source, 'geometry', name, 'input')
        if harmonic: self.wire(harmonic, 'harmonic', name, 'harmonic')
        return name

    def display(self, node, slot='dashboard', kind='image'):
        self.views.append((node, slot, kind))
        self.nodes[self.names[node]].setdefault('viewers', {})[slot] = {'kind': kind, 'settings': {}, 'collapsed': True}

    def ink(self, source, slot='out', name='ink', upload=False, **params):
        if upload:
            self.node(name+'Upload', 'graphics:SignalIn', (1100, 350))
            self.wire(source, slot, name+'Upload', 'input')
            source, slot = name+'Upload', 'out'
        self.node(name, 'graphics:HarmonicInk', (1450, 350), **params)
        self.wire(source, slot, name, 'input')
        return name

    def save(self):
        seq = 0
        def panel(typ, state=None, size=1.0):
            nonlocal seq
            seq += 1
            if typ == 'viewer':
                state = {**state, 'node': self.names[state['node']]}
            return {'kind': 'panel', 'id': f'panel-{seq}', 'size': size, 'panel_type': typ, 'state': json.dumps(state)}
        def split(axis, children, size=1.0):
            nonlocal seq
            seq += 1
            return {'kind': 'split', 'id': f'split-{seq}', 'size': size, 'axis': axis, 'children': children}
        control = panel('control', {'group': 'geometry'}, .25)
        display_state = dict(zip(('node', 'slot', 'kind'), self.views[0]))
        if self.canvas: display_state['settings'] = {'stretch': True}
        display = panel('viewer', display_state, .75)
        content = display
        if self.monitor:
            display['size'] = .78
            monitor = panel('viewer', dict(zip(('node', 'slot', 'kind'), self.monitor)), .22)
            content = split('column', [display, monitor], .75)
        play = split('row', [control, content])
        tabs = [{'id': 'tab-play', 'name': 'Play', 'root': play}]
        viewpoint = {'tab': 'tab-play', 'panel': display['id'], 'paths': {}}
        if self.canvas:
            canvas = panel('viewer', {**dict(zip(('node', 'slot', 'kind'), self.views[0])), 'settings': {'stretch': True}})
            tabs.insert(0, {'id': 'tab-canvas', 'name': 'Canvas', 'root': canvas})
        for i, v in enumerate(self.views[1:]):
            tabs.append({'id': f'tab-view-{i}', 'name': v[0], 'root': panel('viewer', dict(zip(('node', 'slot', 'kind'), v)))})
        tabs.append({'id': 'tab-patch', 'name': 'Patch', 'root': panel('node-editor')})
        patch = {'version': 1, 'goofi': VERSION, 'globals': self.globals, 'global_groups': {},
                 'root': {'nodes': self.nodes, 'links': self.links}, 'arrangement': {'tabs': tabs, '#seq': seq+10},
                 'viewpoint': viewpoint}
        path = HERE/(self.slug+'.gfi')
        with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as z:
            for name, data in {'patch.yaml': yaml.safe_dump(patch, sort_keys=False), 'workspace/.goofiignore': '', **self.extra}.items():
                info = zipfile.ZipInfo(name, (2026, 9, 9, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                z.writestr(info, data)
        return {'file': path.name, 'title': self.title, 'views': self.views, 'nodes': len(self.nodes)}


def recipes():
    p = Patch('01-breathing-lines', 'Breathing lines')
    p.harmonic()
    p.control('phase', .2, -3.14, 3.14)
    p.control('stretch', 1.0, .7, 1.4)
    p.control('damping', .025, 0, .2)
    p.control('yaw', .55, -3.14, 3.14)
    p.control('form', 'trace_3d', kind='dropdown', options=['trace_3d', 'lateral', 'rotary', 'compound'])
    for key in ('phase', 'stretch', 'damping'): p.bind('chord', 'morph', key, 'globals.geometry.'+key)
    p.geometry('trace', 'trace_3d')
    p.bind('trace', 'geometry', 'method', 'globals.geometry.form')
    p.view('lineStudy', 'trace', 'Breathing lines', 'Retune the chord, turn the phase, change the drawing instrument.', camera={'autofit': False, 'radius': 1.25})
    p.bind('lineStudy', 'camera', 'yaw', 'globals.geometry.yaw')
    p.node('lightTrace', 'graphics:HarmonicLissajous', (750, 420))
    p.wire('chord', 'packed', 'lightTrace', 'harmonics')
    p.bind('lightTrace', 'camera', 'yaw', 'globals.geometry.yaw')
    p.display('lineStudy'); p.display('lightTrace', 'out')
    p.node('geometryUpload', 'signal:GeometryUpload', (750, 740))
    p.node('geometryRender', 'graphics:GeometryRender', (1100, 740), view={'radius': 1.5})
    p.wire('trace', 'geometry', 'geometryUpload', 'input')
    p.wire('geometryUpload', 'primitives', 'geometryRender', 'primitives')
    p.display('geometryRender', 'out')
    yield p.save()

    p = Patch('02-plate-mode-walk', 'Plate mode walk')
    p.harmonic('chordA', '1, 5/4, 3/2', '1, 5/4, 3/2', (0, 0), False)
    p.harmonic('chordB', '1, 7/5, 8/5, 11/6', '1, 7/5, 8/5, 11/6', (0, 380), False)
    p.harmonic('chord', '1, 5/4, 3/2', '1, 7/5, 8/5, 11/6', (0, 740))
    p.node('modeWalk', 'signal:HarmonicModes', (380, 150))
    p.wire('chordA', 'harmonic', 'modeWalk', 'input'); p.wire('chordB', 'harmonic', 'modeWalk', 'target'); p.wire('slow', 'out', 'modeWalk', 'mix')
    p.node('plateLight', 'graphics:HarmonicChladni', (750, 180))
    p.wire('modeWalk', 'modes', 'plateLight', 'modes'); p.wire('chord', 'packed', 'plateLight', 'harmonics')
    p.control('approach', 0.0, 0, 1); p.control('symmetry', .0, 0, 1); p.control('lineWidth', .045, .008, .15)
    for name in ('approach', 'symmetry'): p.bind('plateLight', 'field', name, 'globals.geometry.'+name)
    p.ink('plateLight'); p.bind('ink', 'ink', 'width', 'globals.geometry.lineWidth')
    p.geometry('integerPlate', 'plate', 'chordA', (400, 600), geometry={'resolution': 128})
    p.view('plateStudy', 'integerPlate', 'Integer endpoint A', 'Play shows the GPU mode walk. This plate keeps its integer boundary modes.', harmonic='chordA', pos=(760, 620), field={'style': 'nodal'})
    p.display('ink', 'out'); p.display('plateStudy')
    yield p.save()

    p = Patch('03-between-media', 'Between media')
    p.harmonic()
    p.control('approach', .45, 0, 1)
    p.control('period', .7, .3, 1.4)
    p.control('style', 'nodal', kind='dropdown', options=['nodal', 'signed', 'magnitude', 'contours'])
    p.geometry('circle', 'circular_plate', geometry={'resolution': 128})
    p.geometry('openWaves', 'quasicrystal', pos=(380, 500), geometry={'resolution': 128})
    p.bind('openWaves', 'field', 'period', 'globals.geometry.period')
    p.node('between', 'signal:GeometryBlend', (760, 100), blend={'space': 'image'})
    p.wire('circle', 'geometry', 'between', 'a'); p.wire('openWaves', 'geometry', 'between', 'b')
    p.bind('between', 'blend', 'mix', 'globals.geometry.approach')
    p.view('mediaStudy', 'between', 'Between media', 'Tuning moves inside each medium. Approach blends their normalized images.', pos=(1110, 0), field={'style': 'nodal'})
    p.bind('mediaStudy', 'field', 'style', 'globals.geometry.style')
    p.ink('mediaStudy', 'field', upload=True)
    p.bind('ink', 'ink', 'style', 'globals.geometry.style')
    p.display('mediaStudy'); p.display('ink', 'out')
    yield p.save()

    p = Patch('04-sand-and-memory', 'Sand and memory')
    p.harmonic(a='1, 5/3, 7/4', b='1, 4/3, 11/6')
    p.control('affinity', 1.0, -2, 2); p.control('temperature', .035, .003, .15)
    p.control('flowMix', .15, 0, 1); p.control('rate', 1.0, 0, 2); p.control('reset', False, kind='toggle')
    p.geometry('plate', 'circular_plate', geometry={'resolution': 128})
    p.node('sand', 'signal:HarmonicTransport', (750, 0))
    p.wire('plate', 'geometry', 'sand', 'input')
    for name in ('affinity', 'temperature'): p.bind('sand', 'transport', name, 'globals.geometry.'+name)
    p.view('sandStudy', 'sand', 'Sand and powder', 'Affinity crosses from nodal sand to antinodal powder. This is equilibrium density.', pos=(1110, 0), field={'style': 'magnitude'})
    p.node('tracer', 'signal:HarmonicTransport', (750, 500), transport={'method': 'tracer'})
    p.wire('plate', 'geometry', 'tracer', 'input'); p.bind('tracer', 'transport', 'mixing', 'globals.geometry.flowMix')
    p.node('memory', 'graphics:HarmonicFlow', (1110, 500), seed={'grain': 7.0, 'injection': .002})
    p.wire('tracer', 'flow', 'memory', 'flow')
    p.bind('memory', 'motion', 'rate', 'globals.geometry.rate'); p.bind('memory', 'seed', 'reset', 'globals.geometry.reset')
    p.ink('memory', name='carriedInk', ink={'style': 'magnitude'})
    p.display('sandStudy'); p.display('carriedInk', 'out')
    yield p.save()

    p = Patch('05-knot-to-knot', 'Knot to knot')
    p.harmonic('chordA', '1, 5/4, 3/2', '1, 5/4, 3/2', (0, 0), False)
    p.harmonic('chordB', '1, 7/5, 8/5', '1, 7/5, 8/5', (0, 450), False)
    p.control('yaw', .55, -3.14, 3.14); p.control('radius', .045, .01, .12)
    for tag, y in [('A', 0), ('B', 450)]:
        p.geometry('knot'+tag, 'knot', 'chord'+tag, (380, y), geometry={'points': 256}, surface={'tube_sides': 6})
        p.bind('knot'+tag, 'surface', 'tube_radius', 'globals.geometry.radius')
    p.node('crossing', 'signal:GeometryBlend', (760, 150))
    p.wire('knotA', 'geometry', 'crossing', 'a'); p.wire('knotB', 'geometry', 'crossing', 'b'); p.wire('slow', 'out', 'crossing', 'mix')
    p.view('knotStudy', 'crossing', 'Knot to knot', 'Fixed endpoint meshes share triangle indices. Intermediate meshes may intersect.', harmonic=None, pos=(1110, 100), common={'max_frequency': 5.0})
    p.bind('knotStudy', 'camera', 'yaw', 'globals.geometry.yaw')
    p.display('knotStudy')
    yield p.save()

    p = Patch('06-interval-garden', 'Interval garden')
    p.harmonic(a='1, 9/8, 5/4, 4/3, 3/2, 5/3, 15/8', b='1, 8/7, 7/6, 4/3, 3/2, 12/7, 7/4')
    p.control('form', 'chord_graph', kind='dropdown', options=['chord_graph', 'interval_graph', 'tuning_circle', 'times_table', 'ifs', 'subharmonic_tree', 'recursive_polygon', 'pitch_lattice'])
    p.control('color', 'tonotopic', kind='dropdown', options=['tonotopic', 'harmonic', 'consonance', 'spectral', 'physical'])
    p.geometry('garden', 'chord_graph', geometry={'points': 1600}, structure={'depth': 3})
    p.bind('garden', 'geometry', 'method', 'globals.geometry.form')
    p.node('palette', 'signal:BioColors', (380, 500), color={'source': 'tuning', 'method': 'tonotopic', 'fund': 110.0})
    p.wire('chord', 'tuning', 'palette', 'input'); p.bind('palette', 'color', 'method', 'globals.geometry.color')
    p.node('rhythm', 'signal:EuclidRhythm', (760, 500)); p.wire('chord', 'tuning', 'rhythm', 'input')
    p.node('metrics', 'signal:GeometryMetrics', (760, 850)); p.wire('garden', 'geometry', 'metrics', 'input')
    p.view('gardenStudy', 'garden', 'Interval garden', 'The same scale draws a graph, grows a structure, colors a palette, and makes rhythms.')
    p.wire('palette', 'rgb', 'gardenStudy', 'palette'); p.wire('metrics', 'metrics', 'gardenStudy', 'metrics')
    p.display('gardenStudy'); p.display('rhythm', 'patterns')
    yield p.save()

    p = Patch('07-peaks-to-worlds', 'Peaks to worlds')
    # The archive carries only this patch-specific signal fixture; bundle nodes stay shared.
    p.extra['workspace/nodes_signal/geometry_signal.py'] = (HERE/'geometry_signal.py').read_text()
    p.node('signal', 'signal:GeometrySignal', (-380, 480))
    p.control('signalShift', 0.0, 0, 1)
    p.bind('signal', 'signal', 'shift', 'globals.geometry.signalShift')
    p.node('spectrum', 'signal:HarmonicSpectrum', (0, 500), spectrum={'n_peaks': 4, 'precision': .5}, common={'max_frequency': 3.0})
    p.wire('signal', 'out', 'spectrum', 'input')
    p.harmonic(b='1, 9/8, 4/3, 3/2')
    p.nodes[p.names['chord']]['params']['source']['mode_a'] = 'peaks'
    p.wire('spectrum', 'peaks', 'chord', 'a'); p.wire('spectrum', 'peakValues', 'chord', 'ampsA')
    p.geometry('world', 'harmonic_field', geometry={'resolution': 128})
    p.control('form', 'harmonic_field', kind='dropdown', options=['harmonic_field', 'quasicrystal', 'wave_lattice', 'acoustic', 'faraday', 'trace_3d'])
    p.bind('world', 'geometry', 'method', 'globals.geometry.form')
    p.node('tuning', 'signal:Tuning', (380, 500)); p.wire('spectrum', 'peaks', 'tuning', 'input')
    p.node('reduced', 'signal:TuningReduction', (760, 500), mode={'n_steps': 4})
    p.wire('tuning', 'tuning', 'reduced', 'input'); p.wire('reduced', 'reduced', 'chord', 'b')
    p.node('intervals', 'signal:TuningMatrix', (1110, 500))
    p.wire('chord', 'tuning', 'intervals', 'input')
    p.node('palette', 'signal:BioColors', (380, 850), color={'source': 'tuning', 'method': 'spectral', 'fund': 110.0})
    p.wire('chord', 'tuning', 'palette', 'input')
    p.view('peakStudy', 'world', 'Peaks to worlds', 'A synthetic biosignal window becomes harmonic peaks, then a shared geometric frame.')
    p.wire('palette', 'rgb', 'peakStudy', 'palette')
    p.display('peakStudy'); p.display('spectrum', 'matrix'); p.display('intervals', 'matrix')
    yield p.save()

    p = Patch('08-common-chord', 'A common chord')
    p.harmonic(a='1, 5/4, 3/2', b='1, 6/5, 3/2, 7/4')
    p.control('level', .15, 0, .3); p.control('transpose', 0.0, -2, 2)
    p.control('extension', 0.0, 0, 1)
    p.nodes[p.names['chord']]['params']['extension'] = {'kind': 'harmonics', 'count': 1}
    p.bind('chord', 'extension', 'amount', 'globals.geometry.extension')
    p.geometry('drawing', 'trace_3d')
    p.view('chordStudy', 'drawing', 'A common chord', 'The same component weights drive a drawing and eight continuous oscillator voices.')
    p.node('voices', 'signal:HarmonicVoices', (380, 500))
    p.wire('chord', 'harmonic', 'voices', 'input')
    for key in ('level', 'transpose'): p.bind('voices', 'sound', key, 'globals.geometry.'+key)
    for key in ('pitch', 'gain'):
        p.node(key+'Bus', 'audio:SignalIn', (750, 420 if key == 'pitch' else 780))
        p.wire('voices', key, key+'Bus', 'input')
    p.node('oscillator', 'audio:Osc', (1110, 420)); p.bind('oscillator', 'osc', 'pitch', reference='pitchBus.out')
    p.node('amplifier', 'audio:Gain', (1460, 420)); p.bind('amplifier', 'gain', 'gain', reference='gainBus.out')
    p.node('mixdown', 'audio:Mixdown', (1810, 420))
    p.wire('oscillator', 'out', 'amplifier', 'input'); p.wire('amplifier', 'out', 'mixdown', 'input')
    p.display('chordStudy')
    yield p.save()

    p = Patch('09-jade-resonance', 'Jade resonance')
    p.canvas = True
    p.harmonic('chordA', '1, 5/4, 3/2, 7/4', '1, 5/4, 3/2, 7/4', (0, 0), False)
    p.harmonic('chordB', '1, 6/5, 7/5, 9/5', '1, 6/5, 7/5, 9/5', (0, 380), False)
    p.node('modeWalk', 'signal:HarmonicModes', (380, 100))
    p.wire('chordA', 'harmonic', 'modeWalk', 'input')
    p.wire('chordB', 'harmonic', 'modeWalk', 'target')
    p.wire('slow', 'out', 'modeWalk', 'mix')
    p.node('plate', 'graphics:HarmonicChladni', (750, 100),
           field={'symmetry': .82}, common={'width': 512, 'height': 512})
    p.wire('modeWalk', 'modes', 'plate', 'modes')
    p.node('jade', 'graphics:HarmonicRelief', (1120, 100),
           common={'width': 512, 'height': 512})
    p.wire('plate', 'out', 'jade', 'input')
    for name, value, param in [('textureA', 'sand', 'texture_a'), ('textureB', 'lichen', 'texture_b')]:
        p.control(name, value, kind='dropdown', options=RELIEF_TEXTURES)
        p.bind('jade', 'material', param, 'globals.geometry.'+name)
    p.control('textureAuto', True, kind='toggle')
    p.control('textureMix', 0.0, 0, 1)
    p.node('materialMotion', 'signal:LFO', (750, 520), lfo={'frequency': .012, 'phase': .75}, common={'max_frequency': 20.0})
    p.bind('materialMotion', 'lfo', 'amplitude', '0.5 if globals.geometry.textureAuto else 0.0')
    p.bind('materialMotion', 'lfo', 'offset', '0.5 if globals.geometry.textureAuto else globals.geometry.textureMix')
    p.bind('jade', 'material', 'texture_mix', reference='materialMotion.out')
    for name, value, lo, hi, node, group, param in [
        ('symmetry', .82, 0, 1, 'plate', 'field', 'symmetry'),
        ('relief', .22, 0, .45, 'jade', 'form', 'depth'),
        ('goldSeam', .022, .008, .15, 'jade', 'form', 'seam'),
        ('engraving', .55, 0, 1, 'jade', 'form', 'engraving'),
        ('patina', .7, 0, 1, 'jade', 'material', 'patina'),
        ('roughness', .38, .12, .8, 'jade', 'material', 'roughness'),
        ('textureScale', 24.0, 4, 64, 'jade', 'material', 'texture_scale'),
        ('textureDepth', .75, 0, 1, 'jade', 'material', 'texture_depth'),
        ('density', .65, 0, 1, 'jade', 'material', 'density'),
        ('light', -.75, -3.14, 3.14, 'jade', 'light', 'azimuth'),
        ('tilt', .25, 0, .85, 'jade', 'camera', 'tilt'),
    ]:
        p.control(name, value, lo, hi)
        p.bind(node, group, param, 'globals.geometry.'+name)
    p.display('jade', 'out')
    yield p.save()

    p = Patch('10-living-ratios', 'Living ratios', auto=False, mix=0.0)
    p.canvas = True
    p.control('ratios', '3/2, 5/4, 4/3, 7/4, 5/3, 9/8', kind='text')
    p.control('running', True, kind='toggle')
    p.control('stepSeconds', 4.0, .25, 10, step=.05)
    p.control('glide', 1.0, 0, 1)
    p.control('direction', 'ping-pong', kind='dropdown', options=['forward', 'ping-pong'])
    p.node('ratios', 'signal:RatioSequence', (0, 0), chord={'state': 'anchor chord'})
    for name, param in [('ratios', 'ratios'), ('running', 'running'), ('stepSeconds', 'seconds'), ('glide', 'glide'), ('direction', 'direction')]:
        p.bind('ratios', 'sequence', param, 'globals.geometry.'+name)
    p.harmonic('chord', '1, 9/8, 2', '1, 9/8, 2', (380, 0), False)
    p.wire('ratios', 'tuning', 'chord', 'a')
    p.node('modes', 'signal:HarmonicModes', (750, 0),
           modes={'mapping': 'chord pairs', 'interpolation': 'fields', 'max_mode': 24})
    p.wire('ratios', 'transition', 'modes', 'transition')
    p.node('field', 'graphics:HarmonicChladni', (1120, 0),
           field={'approach': 0.0, 'symmetry': 1.0, 'directions': 7, 'period': .5, 'output': 'nodal'}, common={'width': 512, 'height': 512})
    p.wire('chord', 'packed', 'field', 'harmonics'); p.wire('modes', 'modes', 'field', 'modes')
    p.node('organism', 'graphics:HarmonicRelief', (1500, 0),
           material={'texture_a': 'sand', 'texture_b': 'spores'}, form={'input_kind': 'density'}, camera={'tilt': 0.0, 'turn': 0.0},
           common={'width': 512, 'height': 512})
    p.wire('field', 'out', 'organism', 'input')
    p.bind('organism', 'material', 'texture_mix', reference='slow.out')
    for name, value, param in [('textureA', 'sand', 'texture_a'), ('textureB', 'spores', 'texture_b')]:
        p.control(name, value, kind='dropdown', options=RELIEF_TEXTURES)
        p.bind('organism', 'material', param, 'globals.geometry.'+name)
    for name, value, lo, hi, node, group, param in [
        ('density', .9, 0, 1, 'organism', 'material', 'density'),
        ('grainSize', 40.0, 4, 64, 'organism', 'material', 'texture_scale'),
        ('grainHeight', .75, 0, 1, 'organism', 'material', 'texture_depth'),
        ('bandWidth', .05, .008, .15, 'field', 'field', 'sigma'),
        ('approach', 0.0, 0, 1, 'field', 'field', 'approach'),
        ('period', .5, .2, 1.0, 'field', 'field', 'period'),
    ]:
        p.control(name, value, lo, hi, step={'grainSize': .1, 'bandWidth': .0001}.get(name))
        p.bind(node, group, param, 'globals.geometry.'+name)
    p.node('ratioTrace', 'signal:Buffer', (380, 480), buffer={'size': 600})
    p.wire('ratios', 'ratio', 'ratioTrace', 'input')
    p.monitor = ('ratioTrace', 'out', 'line')
    p.display('organism', 'out'); p.display('ratioTrace', 'out', 'line'); p.display('ratios', 'label', 'string')
    p.ink('field', name='nodalLines')
    p.nodes[p.names['nodalLines']]['params'].setdefault('ink', {})['style'] = 'density'
    p.display('nodalLines', 'out')
    p.display('modes', 'mapping', 'string')
    p.control('anchorChord', '1, 9/8, 2', kind='text')
    p.bind('ratios', 'chord', 'anchors', 'globals.geometry.anchorChord')
    p.control('squareSymmetry', 'd4_max', kind='dropdown', options=['none', 'd4_max', 'd4_sum'])
    p.bind('field', 'field', 'density_symmetry', 'globals.geometry.squareSymmetry')
    for name, value, param, options in [
        ('mapping', 'chord pairs', 'mapping', ['per ratio', 'chord pairs']),
        ('pairSet', 'auto', 'pairs', ['auto', 'all', 'root', 'adjacent']),
        ('motion', 'fields', 'interpolation', ['coordinates', 'fields']),
    ]:
        p.control(name, value, kind='dropdown', options=options)
        p.bind('modes', 'modes', param, 'globals.geometry.'+name)
    yield p.save()


if __name__ == '__main__':
    manifest = list(recipes())
    (HERE/'recipes.json').write_text(json.dumps(manifest, indent=2)+'\n')
    print(f'Built {len(manifest)} patches in {HERE}')
