/* goofi
{
  "doc": "Nested fBm pigments and layered refractive pools.\nChoose fBm or refractive pools. Nine palettes use the internal warp fields for secondary pigments and iridescent color. Pool brush controls create a moving ripple; optics controls depth, refraction, dispersion, absorption and lighting. Refraction is a layered screen-space approximation.\nFractal selects noise basis, octave spacing and folds. Geometry stretches, twists or mirrors the domain. Warping controls the two displacement scales and directions. Structure adds terraces and marble bands. These shape controls also feed the refractive pool.",
  "tags": [
    "image",
    "generator"
  ],
  "params": [
    {
      "group": "interaction",
      "name": "mode",
      "kind": "str",
      "default": "fbm",
      "options": [
        "fbm",
        "refractive pools"
      ]
    },
    {
      "group": "brush",
      "name": "brush_x",
      "kind": "float",
      "default": 0.5,
      "min": 0,
      "max": 1,
      "doc": "Horizontal brush position, normalized."
    },
    {
      "group": "brush",
      "name": "brush_y",
      "kind": "float",
      "default": 0.5,
      "min": 0,
      "max": 1,
      "doc": "Vertical brush position, normalized."
    },
    {
      "group": "brush",
      "name": "inject",
      "kind": "float",
      "default": 0.65,
      "min": 0,
      "max": 1,
      "doc": "Strength of the local ripple in refractive pools."
    },
    {
      "group": "brush",
      "name": "erase",
      "kind": "float",
      "default": 0,
      "min": 0,
      "max": 1,
      "doc": "Suppresses the local ripple; 1 fully suppresses it."
    },
    {
      "group": "brush",
      "name": "auto_orbit",
      "kind": "float",
      "default": 0.6,
      "min": 0,
      "max": 1,
      "doc": "Amount of automatic ripple motion; zero uses the manual brush position."
    },
    {
      "group": "optics",
      "name": "depth",
      "kind": "float",
      "default": 1.2,
      "min": 0,
      "max": 4,
      "doc": "Optical thickness of the pigment layers."
    },
    {
      "group": "optics",
      "name": "refraction",
      "kind": "float",
      "default": 0.8,
      "min": 0,
      "max": 2,
      "doc": "Screen-space bending of submerged pigment layers."
    },
    {
      "group": "optics",
      "name": "dispersion",
      "kind": "float",
      "default": 0.3,
      "min": 0,
      "max": 1,
      "doc": "Red/blue separation of refracted samples."
    },
    {
      "group": "optics",
      "name": "absorption",
      "kind": "float",
      "default": 0.65,
      "min": 0,
      "max": 2,
      "doc": "Depth-dependent colored attenuation."
    },
    {
      "group": "optics",
      "name": "relief",
      "kind": "float",
      "default": 0.65,
      "min": 0,
      "max": 2,
      "doc": "Surface-normal strength."
    },
    {
      "group": "optics",
      "name": "light_x",
      "kind": "float",
      "default": -0.45,
      "min": -1,
      "max": 1,
      "doc": "Horizontal light direction."
    },
    {
      "group": "optics",
      "name": "light_y",
      "kind": "float",
      "default": 0.55,
      "min": -1,
      "max": 1,
      "doc": "Vertical light direction."
    },
    {
      "group": "fractal",
      "name": "scale",
      "kind": "float",
      "default": 3.2,
      "min": 0.5,
      "max": 10
    },
    {
      "group": "fractal",
      "name": "octaves",
      "kind": "int",
      "default": 6,
      "min": 1,
      "max": 8
    },
    {
      "group": "fractal",
      "name": "roughness",
      "kind": "float",
      "default": 0.5,
      "min": 0.15,
      "max": 0.8
    },
    {
      "group": "fractal",
      "name": "warp",
      "kind": "float",
      "default": 4,
      "min": 0,
      "max": 8
    },
    {
      "group": "fractal",
      "name": "stages",
      "kind": "int",
      "default": 2,
      "min": 0,
      "max": 2
    },
    {
      "group": "fractal",
      "name": "seed",
      "kind": "float",
      "default": 0,
      "min": 0,
      "max": 100
    },
    {
      "group": "motion",
      "name": "speed",
      "kind": "float",
      "default": 0.22,
      "min": -1,
      "max": 1
    },
    {
      "group": "motion",
      "name": "time_warp",
      "kind": "float",
      "default": 0.65,
      "min": 0,
      "max": 2
    },
    {
      "group": "color",
      "name": "palette",
      "kind": "str",
      "default": "mineral",
      "options": [
        "mineral",
        "ember",
        "ink",
        "monochrome",
        "opalescent",
        "oxidized copper",
        "bioluminescent",
        "petroleum",
        "orchid"
      ]
    },
    {
      "group": "color",
      "name": "veins",
      "kind": "float",
      "default": 0.35,
      "min": 0,
      "max": 1
    },
    {
      "group": "color",
      "name": "contrast",
      "kind": "float",
      "default": 1.25,
      "min": 0.5,
      "max": 2.5
    },
    {
      "group": "color",
      "name": "exposure",
      "kind": "float",
      "default": 1.1,
      "min": 0.25,
      "max": 2.5
    },
    {
      "group": "color",
      "name": "complexity",
      "kind": "float",
      "default": 0.7,
      "min": 0,
      "max": 1
    },
    {
      "group": "color",
      "name": "palette_shift",
      "kind": "float",
      "default": 0,
      "min": -1,
      "max": 1
    },
    {
      "group": "color",
      "name": "color_spread",
      "kind": "float",
      "default": 1.35,
      "min": 0.25,
      "max": 3
    },
    {
      "group": "color",
      "name": "iridescence",
      "kind": "float",
      "default": 0.25,
      "min": 0,
      "max": 1
    },
    {
      "group": "color",
      "name": "saturation",
      "kind": "float",
      "default": 1.05,
      "min": 0,
      "max": 2
    },
    {
      "group": "fractal",
      "name": "noise_basis",
      "kind": "str",
      "default": "value",
      "options": [
        "value",
        "gradient",
        "cells"
      ],
      "doc": "Smooth value noise, directional gradient noise, or cellular distances."
    },
    {
      "group": "fractal",
      "name": "fold",
      "kind": "str",
      "default": "plain",
      "options": [
        "plain",
        "ridged",
        "billow"
      ],
      "doc": "Fold each octave into crests or rounded cloud lobes."
    },
    {
      "group": "fractal",
      "name": "fold_mix",
      "kind": "float",
      "default": 1,
      "min": 0,
      "max": 1,
      "doc": "Strength of ridged or billow folding; plain ignores this."
    },
    {
      "group": "fractal",
      "name": "lacunarity",
      "kind": "float",
      "default": 2.03,
      "min": 1.1,
      "max": 3.5,
      "doc": "Frequency multiplier between octaves."
    },
    {
      "group": "fractal",
      "name": "octave_angle",
      "kind": "float",
      "default": 36.869898,
      "min": -180,
      "max": 180,
      "doc": "Rotation between noise octaves in degrees."
    },
    {
      "group": "geometry",
      "name": "rotation",
      "kind": "float",
      "default": 0,
      "min": -180,
      "max": 180,
      "doc": "Rotate the entire domain in degrees."
    },
    {
      "group": "geometry",
      "name": "stretch",
      "kind": "float",
      "default": 0,
      "min": -3,
      "max": 3,
      "doc": "Directional stretch in powers of two; zero is isotropic."
    },
    {
      "group": "geometry",
      "name": "twist",
      "kind": "float",
      "default": 0,
      "min": -4,
      "max": 4,
      "doc": "Radius-dependent rotation for spirals and whorls."
    },
    {
      "group": "geometry",
      "name": "pan_x",
      "kind": "float",
      "default": 0,
      "min": -10,
      "max": 10,
      "doc": "Horizontal travel through the noise domain."
    },
    {
      "group": "geometry",
      "name": "pan_y",
      "kind": "float",
      "default": 0,
      "min": -10,
      "max": 10,
      "doc": "Vertical travel through the noise domain."
    },
    {
      "group": "geometry",
      "name": "symmetry",
      "kind": "int",
      "default": 0,
      "min": 0,
      "max": 12,
      "doc": "Zero disables folding; 2 to 12 makes mirrored radial sectors."
    },
    {
      "group": "warping",
      "name": "warp_scale",
      "kind": "float",
      "default": 1,
      "min": 0.15,
      "max": 4,
      "doc": "Spatial scale of the first displacement field."
    },
    {
      "group": "warping",
      "name": "second_scale",
      "kind": "float",
      "default": 1,
      "min": 0.15,
      "max": 4,
      "doc": "Spatial scale of the second displacement field."
    },
    {
      "group": "warping",
      "name": "second_gain",
      "kind": "float",
      "default": 1,
      "min": 0,
      "max": 2,
      "doc": "Strength of the final displacement relative to warp."
    },
    {
      "group": "warping",
      "name": "warp_angle",
      "kind": "float",
      "default": 0,
      "min": -180,
      "max": 180,
      "doc": "Rotate displacement vectors to turn folds and streams."
    },
    {
      "group": "warping",
      "name": "cross_mix",
      "kind": "float",
      "default": 0,
      "min": 0,
      "max": 1,
      "doc": "Cross-couple the two components of the displacement."
    },
    {
      "group": "motion",
      "name": "evolution",
      "kind": "float",
      "default": 1,
      "min": 0,
      "max": 3,
      "doc": "Rate of shape evolution within the warp layers."
    },
    {
      "group": "motion",
      "name": "drift_x",
      "kind": "float",
      "default": 0,
      "min": -1,
      "max": 1,
      "doc": "Horizontal domain drift, scaled by speed."
    },
    {
      "group": "motion",
      "name": "drift_y",
      "kind": "float",
      "default": 0,
      "min": -1,
      "max": 1,
      "doc": "Vertical domain drift, scaled by speed."
    },
    {
      "group": "structure",
      "name": "terraces",
      "kind": "float",
      "default": 0,
      "min": 0,
      "max": 1,
      "doc": "Blend the final density into stepped terraces."
    },
    {
      "group": "structure",
      "name": "terrace_count",
      "kind": "float",
      "default": 6,
      "min": 2,
      "max": 24,
      "doc": "Number of density terraces."
    },
    {
      "group": "structure",
      "name": "marbling",
      "kind": "float",
      "default": 0,
      "min": 0,
      "max": 1,
      "doc": "Blend the density into flowing marble bands."
    },
    {
      "group": "structure",
      "name": "band_frequency",
      "kind": "float",
      "default": 12,
      "min": 1,
      "max": 40,
      "doc": "Number of marble oscillations across the density range."
    }
  ],
  "state": [
    "pigments"
  ]
}
*/

// Integer hashing avoids sine-hash seams on GPU lattice boundaries.
fn hash(cell: vec2i) -> f32 {
    let c = vec2u(cell + 65536);
    var h = c.x * 1597334673u ^ c.y * 3812015801u ^ bitcast<u32>(p.seed);
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967296.0;
}

fn noise(v: vec2f) -> f32 {
    let cell = vec2i(floor(v));
    let f = fract(v);
    let w = f*f*f*(f*(f*6.0-15.0)+10.0);
    return mix(mix(hash(cell),hash(cell+vec2i(1,0)),w.x),
               mix(hash(cell+vec2i(0,1)),hash(cell+vec2i(1,1)),w.x),w.y);
}


fn turn(v: vec2f, degrees: f32) -> vec2f {
    let ang=degrees*0.01745329252;
    let c=cos(ang); let sn=sin(ang);
    return vec2f(c*v.x-sn*v.y,sn*v.x+c*v.y);
}
fn grad_dot(cell: vec2i, d: vec2f) -> f32 {
    let angle=hash(cell)*6.2831853;
    return dot(vec2f(cos(angle),sin(angle)),d);
}
fn basis(v: vec2f) -> f32 {
    if p.noise_basis==0u { return noise(v); }
    let cell=vec2i(floor(v));
    let f=fract(v);
    if p.noise_basis==1u {
        let w=f*f*f*(f*(f*6.0-15.0)+10.0);
        let n=mix(mix(grad_dot(cell,f),grad_dot(cell+vec2i(1,0),f-vec2f(1,0)),w.x),
            mix(grad_dot(cell+vec2i(0,1),f-vec2f(0,1)),
                grad_dot(cell+vec2i(1,1),f-vec2f(1,1)),w.x),w.y);
        return clamp(0.5+n*0.7,0.0,1.0);
    }
    var nearest=4.0;
    for(var y=-1;y<=1;y++){
        for(var x=-1;x<=1;x++){
            let o=vec2i(x,y);
            let point=vec2f(o)+vec2f(hash(cell+o),hash(cell+o+vec2i(127,311)));
            nearest=min(nearest,length(point-f));
        }
    }
    return clamp(nearest,0.0,1.0);
}

fn fbm(v: vec2f) -> f32 {
    var at = v;
    var sum = 0.0;
    var amp = 1.0;
    var norm = 0.0;
    for (var i=0; i<clamp(p.octaves,1,8); i++) {
        let n=basis(at);
        var folded=n;
        if p.fold==1u { folded=1.0-abs(2.0*n-1.0); }
        if p.fold==2u { folded=abs(2.0*n-1.0); }
        sum += amp*mix(n,folded,clamp(p.fold_mix,0.0,1.0));
        norm += amp;
        // Rotate each octave so lattice axes do not stack.
        at = turn(at,p.octave_angle)*clamp(p.lacunarity,1.01,4.0)+vec2f(13.1,7.7);
        amp *= clamp(p.roughness,0.0,0.85);
    }
    return sum/max(norm,0.001);
}


// Seven pigment stops, smoothly interpolated within each interval.
fn pigment(x: f32) -> vec3f {
    var c: array<vec3f,7>;
    switch p.palette {
        default: { c = array<vec3f,7>(vec3f(0.031373,0.082353,0.113725),vec3f(0.070588,0.247059,0.286275),vec3f(0.086275,0.521569,0.466667),vec3f(0.443137,0.607843,0.407843),vec3f(0.729412,0.458824,0.172549),vec3f(0.854902,0.752941,0.501961),vec3f(0.941176,0.960784,0.823529)); }
        case 1u: { c = array<vec3f,7>(vec3f(0.031373,0.011765,0.074510),vec3f(0.160784,0.043137,0.250980),vec3f(0.450980,0.047059,0.188235),vec3f(0.729412,0.141176,0.039216),vec3f(0.976471,0.407843,0.070588),vec3f(1.000000,0.768627,0.341176),vec3f(1.000000,0.941176,0.760784)); }
        case 2u: { c = array<vec3f,7>(vec3f(0.011765,0.027451,0.086275),vec3f(0.070588,0.141176,0.333333),vec3f(0.207843,0.235294,0.556863),vec3f(0.407843,0.239216,0.549020),vec3f(0.694118,0.427451,0.600000),vec3f(0.517647,0.745098,0.811765),vec3f(0.921569,0.960784,1.000000)); }
        case 3u: { c = array<vec3f,7>(vec3f(0.000000,0.000000,0.000000),vec3f(0.160784,0.160784,0.160784),vec3f(0.333333,0.333333,0.333333),vec3f(0.501961,0.501961,0.501961),vec3f(0.666667,0.666667,0.666667),vec3f(0.866667,0.866667,0.866667),vec3f(1.000000,1.000000,1.000000)); }
        case 4u: { c = array<vec3f,7>(vec3f(0.031373,0.043137,0.141176),vec3f(0.156863,0.278431,0.450980),vec3f(0.309804,0.666667,0.662745),vec3f(0.611765,0.474510,0.760784),vec3f(0.917647,0.607843,0.662745),vec3f(0.929412,0.811765,0.509804),vec3f(0.949020,1.000000,0.909804)); }
        case 5u: { c = array<vec3f,7>(vec3f(0.011765,0.062745,0.043137),vec3f(0.027451,0.294118,0.243137),vec3f(0.082353,0.572549,0.482353),vec3f(0.415686,0.647059,0.474510),vec3f(0.521569,0.250980,0.105882),vec3f(0.843137,0.517647,0.203922),vec3f(0.960784,0.866667,0.631373)); }
        case 6u: { c = array<vec3f,7>(vec3f(0.003922,0.011765,0.031373),vec3f(0.011765,0.058824,0.168627),vec3f(0.094118,0.133333,0.415686),vec3f(0.043137,0.443137,0.545098),vec3f(0.070588,0.823529,0.631373),vec3f(0.658824,0.925490,0.298039),vec3f(0.925490,1.000000,0.792157)); }
        case 7u: { c = array<vec3f,7>(vec3f(0.007843,0.007843,0.027451),vec3f(0.094118,0.035294,0.223529),vec3f(0.321569,0.125490,0.513725),vec3f(0.054902,0.509804,0.564706),vec3f(0.541176,0.686275,0.188235),vec3f(0.858824,0.384314,0.152941),vec3f(0.886275,0.694118,0.847059)); }
        case 8u: { c = array<vec3f,7>(vec3f(0.035294,0.007843,0.101961),vec3f(0.203922,0.047059,0.325490),vec3f(0.513725,0.078431,0.498039),vec3f(0.807843,0.223529,0.549020),vec3f(0.972549,0.537255,0.568627),vec3f(0.686275,0.713725,0.882353),vec3f(0.972549,0.905882,0.780392)); }
    }
    let x6 = clamp(x,0.0,1.0)*6.0;
    let i = min(u32(floor(x6)),5u);
    return mix(c[i],c[i+1u],smoothstep(0.0,1.0,x6-f32(i)));
}

fn base_pigments(uv: vec2f) -> vec4f {
    let aspect = vec2f(resolution.x/max(resolution.y,1.0),1.0);
    var domain=(uv-0.5)*aspect*max(p.scale,0.01);
    domain=turn(domain,p.rotation);
    let elongation=exp2(clamp(p.stretch,-4.0,4.0));
    domain*=vec2f(elongation,1.0/elongation);
    let radius=length(domain);
    if p.symmetry>=2 {
        let sector=6.2831853/f32(clamp(p.symmetry,2,12));
        let angle=atan2(domain.y,domain.x);
        let folded=abs((angle+sector*0.5)-floor((angle+sector*0.5)/sector)*sector-sector*0.5);
        domain=radius*vec2f(cos(folded),sin(folded));
    }
    domain=turn(domain,p.twist*radius*35.0);
    let at=domain+vec2f(2.3,1.7)+vec2f(p.pan_x,p.pan_y)+
        time*p.speed*vec2f(p.drift_x,p.drift_y);
    let t = time*p.speed;
    // Bounded spatial phase distortion: time evolves smoothly without an
    // ever-growing spatial gradient. At speed zero this whole field is static.
    let clock_field = noise(at*0.55+vec2f(0.07*t,-0.04*t));
    let tau = (t + p.time_warp*2.0*sin(t*0.37+6.2831853*clock_field))*p.evolution;
    let first_at=at*max(p.warp_scale,0.01);
    let q = vec2f(fbm(first_at+vec2f(0.12*tau,0.07*tau)),
                  fbm(first_at+vec2f(5.2,1.3)+vec2f(-0.09*tau,0.11*tau)));
    let qdir=turn(mix(q,q.yx,clamp(p.cross_mix,0.0,1.0)),p.warp_angle);
    let second_at=at*max(p.second_scale,0.01)+p.warp*qdir;
    let r = vec2f(fbm(second_at+vec2f(1.7,9.2)+vec2f(0.15*tau,-0.08*tau)),
                  fbm(second_at+vec2f(8.3,2.8)+vec2f(-0.11*tau,0.13*tau)));
    let rdir=turn(mix(r,r.yx,clamp(p.cross_mix,0.0,1.0)),p.warp_angle);
    var f = fbm(at+p.warp*p.second_gain*rdir);
    if p.stages == 0 { f = fbm(at+vec2f(0.1*tau,0.0)); }
    if p.stages == 1 { f = fbm(at+p.warp*qdir); }
    let count=max(p.terrace_count,2.0);
    let tier=f*count;
    let terraced=(floor(tier)+smoothstep(0.35,0.65,fract(tier)))/count;
    f=mix(f,terraced,clamp(p.terraces,0.0,1.0));
    let bands=0.5+0.5*sin(f*max(p.band_frequency,1.0)*6.2831853);
    f=mix(f,bands,clamp(p.marbling,0.0,1.0));
    let density = clamp((f-0.5)*p.contrast*2.1+0.5,0.0,1.0);
    // q and r reveal secondary pigments and interfaces independently of density.
    let phase = p.palette_shift;
    let spread = max(p.color_spread,0.01);
    let base = (density-0.5)*spread+0.5+phase;
    let inclusion = smoothstep(0.10,0.42,length(q-0.5));
    let interface_mask = smoothstep(0.05,0.35,abs(r.x-r.y));
    let secondary = pigment(0.5+(r.y-q.x)*spread*1.8+phase);
    var rgb = pigment(base);
    rgb = mix(rgb,secondary,inclusion*p.complexity*0.70);
    // Artistic spectral tint, not a physical refraction model.
    let spectral = 0.5+0.5*cos(vec3f(0.0,2.1,4.2)+
        (r.x*2.2-q.y*1.4+f*1.7+phase)*6.2831853);
    rgb = mix(rgb,rgb*(0.5+spectral)*1.2,p.iridescence*interface_mask);
    let filament = pow(clamp(1.0-abs(sin(f*42.0+r.x*7.0)),0.0,1.0),9.0);
    let vein_color = pigment(0.78+(q.y-r.x)*0.6+phase);
    rgb *= 0.72+0.50*density;
    rgb += p.veins*filament*vein_color*0.55;
    let luminance = dot(rgb,vec3f(0.2126,0.7152,0.0722));
    rgb = max(mix(vec3f(luminance),rgb,p.saturation),vec3f(0.0));
    if p.palette == 3u { rgb=vec3f(density); }
    rgb = clamp(rgb*p.exposure,vec3f(0.0),vec3f(1.0));
    return vec4f(rgb,f);
}


// A movable ripple disturbs the refractive pool surface.
fn brush_center() -> vec2f {
    let t = time*p.speed;
    return clamp(vec2f(p.brush_x,p.brush_y)+p.auto_orbit*
        vec2f(0.23*sin(t*0.41),0.19*cos(t*0.33)),vec2f(0.0),vec2f(1.0));
}
fn sample_pigment(uv: vec2f) -> vec4f {
    // Mirror edges to avoid stretched strips at large refractive displacements.
    let mirror=1.0-abs(fract(uv*0.5)*2.0-1.0);
    return textureSampleLevel(pigments,samp,mirror,0.0);
}
fn next_pigments(uv: vec2f) -> vec4f {
    return base_pigments(uv);
}
fn pool_height(uv: vec2f) -> f32 {
    let aspect=vec2f(resolution.x/max(resolution.y,1.0),1.0);
    let d=length((uv-brush_center())*aspect);
    let ripple=sin(d*95.0-time*p.speed*3.0)*exp(-d*8.0);
    return sample_pigment(uv).a+0.08*p.inject*(1.0-p.erase)*ripple;
}
fn spectral_sample(uv: vec2f,offset: vec2f) -> vec3f {
    let separation=clamp(p.dispersion,0.0,1.0)*0.16;
    return vec3f(sample_pigment(uv+offset*(1.0+separation)).r,
                 sample_pigment(uv+offset).g,
                 sample_pigment(uv+offset*(1.0-separation)).b);
}
fn shade(uv: vec2f) -> vec4f {
    let base=sample_pigment(uv);
    if p.mode==0u { return vec4f(base.rgb,1.0); }
    // Layered screen-space refraction; not ray-traced caustics.
    let px=2.0/max(resolution,vec2f(1.0));
    let h=pool_height(uv);
    let grad=vec2f(pool_height(uv+vec2f(px.x,0.0))-pool_height(uv-vec2f(px.x,0.0)),
                   pool_height(uv+vec2f(0.0,px.y))-pool_height(uv-vec2f(0.0,px.y)))/px;
    let normal=normalize(vec3f(-grad*0.10*p.relief,1.0));
    let depth=max(p.depth,0.0);
    let offset=normal.xy*0.055*p.refraction*depth;
    var rgb=vec3f(0.0);
    var weight=0.0;
    for(var layer=0;layer<3;layer++){
        let z=(f32(layer)+1.0)/3.0;
        let w=1.0-z*0.35;
        let submerged=spectral_sample(uv,offset*z);
        let transmission=exp(-vec3f(0.65,0.27,0.12)*p.absorption*depth*z*(0.35+h));
        rgb+=submerged*transmission*w;
        weight+=w;
    }
    rgb/=weight;
    let light=normalize(vec3f(p.light_x,p.light_y,0.85));
    let diffuse=0.65+0.35*max(dot(normal,light),0.0);
    let spec=pow(max(dot(normal,normalize(light+vec3f(0.0,0.0,1.0))),0.0),48.0);
    let fresnel=pow(1.0-max(normal.z,0.0),3.0);
    rgb=rgb*diffuse+vec3f(1.0,0.91,0.76)*spec*0.32+
        vec3f(0.25,0.40,0.60)*fresnel*0.25;
    return vec4f(clamp(rgb,vec3f(0.0),vec3f(1.0)),1.0);
}
