/* goofi
{
  "doc": "Six nested scales of local agreement and distant inhibition, drawn in ink.\nA visual metaphor for thought, not a brain model. Equal rules at geometrically spaced radii create competing domains. Activity advects through its own memory gradient; memory slowly follows. Finite resolution bounds the scale range.\nThe field and its colouring are one node here: the `structure` and `motion` groups shape what happens, and `tone`, `palette` and `texture` decide how it is drawn.",
  "tags": [
    "generator",
    "simulation",
    "image"
  ],
  "state": [
    "field"
  ],
  "params": [
    {
      "group": "thought",
      "name": "pace",
      "kind": "float",
      "default": 0.65,
      "min": 0.0,
      "max": 2.0
    },
    {
      "group": "thought",
      "name": "flow",
      "kind": "float",
      "default": 1.2,
      "min": 0.0,
      "max": 3.0
    },
    {
      "group": "thought",
      "name": "memory",
      "kind": "float",
      "default": 0.975,
      "min": 0.8,
      "max": 0.999
    },
    {
      "group": "thought",
      "name": "seed",
      "kind": "float",
      "default": 17.0,
      "min": 0.0,
      "max": 1000.0
    },
    {
      "group": "structure",
      "name": "size",
      "kind": "float",
      "default": 1.0,
      "min": 1.0,
      "max": 8.0,
      "doc": "Base interaction radius in pixels; larger values grow broader structures."
    },
    {
      "group": "structure",
      "name": "scales",
      "kind": "int",
      "default": 6,
      "min": 1,
      "max": 8,
      "doc": "Number of nested interaction scales."
    },
    {
      "group": "structure",
      "name": "spacing",
      "kind": "float",
      "default": 2.0,
      "min": 1.2,
      "max": 3.0,
      "doc": "Ratio between successive interaction radii."
    },
    {
      "group": "structure",
      "name": "inhibition",
      "kind": "float",
      "default": 2.0,
      "min": 1.1,
      "max": 4.0,
      "doc": "Distance of inhibition relative to local reinforcement."
    },
    {
      "group": "structure",
      "name": "reinforcement",
      "kind": "float",
      "default": 0.022,
      "min": 0.0,
      "max": 0.08,
      "doc": "Strength of local pattern amplification."
    },
    {
      "group": "structure",
      "name": "sensitivity",
      "kind": "float",
      "default": 0.025,
      "min": 0.001,
      "max": 0.15,
      "doc": "Lower values amplify weaker differences."
    },
    {
      "group": "structure",
      "name": "persistence",
      "kind": "float",
      "default": 0.006,
      "min": -0.03,
      "max": 0.03,
      "doc": "Positive values sustain departures from memory; negative values pull back."
    },
    {
      "group": "structure",
      "name": "balance",
      "kind": "float",
      "default": 0.0015,
      "min": 0.0,
      "max": 0.03,
      "doc": "Restoring force toward the middle activity level."
    },
    {
      "group": "motion",
      "name": "curl",
      "kind": "float",
      "default": 18.0,
      "min": 0.0,
      "max": 60.0,
      "doc": "Circulation driven by the memory gradient."
    },
    {
      "group": "motion",
      "name": "drift",
      "kind": "float",
      "default": 1.0,
      "min": 0.0,
      "max": 4.0,
      "doc": "Strength of the broad background current."
    },
    {
      "group": "motion",
      "name": "driftSize",
      "kind": "float",
      "default": 1.0,
      "min": 0.25,
      "max": 4.0,
      "doc": "Spatial size of background currents."
    },
    {
      "group": "motion",
      "name": "direction",
      "kind": "float",
      "default": 0.0,
      "min": -180.0,
      "max": 180.0,
      "doc": "Rotate circulation into convergence or divergence, in degrees."
    },
    {
      "group": "motion",
      "name": "novelty",
      "kind": "float",
      "default": 0.0,
      "min": 0.0,
      "max": 0.02,
      "doc": "Continuous small random perturbations; zero keeps evolution deterministic after seeding."
    },
    {
      "group": "ink",
      "name": "exposure",
      "kind": "float",
      "default": 1.2,
      "min": 0.2,
      "max": 3.0
    },
    {
      "group": "ink",
      "name": "contours",
      "kind": "float",
      "default": 0.32,
      "min": 0.0,
      "max": 1.0
    },
    {
      "group": "tone",
      "name": "contrast",
      "kind": "float",
      "default": 1.0,
      "min": 0.2,
      "max": 3.0,
      "doc": "Contrast of the activity field before coloring."
    },
    {
      "group": "tone",
      "name": "midpoint",
      "kind": "float",
      "default": 0.5,
      "min": 0.0,
      "max": 1.0,
      "doc": "Activity value centered in the palette."
    },
    {
      "group": "tone",
      "name": "gamma",
      "kind": "float",
      "default": 1.0,
      "min": 0.3,
      "max": 3.0,
      "doc": "Display gamma; higher values lift dark tones."
    },
    {
      "group": "palette",
      "name": "hue",
      "kind": "float",
      "default": 0.0,
      "min": -180.0,
      "max": 180.0,
      "doc": "Rotate all palette hues in degrees."
    },
    {
      "group": "palette",
      "name": "saturation",
      "kind": "float",
      "default": 1.0,
      "min": 0.0,
      "max": 2.0,
      "doc": "Zero gives monochrome; one is the original palette."
    },
    {
      "group": "palette",
      "name": "warmth",
      "kind": "float",
      "default": 1.0,
      "min": 0.0,
      "max": 2.0,
      "doc": "Amount of amber in changing edges."
    },
    {
      "group": "palette",
      "name": "highlights",
      "kind": "float",
      "default": 0.8,
      "min": 0.0,
      "max": 1.0,
      "doc": "Pale jade highlights in active regions."
    },
    {
      "group": "texture",
      "name": "edgeGain",
      "kind": "float",
      "default": 0.7,
      "min": 0.0,
      "max": 2.0,
      "doc": "Brightness of fine activity boundaries."
    },
    {
      "group": "texture",
      "name": "edgeWidth",
      "kind": "float",
      "default": 1.0,
      "min": 0.5,
      "max": 5.0,
      "doc": "Sampling radius of edge illumination in pixels."
    },
    {
      "group": "texture",
      "name": "changeGain",
      "kind": "float",
      "default": 8.0,
      "min": 0.0,
      "max": 30.0,
      "doc": "Sensitivity of warm color to differences from memory."
    },
    {
      "group": "texture",
      "name": "contourCount",
      "kind": "float",
      "default": 12.0,
      "min": 1.0,
      "max": 60.0,
      "doc": "Number of contour bands across the activity range."
    },
    {
      "group": "texture",
      "name": "contourSharpness",
      "kind": "float",
      "default": 18.0,
      "min": 1.0,
      "max": 80.0,
      "doc": "Higher values make thinner etched contours."
    },
    {
      "group": "texture",
      "name": "scaleGlow",
      "kind": "float",
      "default": 1.0,
      "min": 0.0,
      "max": 4.0,
      "doc": "Warm glow where large-scale structure is changing."
    }
  ]
}
*/
fn hash(q: vec2f) -> f32 {
    var h = u32(i32(q.x)+65536) * 1597334673u ^ u32(i32(q.y)+65536) * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967296.0;
}
fn readfield(at: vec2i) -> vec4f {
    let sz = vec2i(resolution);
    return textureLoad(field, ((at % sz) + sz) % sz, 0);
}
fn ring(at: vec2i, r: i32) -> f32 {
    return (readfield(at+vec2i(r,0)).r + readfield(at-vec2i(r,0)).r
      + readfield(at+vec2i(0,r)).r + readfield(at-vec2i(0,r)).r
      + readfield(at+vec2i(r,r)).r + readfield(at-vec2i(r,r)).r
      + readfield(at+vec2i(r,-r)).r + readfield(at+vec2i(-r,r)).r) / 8.0;
}
fn samplefield(at: vec2f) -> vec4f {
    let a = vec2i(floor(at)); let f = fract(at);
    return mix(mix(readfield(a),readfield(a+vec2i(1,0)),f.x),mix(readfield(a+vec2i(0,1)),readfield(a+vec2i(1,1)),f.x),f.y);
}
fn next_field(uv: vec2f) -> vec4f {
    let at = vec2i(uv * resolution);
    if frame == 0u {
        let n = 0.5 + 0.22 * (hash(vec2f(at)+p.seed)-0.5);
        return vec4f(n,n,0.0,1.0);
    }
    let grad = vec2f(readfield(at+vec2i(2,0)).g-readfield(at-vec2i(2,0)).g, readfield(at+vec2i(0,2)).g-readfield(at-vec2i(0,2)).g);
    let velocity = (vec2f(-grad.y,grad.x)*cos(radians(p.direction)) + grad*sin(radians(p.direction)))*p.curl + p.drift*vec2f(0.22*sin(f32(at.y)*0.012/p.driftSize),0.18*cos(f32(at.x)*0.013/p.driftSize));
    let old = samplefield(vec2f(at) - velocity*p.flow*p.pace);
    var best = 10.0; var drive = 0.0; var scale = 0.0;
    for (var k = 0; k < clamp(p.scales,1,8); k++) {
        let r = i32(clamp(round(p.size*pow(p.spacing,f32(k))),1.0,min(resolution.x,resolution.y)*0.25));
        let local = ring(at,r); let distant = ring(at,max(1,i32(round(f32(r)*p.inhibition))));
        let delta = local-distant;
        let variation = abs(delta);
        if variation < best { best = variation; drive = delta; scale = f32(k)/f32(max(p.scales-1,1)); }
    }
    // Saturating reinforcement and fatigue prevent a frozen uniform state.
    let reinforcement = drive / (p.sensitivity+abs(drive));
    let fatigue = old.r-old.g;
    let next = clamp(old.r + p.pace*(p.reinforcement*reinforcement + p.persistence*fatigue + p.balance*(0.5-old.r) + p.novelty*(hash(vec2f(at)+vec2f(f32(frame%65536u),p.seed))-0.5)),0.0,1.0);
    return vec4f(next,mix(next,old.g,p.memory),mix(old.b,scale,0.035),1.0);
}
fn shade(uv: vec2f) -> vec4f {
    // Texel space, so the edge taps wrap with the field instead of clamping at its border —
    // `at - 0.5` lands on the texel's own integer coordinate, which is what `samplefield` bisects.
    let at = uv * resolution;
    let c = readfield(vec2i(at));
    let g = at - vec2f(0.5);
    let value = clamp((c.r-p.midpoint)*p.contrast+0.5,0.0,1.0);
    let gx = samplefield(g+vec2f(p.edgeWidth,0.0)).r-samplefield(g-vec2f(p.edgeWidth,0.0)).r;
    let gy = samplefield(g+vec2f(0.0,p.edgeWidth)).r-samplefield(g-vec2f(0.0,p.edgeWidth)).r;
    let edge = clamp(length(vec2f(gx,gy))*5.0,0.0,1.0);
    let a = smoothstep(0.12,0.88,value);
    var col = mix(vec3f(0.008,0.015,0.035),vec3f(0.025,0.28,0.29),a);
    col = mix(col,vec3f(0.48,0.78,0.66),smoothstep(0.60,1.0,value)*p.highlights);
    let change = clamp(abs(c.r-c.g)*p.changeGain,0.0,1.0);
    col += edge*mix(vec3f(0.09,0.25,0.3),mix(vec3f(0.09,0.25,0.3),vec3f(0.8,0.39,0.12),p.warmth),change)*p.edgeGain;
    let lines = pow(0.5+0.5*cos(value*p.contourCount*6.2831853),p.contourSharpness);
    col *= 1.0-p.contours*lines;
    col += vec3f(0.12,0.085,0.045)*c.b*change*p.scaleGlow;
    // Rotate around the neutral axis, then adjust saturation and tone mapping.
    let axis = normalize(vec3f(1.0));
    let angle = radians(p.hue);
    col = col*cos(angle) + cross(axis,col)*sin(angle) + axis*dot(axis,col)*(1.0-cos(angle));
    let luminance = dot(col,vec3f(0.2126,0.7152,0.0722));
    col = max(mix(vec3f(luminance),col,p.saturation),vec3f(0.0));
    return vec4f(pow(1.0-exp(-col*p.exposure),vec3f(1.0/max(p.gamma,0.01))),1.0);
}
