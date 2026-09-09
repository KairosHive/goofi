import goofi
import numpy as np
from biotuner.harmonic_geometry import GeometryData
from biotuner.harmonic_geometry.media.eigenmode.rigid_plate import chord_to_int_modes, chladni_field_pairwise, chladni_nodal_density


class ChladniReference(goofi.Node):
    OUTPUTS = {'fields': goofi.DataType.ARRAY, 'pairs': goofi.DataType.ARRAY, 'density': goofi.DataType.ARRAY}
    PARAMS = {'common': {'autotrigger': goofi.BoolParam(True), 'max_frequency': goofi.FloatParam(1.0, 0.0, 10.0)}}

    def process(self):
        fields, pairs = [], []
        for ratio in [3/2, 5/4, 4/3, 7/4, 5/3, 9/8]:
            field = chladni_field_pairwise(chord_to_int_modes([1, ratio, 2]), resolution=64,
                                          max_mode=24, pair_subset='auto')
            fields.append(field.coordinates)
            pairs.append(field.parameters['pairs'])
        densities = []
        for step in range(6):
            phases = []
            for phase in [0, .25, .5, .75, .999]:
                t = phase*phase*(3-2*phase)
                geom = GeometryData('field_2d', (1-t)*fields[step]+t*fields[(step+1)%6],
                                    parameters={'symmetry': 'd4_max'})
                phases.append(chladni_nodal_density(geom, sigma=.05).coordinates)
            densities.append(phases)
        return {'fields': np.asarray(fields, dtype=np.float32), 'pairs': np.asarray(pairs, dtype=np.float32),
                'density': np.asarray(densities, dtype=np.float32)}
