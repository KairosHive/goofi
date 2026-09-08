/* goofi
{
  "doc": "Inception: living iridescent thought-forms unfolding from within.\nInspired by Besant and Leadbeater's Thought-Forms (1901), especially the meditation and music plates. Nested luminous membranes continually bloom, curl and dissolve. Stateful reaction-diffusion bends and illuminates the membranes, with vortices, orbital drift and peristaltic waves. Speed controls metabolism; morph changes the organism; intricacy adds fine ribs; iridescence controls interference colour. Every artistic parameter has its own GPU sine LFO: lfo_rates are Hz and lfo_depths are peak excursions in parameter units; zero depth stops that modulation. Base controls remain editable. Motion speed uses an integrated phase to avoid acceleration from multiplying a changing speed by elapsed time.",
  "tags": [
    "image",
    "generator",
    "simulation"
  ],
  "state": [
    "chem"
  ],
  "params": [
    {
      "group": "inception",
      "name": "speed",
      "kind": "float",
      "default": 0.24,
      "min": 0,
      "max": 2
    },
    {
      "group": "inception",
      "name": "morph",
      "kind": "float",
      "default": 0.5,
      "min": 0,
      "max": 4
    },
    {
      "group": "inception",
      "name": "intricacy",
      "kind": "float",
      "default": 0.65,
      "min": 0,
      "max": 1
    },
    {
      "group": "inception",
      "name": "iridescence",
      "kind": "float",
      "default": 0.8,
      "min": 0,
      "max": 1
    },
    {
      "group": "inception",
      "name": "bloom",
      "kind": "float",
      "default": 1.1,
      "min": 0.1,
      "max": 3
    },
    {
      "group": "inception",
      "name": "zoom",
      "kind": "float",
      "default": 1,
      "min": 0.4,
      "max": 2.5
    },
    {
      "group": "form",
      "name": "petals",
      "kind": "float",
      "default": 0.5,
      "min": 0,
      "max": 1
    },
    {
      "group": "form",
      "name": "fold",
      "kind": "float",
      "default": 0.115,
      "min": 0,
      "max": 0.3
    },
    {
      "group": "form",
      "name": "warp",
      "kind": "float",
      "default": 0.035,
      "min": 0,
      "max": 0.16
    },
    {
      "group": "form",
      "name": "elongation",
      "kind": "float",
      "default": 1,
      "min": 0.55,
      "max": 1.65
    },
    {
      "group": "form",
      "name": "twist",
      "kind": "float",
      "default": 0.055,
      "min": -0.2,
      "max": 0.2
    },
    {
      "group": "form",
      "name": "spacing",
      "kind": "float",
      "default": 1,
      "min": 0.4,
      "max": 2.4
    },
    {
      "group": "form",
      "name": "rotation",
      "kind": "float",
      "default": 0,
      "min": -3.14,
      "max": 3.14
    },
    {
      "group": "surface",
      "name": "membrane",
      "kind": "float",
      "default": 0.065,
      "min": 0.015,
      "max": 0.16
    },
    {
      "group": "surface",
      "name": "halo",
      "kind": "float",
      "default": 0.025,
      "min": 0.005,
      "max": 0.09
    },
    {
      "group": "surface",
      "name": "lace",
      "kind": "float",
      "default": 0.65,
      "min": 0,
      "max": 1
    },
    {
      "group": "surface",
      "name": "rib_density",
      "kind": "float",
      "default": 1,
      "min": 0.3,
      "max": 2
    },
    {
      "group": "surface",
      "name": "transparency",
      "kind": "float",
      "default": 0.895,
      "min": 0.5,
      "max": 1
    },
    {
      "group": "color",
      "name": "hue",
      "kind": "float",
      "default": 0,
      "min": -1,
      "max": 1
    },
    {
      "group": "color",
      "name": "dispersion",
      "kind": "float",
      "default": 0.073,
      "min": 0,
      "max": 0.2
    },
    {
      "group": "color",
      "name": "core",
      "kind": "float",
      "default": 1,
      "min": 0,
      "max": 2
    },
    {
      "group": "color",
      "name": "aura",
      "kind": "float",
      "default": 1,
      "min": 0,
      "max": 3
    },
    {
      "group": "life",
      "name": "feed",
      "kind": "float",
      "default": 0.035,
      "min": 0.025,
      "max": 0.045
    },
    {
      "group": "life",
      "name": "kill",
      "kind": "float",
      "default": 0.06,
      "min": 0.054,
      "max": 0.066
    },
    {
      "group": "life",
      "name": "metabolism",
      "kind": "float",
      "default": 0.8,
      "min": 0.3,
      "max": 1
    },
    {
      "group": "life",
      "name": "emergence",
      "kind": "float",
      "default": 0.6,
      "min": 0,
      "max": 1
    },
    {
      "group": "motion",
      "name": "flow",
      "kind": "float",
      "default": 0.18,
      "min": 0,
      "max": 0.5
    },
    {
      "group": "motion",
      "name": "turbulence",
      "kind": "float",
      "default": 0.4,
      "min": 0,
      "max": 1
    },
    {
      "group": "motion",
      "name": "orbit",
      "kind": "float",
      "default": 0.08,
      "min": 0,
      "max": 0.2
    },
    {
      "group": "motion",
      "name": "peristalsis",
      "kind": "float",
      "default": 0.35,
      "min": 0,
      "max": 1
    },
    {
      "group": "lfo_rates",
      "name": "speed_hz",
      "kind": "float",
      "default": 0.0031,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "speed_depth",
      "kind": "float",
      "default": 0.09,
      "min": 0,
      "max": 1
    },
    {
      "group": "lfo_rates",
      "name": "morph_hz",
      "kind": "float",
      "default": 0.0037,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "morph_depth",
      "kind": "float",
      "default": 1.5,
      "min": 0,
      "max": 2
    },
    {
      "group": "lfo_rates",
      "name": "intricacy_hz",
      "kind": "float",
      "default": 0.0071,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "intricacy_depth",
      "kind": "float",
      "default": 0.3,
      "min": 0,
      "max": 0.5
    },
    {
      "group": "lfo_rates",
      "name": "iridescence_hz",
      "kind": "float",
      "default": 0.0043,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "iridescence_depth",
      "kind": "float",
      "default": 0.26,
      "min": 0,
      "max": 0.5
    },
    {
      "group": "lfo_rates",
      "name": "bloom_hz",
      "kind": "float",
      "default": 0.0053,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "bloom_depth",
      "kind": "float",
      "default": 0.4,
      "min": 0,
      "max": 1.45
    },
    {
      "group": "lfo_rates",
      "name": "zoom_hz",
      "kind": "float",
      "default": 0.0047,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "zoom_depth",
      "kind": "float",
      "default": 0.16,
      "min": 0,
      "max": 1.05
    },
    {
      "group": "lfo_rates",
      "name": "petals_hz",
      "kind": "float",
      "default": 0.0061,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "petals_depth",
      "kind": "float",
      "default": 0.47,
      "min": 0,
      "max": 0.5
    },
    {
      "group": "lfo_rates",
      "name": "fold_hz",
      "kind": "float",
      "default": 0.0083,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "fold_depth",
      "kind": "float",
      "default": 0.11,
      "min": 0,
      "max": 0.15
    },
    {
      "group": "lfo_rates",
      "name": "warp_hz",
      "kind": "float",
      "default": 0.0097,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "warp_depth",
      "kind": "float",
      "default": 0.06,
      "min": 0,
      "max": 0.08
    },
    {
      "group": "lfo_rates",
      "name": "elongation_hz",
      "kind": "float",
      "default": 0.0059,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "elongation_depth",
      "kind": "float",
      "default": 0.3,
      "min": 0,
      "max": 0.5499999999999999
    },
    {
      "group": "lfo_rates",
      "name": "twist_hz",
      "kind": "float",
      "default": 0.0041,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "twist_depth",
      "kind": "float",
      "default": 0.15,
      "min": 0,
      "max": 0.2
    },
    {
      "group": "lfo_rates",
      "name": "spacing_hz",
      "kind": "float",
      "default": 0.0033,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "spacing_depth",
      "kind": "float",
      "default": 0.65,
      "min": 0,
      "max": 1
    },
    {
      "group": "lfo_rates",
      "name": "rotation_hz",
      "kind": "float",
      "default": 0.0029,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "rotation_depth",
      "kind": "float",
      "default": 1.4,
      "min": 0,
      "max": 3.14
    },
    {
      "group": "lfo_rates",
      "name": "membrane_hz",
      "kind": "float",
      "default": 0.0067,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "membrane_depth",
      "kind": "float",
      "default": 0.045,
      "min": 0,
      "max": 0.07250000000000001
    },
    {
      "group": "lfo_rates",
      "name": "halo_hz",
      "kind": "float",
      "default": 0.0079,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "halo_depth",
      "kind": "float",
      "default": 0.022,
      "min": 0,
      "max": 0.042499999999999996
    },
    {
      "group": "lfo_rates",
      "name": "lace_hz",
      "kind": "float",
      "default": 0.0103,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "lace_depth",
      "kind": "float",
      "default": 0.4,
      "min": 0,
      "max": 0.5
    },
    {
      "group": "lfo_rates",
      "name": "rib_density_hz",
      "kind": "float",
      "default": 0.0051,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "rib_density_depth",
      "kind": "float",
      "default": 0.6,
      "min": 0,
      "max": 0.85
    },
    {
      "group": "lfo_rates",
      "name": "transparency_hz",
      "kind": "float",
      "default": 0.0089,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "transparency_depth",
      "kind": "float",
      "default": 0.12,
      "min": 0,
      "max": 0.25
    },
    {
      "group": "lfo_rates",
      "name": "hue_hz",
      "kind": "float",
      "default": 0.0039,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "hue_depth",
      "kind": "float",
      "default": 0.7,
      "min": 0,
      "max": 1
    },
    {
      "group": "lfo_rates",
      "name": "dispersion_hz",
      "kind": "float",
      "default": 0.0057,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "dispersion_depth",
      "kind": "float",
      "default": 0.07,
      "min": 0,
      "max": 0.1
    },
    {
      "group": "lfo_rates",
      "name": "core_hz",
      "kind": "float",
      "default": 0.0127,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "core_depth",
      "kind": "float",
      "default": 0.8,
      "min": 0,
      "max": 1
    },
    {
      "group": "lfo_rates",
      "name": "aura_hz",
      "kind": "float",
      "default": 0.0113,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "aura_depth",
      "kind": "float",
      "default": 1,
      "min": 0,
      "max": 1.5
    },
    {
      "group": "lfo_rates",
      "name": "feed_hz",
      "kind": "float",
      "default": 0.0023,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "feed_depth",
      "kind": "float",
      "default": 0.003,
      "min": 0,
      "max": 0.009999999999999998
    },
    {
      "group": "lfo_rates",
      "name": "kill_hz",
      "kind": "float",
      "default": 0.0019,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "kill_depth",
      "kind": "float",
      "default": 0.002,
      "min": 0,
      "max": 0.006000000000000002
    },
    {
      "group": "lfo_rates",
      "name": "metabolism_hz",
      "kind": "float",
      "default": 0.0063,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "metabolism_depth",
      "kind": "float",
      "default": 0.15,
      "min": 0,
      "max": 0.35
    },
    {
      "group": "lfo_rates",
      "name": "emergence_hz",
      "kind": "float",
      "default": 0.0049,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "emergence_depth",
      "kind": "float",
      "default": 0.25,
      "min": 0,
      "max": 0.5
    },
    {
      "group": "lfo_rates",
      "name": "flow_hz",
      "kind": "float",
      "default": 0.0073,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "flow_depth",
      "kind": "float",
      "default": 0.14,
      "min": 0,
      "max": 0.25
    },
    {
      "group": "lfo_rates",
      "name": "turbulence_hz",
      "kind": "float",
      "default": 0.0081,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "turbulence_depth",
      "kind": "float",
      "default": 0.3,
      "min": 0,
      "max": 0.5
    },
    {
      "group": "lfo_rates",
      "name": "orbit_hz",
      "kind": "float",
      "default": 0.0036,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "orbit_depth",
      "kind": "float",
      "default": 0.065,
      "min": 0,
      "max": 0.1
    },
    {
      "group": "lfo_rates",
      "name": "peristalsis_hz",
      "kind": "float",
      "default": 0.0091,
      "min": 0.0001,
      "max": 0.1
    },
    {
      "group": "lfo_depths",
      "name": "peristalsis_depth",
      "kind": "float",
      "default": 0.3,
      "min": 0,
      "max": 0.5
    }
  ]
}
*/

// Stateful Gray-Scott chemistry on a 192-square logical grid, plus nacre sheets.
// Local reaction, diffusion and weak advection retain history across frames.
const TAU: f32 = 6.28318530718;
const GRID: f32 = 192.0;

struct Modulated {
    speed: f32,
    morph: f32,
    intricacy: f32,
    iridescence: f32,
    bloom: f32,
    zoom: f32,
    petals: f32,
    fold: f32,
    warp: f32,
    elongation: f32,
    twist: f32,
    spacing: f32,
    rotation: f32,
    membrane: f32,
    halo: f32,
    lace: f32,
    rib_density: f32,
    transparency: f32,
    hue: f32,
    dispersion: f32,
    core: f32,
    aura: f32,
    feed: f32,
    kill: f32,
    metabolism: f32,
    emergence: f32,
    flow: f32,
    turbulence: f32,
    orbit: f32,
    peristalsis: f32,
}
fn modulated() -> Modulated {
    var c: Modulated;
    c.speed = p.speed + min(p.speed_depth,max(0.0,min(p.speed-0.0,2.0-p.speed)))*sin(TAU*p.speed_hz*time+0.0);
    c.morph = p.morph + min(p.morph_depth,max(0.0,min(p.morph-0.0,4.0-p.morph)))*sin(TAU*p.morph_hz*time+2.399963);
    c.intricacy = p.intricacy + min(p.intricacy_depth,max(0.0,min(p.intricacy-0.0,1.0-p.intricacy)))*sin(TAU*p.intricacy_hz*time+4.799926);
    c.iridescence = p.iridescence + min(p.iridescence_depth,max(0.0,min(p.iridescence-0.0,1.0-p.iridescence)))*sin(TAU*p.iridescence_hz*time+0.916704);
    c.bloom = p.bloom + min(p.bloom_depth,max(0.0,min(p.bloom-0.1,3.0-p.bloom)))*sin(TAU*p.bloom_hz*time+3.316667);
    c.zoom = p.zoom + min(p.zoom_depth,max(0.0,min(p.zoom-0.4,2.5-p.zoom)))*sin(TAU*p.zoom_hz*time+5.71663);
    c.petals = p.petals + min(p.petals_depth,max(0.0,min(p.petals-0.0,1.0-p.petals)))*sin(TAU*p.petals_hz*time+1.833408);
    c.fold = p.fold + min(p.fold_depth,max(0.0,min(p.fold-0.0,0.3-p.fold)))*sin(TAU*p.fold_hz*time+4.233371);
    c.warp = p.warp + min(p.warp_depth,max(0.0,min(p.warp-0.0,0.16-p.warp)))*sin(TAU*p.warp_hz*time+0.350149);
    c.elongation = p.elongation + min(p.elongation_depth,max(0.0,min(p.elongation-0.55,1.65-p.elongation)))*sin(TAU*p.elongation_hz*time+2.750113);
    c.twist = p.twist + min(p.twist_depth,max(0.0,min(p.twist + 0.2,0.2-p.twist)))*sin(TAU*p.twist_hz*time+5.150076);
    c.spacing = p.spacing + min(p.spacing_depth,max(0.0,min(p.spacing-0.4,2.4-p.spacing)))*sin(TAU*p.spacing_hz*time+1.266854);
    c.rotation = p.rotation + min(p.rotation_depth,max(0.0,min(p.rotation + 3.14,3.14-p.rotation)))*sin(TAU*p.rotation_hz*time+3.666817);
    c.membrane = p.membrane + min(p.membrane_depth,max(0.0,min(p.membrane-0.015,0.16-p.membrane)))*sin(TAU*p.membrane_hz*time+6.06678);
    c.halo = p.halo + min(p.halo_depth,max(0.0,min(p.halo-0.005,0.09-p.halo)))*sin(TAU*p.halo_hz*time+2.183558);
    c.lace = p.lace + min(p.lace_depth,max(0.0,min(p.lace-0.0,1.0-p.lace)))*sin(TAU*p.lace_hz*time+4.583521);
    c.rib_density = p.rib_density + min(p.rib_density_depth,max(0.0,min(p.rib_density-0.3,2.0-p.rib_density)))*sin(TAU*p.rib_density_hz*time+0.700299);
    c.transparency = p.transparency + min(p.transparency_depth,max(0.0,min(p.transparency-0.5,1.0-p.transparency)))*sin(TAU*p.transparency_hz*time+3.100262);
    c.hue = p.hue + min(p.hue_depth,max(0.0,min(p.hue + 1.0,1.0-p.hue)))*sin(TAU*p.hue_hz*time+5.500225);
    c.dispersion = p.dispersion + min(p.dispersion_depth,max(0.0,min(p.dispersion-0.0,0.2-p.dispersion)))*sin(TAU*p.dispersion_hz*time+1.617003);
    c.core = p.core + min(p.core_depth,max(0.0,min(p.core-0.0,2.0-p.core)))*sin(TAU*p.core_hz*time+4.016966);
    c.aura = p.aura + min(p.aura_depth,max(0.0,min(p.aura-0.0,3.0-p.aura)))*sin(TAU*p.aura_hz*time+0.133744);
    c.feed = p.feed + min(p.feed_depth,max(0.0,min(p.feed-0.025,0.045-p.feed)))*sin(TAU*p.feed_hz*time+2.533707);
    c.kill = p.kill + min(p.kill_depth,max(0.0,min(p.kill-0.054,0.066-p.kill)))*sin(TAU*p.kill_hz*time+4.93367);
    c.metabolism = p.metabolism + min(p.metabolism_depth,max(0.0,min(p.metabolism-0.3,1.0-p.metabolism)))*sin(TAU*p.metabolism_hz*time+1.050448);
    c.emergence = p.emergence + min(p.emergence_depth,max(0.0,min(p.emergence-0.0,1.0-p.emergence)))*sin(TAU*p.emergence_hz*time+3.450411);
    c.flow = p.flow + min(p.flow_depth,max(0.0,min(p.flow-0.0,0.5-p.flow)))*sin(TAU*p.flow_hz*time+5.850374);
    c.turbulence = p.turbulence + min(p.turbulence_depth,max(0.0,min(p.turbulence-0.0,1.0-p.turbulence)))*sin(TAU*p.turbulence_hz*time+1.967152);
    c.orbit = p.orbit + min(p.orbit_depth,max(0.0,min(p.orbit-0.0,0.2-p.orbit)))*sin(TAU*p.orbit_hz*time+4.367115);
    c.peristalsis = p.peristalsis + min(p.peristalsis_depth,max(0.0,min(p.peristalsis-0.0,1.0-p.peristalsis)))*sin(TAU*p.peristalsis_hz*time+0.483893);
    return c;
}

fn cell(at: vec2i) -> vec2f {
    let wrapped = ((at % vec2i(192)) + vec2i(192)) % vec2i(192);
    let pixel = vec2i((vec2f(wrapped)+0.5)*resolution/GRID);
    return textureLoad(chem,pixel,0).rg;
}
fn chemicals(at: vec2f) -> vec2f {
    let b = vec2i(floor(at));
    let f = fract(at);
    return mix(mix(cell(b),cell(b+vec2i(1,0)),f.x),
        mix(cell(b+vec2i(0,1)),cell(b+vec2i(1,1)),f.x),f.y);
}
fn seedhash(at: vec2i) -> f32 {
    let q = vec2u(at+65536);
    var h = q.x*1597334673u ^ q.y*3812015801u;
    h = (h ^ (h >> 16u))*2246822519u;
    return f32(h ^ (h >> 13u))/4294967296.0;
}
fn next_chem(uv: vec2f) -> vec4f {
    let c = modulated();
    let at = vec2i(floor(uv*GRID));
    if frame == 0u {
        let blot = step(0.88,seedhash(at/vec2i(4)));
        return vec4f(1.0-0.5*blot,0.55*blot,0.0,1.0);
    }
    let s = cell(at);
    let east = cell(at+vec2i(1,0));
    let west = cell(at-vec2i(1,0));
    let north = cell(at+vec2i(0,1));
    let south = cell(at-vec2i(0,1));
    let lap = -s+0.2*(east+west+north+south)+0.05*(
        cell(at+vec2i(1,1))+cell(at+vec2i(-1,1))+
        cell(at+vec2i(1,-1))+cell(at-vec2i(1,1)));
    let xy = (vec2f(at)+0.5)/GRID*TAU;
    let drift = c.flow*vec2f(sin(xy.y*2.0+time*0.023),sin(xy.x*2.0-time*0.019));
    let curl = c.turbulence*vec2f(north.y-south.y,west.y-east.y)*0.6;
    let carried = chemicals(vec2f(at)-drift-curl);
    let reaction = s.x*s.y*s.y;
    let next = clamp(carried+c.metabolism*(vec2f(0.85,0.42)*lap+
        vec2f(-reaction+c.feed*(1.0-s.x),reaction-(c.feed+c.kill)*s.y)),vec2f(0.0),vec2f(1.0));
    return vec4f(next,0.0,1.0);
}
fn rotate(q: vec2f, a: f32) -> vec2f {
    return vec2f(cos(a)*q.x-sin(a)*q.y,sin(a)*q.x+cos(a)*q.y);
}
fn pearl(x: f32) -> vec3f {
    return 0.52 + 0.48*cos(TAU*(vec3f(0.02,0.35,0.67)+x));
}
fn shade(uv: vec2f) -> vec4f {
    let c = modulated();
    let omega = TAU*max(p.speed_hz,0.0001);
    let speedamp = min(p.speed_depth,max(0.0,min(p.speed,2.0-p.speed)));
    let t = time*p.speed+speedamp*(1.0-cos(omega*time))/omega;
    let life = chemicals(uv*GRID-0.5).y;
    let grad = vec2f(chemicals(uv*GRID+vec2f(1.0,0.0)).y-chemicals(uv*GRID-vec2f(1.0,0.0)).y,
        chemicals(uv*GRID+vec2f(0.0,1.0)).y-chemicals(uv*GRID-vec2f(0.0,1.0)).y);
    var q = (uv-0.5)*vec2f(resolution.x/max(resolution.y,1.0),1.0)*2.6/max(c.zoom,0.1);
    q += c.emergence*0.3*vec2f(grad.y,-grad.x);
    let r0 = length(q);
    var col = vec3f(0.003,0.005,0.014);
    col += c.aura*vec3f(0.032,0.016,0.065)*exp(-2.8*r0*r0);
    let aa = 2.6/max(resolution.y,1.0)/max(c.zoom,0.1);
    // Back to front, like transparent nacre sheets around a luminous seed.
    for(var j=0; j<18; j++) {
        let k = f32(j);
        let depth = pow(k/17.0,c.spacing);
        let phase = t*0.7+k*0.41+c.morph*2.0;
        let orbiting = c.orbit*vec2f(sin(t*0.37+k*0.6),cos(t*0.29+k*0.47));
        var v = rotate(q+orbiting,c.rotation+t*0.055+0.14*sin(phase*0.37)+k*c.twist);
        // Composed shears produce rolling eddies rather than rigid rotation.
        v.x += c.turbulence*0.07*sin(v.y*5.0+sin(t*0.31+k*0.2));
        v.y += c.turbulence*0.07*sin(v.x*6.0-cos(t*0.27+k*0.3));
        v += c.warp*vec2f(sin(phase+v.y*3.0),cos(phase*0.8+v.x*3.0));
        v.y *= c.elongation*(1.0+0.14*sin(phase*0.61));
        let r = length(v);
        let a = atan2(v.y,v.x);
        let lobes = mix(cos(3.0*a+phase*0.33),cos(7.0*a-phase*0.21),c.petals);
        let folds = sin(5.0*a+2.3*sin(3.0*a-phase*0.4)+phase);
        let swallow = c.peristalsis*0.075*sin(9.0*r-1.3*t+2.0*sin(3.0*a+phase*0.2));
        let radius = (0.94-depth*0.73)*(1.0+0.13*lobes+c.fold*folds+0.035*sin(11.0*a+phase)+swallow+c.emergence*0.18*(life-0.2));
        let d = r-radius;
        let edge = exp(-abs(d)/max(aa*1.2,0.0025));
        let halo = exp(-abs(d)/max(c.halo,0.001));
        let inside = 1.0-smoothstep(-aa,aa,d);
        let band = exp(-abs(d+0.035)/max(c.membrane,0.001))*inside;
        let ribphase = (35.0+65.0*c.intricacy)*c.rib_density*r + 5.0*sin(6.0*a+phase)+3.0*sin(9.0*r-phase);
        let ribs = pow(0.5+0.5*cos(ribphase),14.0);
        let lace = pow(0.5+0.5*sin(48.0*a+12.0*r+phase),18.0);
        let flowing = 0.5+0.5*sin(12.0*r-2.0*t+3.0*sin(4.0*a+phase));
        let hue = c.hue+k*c.dispersion+0.1*t+0.16*folds+c.iridescence*(0.38*sin(a+phase)+2.4*d)+c.emergence*life*0.8;
        let tint = mix(vec3f(0.72,0.34,0.58),pearl(hue),c.iridescence);
        let opacity = band*(1.0-c.transparency);
        col *= 1.0-opacity;
        col += tint*(0.22*edge+0.04*halo+band*(0.045+0.13*ribs*c.intricacy+0.08*lace*c.lace))*(0.65+0.35*flowing);
        col += vec3f(1.0,0.81,0.53)*edge*0.035;
        col += pearl(hue+0.2)*band*c.emergence*(0.3*life+0.55*length(grad));
    }
    let heart = exp(-r0*r0/0.006)*(0.8+0.2*sin(t*1.7));
    col += c.core*vec3f(0.72,0.39,0.19)*heart;
    col = vec3f(1.0)-exp(-col*c.bloom*1.6);
    return vec4f(pow(max(col,vec3f(0.0)),vec3f(0.88)),1.0);
}
