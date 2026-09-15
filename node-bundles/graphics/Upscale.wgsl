/* goofi
{ "doc": "Enlarge an SDR image on the GPU.\nSet common/width and common/height to the output size.\nFSR 1 and NIS preserve edges and sharpen existing detail. They do not reconstruct detail with AI. Input RGB must be in 0..1. Alpha uses linear sampling. Smaller output dimensions use linear sampling.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "upscale", "name": "method", "kind": "str", "default": "fsr1", "options": ["linear", "fsr1", "nis"]},
    {"group": "upscale", "name": "sharpness", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0} ] }
*/
// FSR 1 EASU/RCAS port, Copyright (c) 2021 Advanced Micro Devices, Inc.
// NIS SDR port, Copyright (c) 2022 NVIDIA CORPORATION & AFFILIATES.
// See UPSCALING-LICENSES.txt. Exact reciprocal operations replace fast approximations.

fn load_rgb(at: vec2i) -> vec3f {
    return clamp(textureLoad(input, clamp(at, vec2i(0), vec2i(textureDimensions(input))-1), 0).rgb, vec3f(0.0), vec3f(1.0));
}

fn easu_direction(a: f32, b: f32, c: f32, d: f32, e: f32) -> vec3f {
    let gradient = vec2f(d-b, e-a);
    let extent = max(vec2f(abs(d-c), abs(e-c)), vec2f(abs(c-b), abs(c-a)));
    let strength = clamp(abs(gradient)/max(extent, vec2f(1e-8)), vec2f(0.0), vec2f(1.0));
    return vec3f(gradient, dot(strength, strength));
}

fn easu(uv: vec2f) -> vec3f {
    let position = clamp(uv, 0.5/resolution, (resolution-0.5)/resolution)*vec2f(textureDimensions(input))-0.5;
    let base = vec2i(floor(position));
    let fraction = fract(position);
    let offsets = array<vec2i, 12>(vec2i(0,-1), vec2i(1,-1), vec2i(-1,0), vec2i(0,0),
        vec2i(1,0), vec2i(2,0), vec2i(-1,1), vec2i(0,1), vec2i(1,1), vec2i(2,1), vec2i(0,2), vec2i(1,2));
    var colors: array<vec3f, 12>;
    var luma: array<f32, 12>;
    for (var i = 0u; i < 12u; i++) {
        colors[i] = load_rgb(base+offsets[i]);
        luma[i] = dot(colors[i], vec3f(0.5,1.0,0.5));
    }
    let s = easu_direction(luma[0],luma[2],luma[3],luma[4],luma[7]);
    let t = easu_direction(luma[1],luma[3],luma[4],luma[5],luma[8]);
    let u = easu_direction(luma[3],luma[6],luma[7],luma[8],luma[10]);
    let v = easu_direction(luma[4],luma[7],luma[8],luma[9],luma[11]);
    let edge = mix(mix(s,t,fraction.x),mix(u,v,fraction.x),fraction.y);
    var direction = vec2f(1.0,0.0);
    if (dot(edge.xy,edge.xy) >= 1.0/32768.0) { direction = normalize(edge.xy); }
    let strength = edge.z*edge.z*0.25;
    let stretch = dot(direction,direction)/max(abs(direction.x),abs(direction.y));
    let extent = vec2f(1.0+(stretch-1.0)*strength,1.0-0.5*strength);
    let lobe = 0.5-0.29*strength;
    var sum = vec3f(0.0);
    var weight = 0.0;
    for (var i = 0u; i < 12u; i++) {
        let offset = vec2f(offsets[i])-fraction;
        let rotated = vec2f(dot(offset,direction),dot(offset,vec2f(-direction.y,direction.x)))*extent;
        let distance = min(dot(rotated,rotated),1.0/lobe);
        let window = lobe*distance-1.0;
        let kernel = 0.4*distance-1.0;
        let w = (1.5625*kernel*kernel-0.5625)*window*window;
        sum += colors[i]*w;
        weight += w;
    }
    let lo = min(min(colors[3],colors[4]),min(colors[7],colors[8]));
    let hi = max(max(colors[3],colors[4]),max(colors[7],colors[8]));
    return clamp(sum/max(weight,1e-8),lo,hi);
}

fn fsr1(uv: vec2f) -> vec3f {
    let e = easu(uv);
    if (p.sharpness == 0.0) { return e; }
    // Evaluate the five EASU pixels used by RCAS in this pass. This avoids a
    // frame of delay or a hidden graph node, at the cost of repeated sampling.
    let pixel = 1.0/resolution;
    let b = easu(uv-vec2f(0.0,pixel.y));
    let d = easu(uv-vec2f(pixel.x,0.0));
    let f = easu(uv+vec2f(pixel.x,0.0));
    let h = easu(uv+vec2f(0.0,pixel.y));
    let lo = min(min(b,d),min(f,h));
    let hi = max(max(b,d),max(f,h));
    let hit_min = min(lo,e)/max(4.0*hi,vec3f(1e-8));
    let hit_max = (1.0-max(hi,e))/min(4.0*lo-4.0,vec3f(-1e-8));
    let lobes = max(-hit_min,hit_max);
    let lobe = max(-0.1875,min(max(lobes.r,max(lobes.g,lobes.b)),0.0))*p.sharpness;
    return clamp((e+lobe*(b+d+f+h))/(1.0+4.0*lobe),vec3f(0.0),vec3f(1.0));
}

fn nis_edge(tile: array<array<f32,6>,6>, x: u32, y: u32) -> vec4f {
    let a = tile[y-1u][x-1u]; let b = tile[y-1u][x]; let c = tile[y-1u][x+1u];
    let d = tile[y][x-1u]; let f = tile[y][x+1u];
    let g = tile[y+1u][x-1u]; let h = tile[y+1u][x]; let i = tile[y+1u][x+1u];
    let gradients = abs(vec4f(a+b+c-g-h-i,a+d+g-c-f-i,d+a+b-h-i-f,d+g+h-b-c-f));
    let axial = vec2f(max(gradients.x,gradients.y),min(gradients.x,gradients.y));
    let diagonal = vec2f(max(gradients.z,gradients.w),min(gradients.z,gradients.w));
    if (axial.x+diagonal.x == 0.0) { return vec4f(0.0); }
    let ca = axial.x > axial.y*(2254.0/1024.0) && axial.x > 0.0625 && axial.x > diagonal.y;
    let cd = diagonal.x > diagonal.y*(2254.0/1024.0) && diagonal.x > 0.0625 && diagonal.x > axial.y;
    let wa = select(1.0,axial.x/(axial.x+diagonal.x),ca && cd);
    let wd = select(1.0,diagonal.x/(axial.x+diagonal.x),ca && cd);
    return vec4f(select(0.0,wa,ca && gradients.x == axial.x),select(0.0,wa,ca && gradients.x != axial.x),
        select(0.0,wd,cd && gradients.z == diagonal.x),select(0.0,wd,cd && gradients.z != diagonal.x));
}

fn nis_poly(values: array<f32,6>, phase: u32) -> f32 {
    var value = 0.0; var usm = 0.0;
    for (var i=0u; i<6u; i++) {
        value += values[i]*nis_scale[phase][i];
        usm += values[i]*nis_usm[phase][i];
    }
    let slider = p.sharpness-0.5;
    let min_scale = select(1.0,1.25,slider>=0.0);
    let max_scale = select(1.75,1.25,slider>=0.0);
    let strength_min = max(0.0,0.4+slider*min_scale*1.2);
    let strength_max = 1.6+slider*max_scale*1.8;
    let limit_min = max(0.1,0.14+slider*min_scale*0.32);
    let limit_max = 0.5+slider*min_scale*0.6;
    let ramp = 1.0-clamp((value-0.45)/0.45,0.0,1.0);
    usm *= mix(strength_min,strength_max,ramp);
    let limit = max(0.0,mix(limit_min,limit_max,ramp)*value);
    usm = clamp(usm,-limit,limit);
    let sa = select(values[3],values[0],phase<=32u);
    let sb = select(values[5],values[2],phase<=32u);
    let a = max(max(values[1],values[2]),sa)-min(min(values[1],values[2]),sa);
    let b = max(max(values[3],values[4]),sb)-min(min(values[3],values[4]),sb);
    let ratio = max(a,b)/(min(a,b)+1.0/255.0);
    return value+usm*(1.0-clamp((ratio-2.0)/8.0,0.0,1.0));
}

fn nis(uv: vec2f) -> vec3f {
    let position = uv*vec2f(textureDimensions(input))-0.5;
    let base = vec2i(floor(position));
    let fraction = fract(position);
    let phase = min(vec2u(fraction*64.0),vec2u(63u));
    var tile: array<array<f32,6>,6>;
    for (var y=0u; y<6u; y++) {
        for (var x=0u; x<6u; x++) {
            tile[y][x] = dot(load_rgb(base+vec2i(i32(x)-2,i32(y)-2)),vec3f(0.2126,0.7152,0.0722));
        }
    }
    let weights = mix(mix(nis_edge(tile,2u,2u),nis_edge(tile,3u,2u),fraction.x),
        mix(nis_edge(tile,2u,3u),nis_edge(tile,3u,3u),fraction.x),fraction.y);
    var normal = 0.0;
    for (var x=0u; x<6u; x++) {
        var vertical = 0.0;
        for (var y=0u; y<6u; y++) { vertical += tile[y][x]*nis_scale[phase.y][y]; }
        normal += vertical*nis_scale[phase.x][x];
    }
    var luminance = normal*(1.0-dot(weights,vec4f(1.0)));
    var vertical: array<f32,6>; var horizontal: array<f32,6>;
    for (var i=0u; i<6u; i++) {
        vertical[i] = mix(tile[i][2],tile[i][3],fraction.x);
        horizontal[i] = mix(tile[2][i],tile[3][i],fraction.y);
    }
    luminance += nis_poly(vertical,phase.y)*weights.x+nis_poly(horizontal,phase.x)*weights.y;
    var diagonal: array<f32,7>;
    var values: array<f32,6>;
    if (weights.z > 0.0) {
        let blend = 0.5+0.5*(fraction.x-fraction.y);
        diagonal[1] = mix(tile[2][1],tile[1][2],blend);
        diagonal[3] = mix(tile[3][2],tile[2][3],blend);
        diagonal[5] = mix(tile[4][3],tile[3][4],blend);
        let t = blend-0.5;
        diagonal[0] = mix(tile[1][1],select(tile[2][0],tile[0][2],t>=0.0),abs(t));
        diagonal[2] = mix(tile[2][2],select(tile[3][1],tile[1][3],t>=0.0),abs(t));
        diagonal[4] = mix(tile[3][3],select(tile[4][2],tile[2][4],t>=0.0),abs(t));
        diagonal[6] = mix(tile[4][4],select(tile[5][3],tile[3][5],t>=0.0),abs(t));
        let along = fraction.x+fraction.y;
        let shift = select(0u,1u,along>=1.0);
        for (var i=0u; i<6u; i++) { values[i] = diagonal[i+shift]; }
        luminance += nis_poly(values,min(u32((along-f32(shift))*64.0),63u))*weights.z;
    }
    if (weights.w > 0.0) {
        let blend = 0.5*(fraction.x+fraction.y);
        diagonal[1] = mix(tile[3][1],tile[4][2],blend);
        diagonal[3] = mix(tile[2][2],tile[3][3],blend);
        diagonal[5] = mix(tile[1][3],tile[2][4],blend);
        let t = blend-0.5;
        diagonal[0] = mix(tile[4][1],select(tile[3][0],tile[5][2],t>=0.0),abs(t));
        diagonal[2] = mix(tile[3][2],select(tile[2][1],tile[4][3],t>=0.0),abs(t));
        diagonal[4] = mix(tile[2][3],select(tile[1][2],tile[3][4],t>=0.0),abs(t));
        diagonal[6] = mix(tile[1][4],select(tile[0][3],tile[2][5],t>=0.0),abs(t));
        let along = 1.0+fraction.x-fraction.y;
        let shift = select(0u,1u,along>=1.0);
        for (var i=0u; i<6u; i++) { values[i] = diagonal[i+shift]; }
        luminance += nis_poly(values,min(u32((along-f32(shift))*64.0),63u))*weights.w;
    }
    let color = textureSampleLevel(input,samp,uv,0.0).rgb;
    return clamp(color+vec3f(luminance-dot(color,vec3f(0.2126,0.7152,0.0722))),vec3f(0.0),vec3f(1.0));
}

fn shade(uv: vec2f) -> vec4f {
    let linear = textureSampleLevel(input,samp,uv,0.0);
    if (p.method == 0u || any(resolution < vec2f(textureDimensions(input)))) { return linear; }
    if (p.method == 1u) { return vec4f(fsr1(uv),linear.a); }
    return vec4f(nis(uv),linear.a);
}

// NIS 1.0.3 filter bank, 64 phases and six taps.
const nis_scale = array<array<f32,6>,64>(
    array<f32,6>(0.0,0.0,1.0000,0.0,0.0,0.0),
    array<f32,6>(0.0029,-0.0127,1.0000,0.0132,-0.0034,0.0),
    array<f32,6>(0.0063,-0.0249,0.9985,0.0269,-0.0068,0.0),
    array<f32,6>(0.0088,-0.0361,0.9956,0.0415,-0.0103,0.0005),
    array<f32,6>(0.0117,-0.0474,0.9932,0.0562,-0.0142,0.0005),
    array<f32,6>(0.0142,-0.0576,0.9897,0.0713,-0.0181,0.0005),
    array<f32,6>(0.0166,-0.0674,0.9844,0.0874,-0.0220,0.0010),
    array<f32,6>(0.0186,-0.0762,0.9785,0.1040,-0.0264,0.0015),
    array<f32,6>(0.0205,-0.0850,0.9727,0.1206,-0.0308,0.0020),
    array<f32,6>(0.0225,-0.0928,0.9648,0.1382,-0.0352,0.0024),
    array<f32,6>(0.0239,-0.1006,0.9575,0.1558,-0.0396,0.0029),
    array<f32,6>(0.0254,-0.1074,0.9487,0.1738,-0.0439,0.0034),
    array<f32,6>(0.0264,-0.1138,0.9390,0.1929,-0.0488,0.0044),
    array<f32,6>(0.0278,-0.1191,0.9282,0.2119,-0.0537,0.0049),
    array<f32,6>(0.0288,-0.1245,0.9170,0.2310,-0.0581,0.0059),
    array<f32,6>(0.0293,-0.1294,0.9058,0.2510,-0.0630,0.0063),
    array<f32,6>(0.0303,-0.1333,0.8926,0.2710,-0.0679,0.0073),
    array<f32,6>(0.0308,-0.1367,0.8789,0.2915,-0.0728,0.0083),
    array<f32,6>(0.0308,-0.1401,0.8657,0.3120,-0.0776,0.0093),
    array<f32,6>(0.0313,-0.1426,0.8506,0.3330,-0.0825,0.0103),
    array<f32,6>(0.0313,-0.1445,0.8354,0.3540,-0.0874,0.0112),
    array<f32,6>(0.0313,-0.1460,0.8193,0.3755,-0.0923,0.0122),
    array<f32,6>(0.0313,-0.1470,0.8022,0.3965,-0.0967,0.0137),
    array<f32,6>(0.0308,-0.1479,0.7856,0.4185,-0.1016,0.0146),
    array<f32,6>(0.0303,-0.1479,0.7681,0.4399,-0.1060,0.0156),
    array<f32,6>(0.0298,-0.1479,0.7505,0.4614,-0.1104,0.0166),
    array<f32,6>(0.0293,-0.1470,0.7314,0.4829,-0.1147,0.0181),
    array<f32,6>(0.0288,-0.1460,0.7119,0.5049,-0.1187,0.0190),
    array<f32,6>(0.0278,-0.1445,0.6929,0.5264,-0.1226,0.0200),
    array<f32,6>(0.0273,-0.1431,0.6724,0.5479,-0.1260,0.0215),
    array<f32,6>(0.0264,-0.1411,0.6528,0.5693,-0.1299,0.0225),
    array<f32,6>(0.0254,-0.1387,0.6323,0.5903,-0.1328,0.0234),
    array<f32,6>(0.0244,-0.1357,0.6113,0.6113,-0.1357,0.0244),
    array<f32,6>(0.0234,-0.1328,0.5903,0.6323,-0.1387,0.0254),
    array<f32,6>(0.0225,-0.1299,0.5693,0.6528,-0.1411,0.0264),
    array<f32,6>(0.0215,-0.1260,0.5479,0.6724,-0.1431,0.0273),
    array<f32,6>(0.0200,-0.1226,0.5264,0.6929,-0.1445,0.0278),
    array<f32,6>(0.0190,-0.1187,0.5049,0.7119,-0.1460,0.0288),
    array<f32,6>(0.0181,-0.1147,0.4829,0.7314,-0.1470,0.0293),
    array<f32,6>(0.0166,-0.1104,0.4614,0.7505,-0.1479,0.0298),
    array<f32,6>(0.0156,-0.1060,0.4399,0.7681,-0.1479,0.0303),
    array<f32,6>(0.0146,-0.1016,0.4185,0.7856,-0.1479,0.0308),
    array<f32,6>(0.0137,-0.0967,0.3965,0.8022,-0.1470,0.0313),
    array<f32,6>(0.0122,-0.0923,0.3755,0.8193,-0.1460,0.0313),
    array<f32,6>(0.0112,-0.0874,0.3540,0.8354,-0.1445,0.0313),
    array<f32,6>(0.0103,-0.0825,0.3330,0.8506,-0.1426,0.0313),
    array<f32,6>(0.0093,-0.0776,0.3120,0.8657,-0.1401,0.0308),
    array<f32,6>(0.0083,-0.0728,0.2915,0.8789,-0.1367,0.0308),
    array<f32,6>(0.0073,-0.0679,0.2710,0.8926,-0.1333,0.0303),
    array<f32,6>(0.0063,-0.0630,0.2510,0.9058,-0.1294,0.0293),
    array<f32,6>(0.0059,-0.0581,0.2310,0.9170,-0.1245,0.0288),
    array<f32,6>(0.0049,-0.0537,0.2119,0.9282,-0.1191,0.0278),
    array<f32,6>(0.0044,-0.0488,0.1929,0.9390,-0.1138,0.0264),
    array<f32,6>(0.0034,-0.0439,0.1738,0.9487,-0.1074,0.0254),
    array<f32,6>(0.0029,-0.0396,0.1558,0.9575,-0.1006,0.0239),
    array<f32,6>(0.0024,-0.0352,0.1382,0.9648,-0.0928,0.0225),
    array<f32,6>(0.0020,-0.0308,0.1206,0.9727,-0.0850,0.0205),
    array<f32,6>(0.0015,-0.0264,0.1040,0.9785,-0.0762,0.0186),
    array<f32,6>(0.0010,-0.0220,0.0874,0.9844,-0.0674,0.0166),
    array<f32,6>(0.0005,-0.0181,0.0713,0.9897,-0.0576,0.0142),
    array<f32,6>(0.0005,-0.0142,0.0562,0.9932,-0.0474,0.0117),
    array<f32,6>(0.0005,-0.0103,0.0415,0.9956,-0.0361,0.0088),
    array<f32,6>(0.0,-0.0068,0.0269,0.9985,-0.0249,0.0063),
    array<f32,6>(0.0,-0.0034,0.0132,1.0000,-0.0127,0.0029),
);

// NIS 1.0.3 filter bank, 64 phases and six taps.
const nis_usm = array<array<f32,6>,64>(
    array<f32,6>(0.0,-0.6001,1.2002,-0.6001,0.0,0.0),
    array<f32,6>(0.0029,-0.6084,1.1987,-0.5903,-0.0029,0.0),
    array<f32,6>(0.0049,-0.6147,1.1958,-0.5791,-0.0068,0.0005),
    array<f32,6>(0.0073,-0.6196,1.1890,-0.5659,-0.0103,0.0),
    array<f32,6>(0.0093,-0.6235,1.1802,-0.5513,-0.0151,0.0),
    array<f32,6>(0.0112,-0.6265,1.1699,-0.5352,-0.0195,0.0005),
    array<f32,6>(0.0122,-0.6270,1.1582,-0.5181,-0.0259,0.0005),
    array<f32,6>(0.0142,-0.6284,1.1455,-0.5005,-0.0317,0.0005),
    array<f32,6>(0.0156,-0.6265,1.1274,-0.4790,-0.0386,0.0005),
    array<f32,6>(0.0166,-0.6235,1.1089,-0.4570,-0.0454,0.0010),
    array<f32,6>(0.0176,-0.6187,1.0879,-0.4346,-0.0532,0.0010),
    array<f32,6>(0.0181,-0.6138,1.0659,-0.4102,-0.0615,0.0015),
    array<f32,6>(0.0190,-0.6069,1.0405,-0.3843,-0.0698,0.0015),
    array<f32,6>(0.0195,-0.6006,1.0161,-0.3574,-0.0796,0.0020),
    array<f32,6>(0.0200,-0.5928,0.9893,-0.3286,-0.0898,0.0024),
    array<f32,6>(0.0200,-0.5820,0.9580,-0.2988,-0.1001,0.0029),
    array<f32,6>(0.0200,-0.5728,0.9292,-0.2690,-0.1104,0.0034),
    array<f32,6>(0.0200,-0.5620,0.8975,-0.2368,-0.1226,0.0039),
    array<f32,6>(0.0205,-0.5498,0.8643,-0.2046,-0.1343,0.0044),
    array<f32,6>(0.0200,-0.5371,0.8301,-0.1709,-0.1465,0.0049),
    array<f32,6>(0.0195,-0.5239,0.7944,-0.1367,-0.1587,0.0054),
    array<f32,6>(0.0195,-0.5107,0.7598,-0.1021,-0.1724,0.0059),
    array<f32,6>(0.0190,-0.4966,0.7231,-0.0649,-0.1865,0.0063),
    array<f32,6>(0.0186,-0.4819,0.6846,-0.0288,-0.1997,0.0068),
    array<f32,6>(0.0186,-0.4668,0.6460,0.0093,-0.2144,0.0073),
    array<f32,6>(0.0176,-0.4507,0.6055,0.0479,-0.2290,0.0083),
    array<f32,6>(0.0171,-0.4370,0.5693,0.0859,-0.2446,0.0088),
    array<f32,6>(0.0161,-0.4199,0.5283,0.1255,-0.2598,0.0098),
    array<f32,6>(0.0161,-0.4048,0.4883,0.1655,-0.2754,0.0103),
    array<f32,6>(0.0151,-0.3887,0.4497,0.2041,-0.2910,0.0107),
    array<f32,6>(0.0142,-0.3711,0.4072,0.2446,-0.3066,0.0117),
    array<f32,6>(0.0137,-0.3555,0.3672,0.2852,-0.3228,0.0122),
    array<f32,6>(0.0132,-0.3394,0.3262,0.3262,-0.3394,0.0132),
    array<f32,6>(0.0122,-0.3228,0.2852,0.3672,-0.3555,0.0137),
    array<f32,6>(0.0117,-0.3066,0.2446,0.4072,-0.3711,0.0142),
    array<f32,6>(0.0107,-0.2910,0.2041,0.4497,-0.3887,0.0151),
    array<f32,6>(0.0103,-0.2754,0.1655,0.4883,-0.4048,0.0161),
    array<f32,6>(0.0098,-0.2598,0.1255,0.5283,-0.4199,0.0161),
    array<f32,6>(0.0088,-0.2446,0.0859,0.5693,-0.4370,0.0171),
    array<f32,6>(0.0083,-0.2290,0.0479,0.6055,-0.4507,0.0176),
    array<f32,6>(0.0073,-0.2144,0.0093,0.6460,-0.4668,0.0186),
    array<f32,6>(0.0068,-0.1997,-0.0288,0.6846,-0.4819,0.0186),
    array<f32,6>(0.0063,-0.1865,-0.0649,0.7231,-0.4966,0.0190),
    array<f32,6>(0.0059,-0.1724,-0.1021,0.7598,-0.5107,0.0195),
    array<f32,6>(0.0054,-0.1587,-0.1367,0.7944,-0.5239,0.0195),
    array<f32,6>(0.0049,-0.1465,-0.1709,0.8301,-0.5371,0.0200),
    array<f32,6>(0.0044,-0.1343,-0.2046,0.8643,-0.5498,0.0205),
    array<f32,6>(0.0039,-0.1226,-0.2368,0.8975,-0.5620,0.0200),
    array<f32,6>(0.0034,-0.1104,-0.2690,0.9292,-0.5728,0.0200),
    array<f32,6>(0.0029,-0.1001,-0.2988,0.9580,-0.5820,0.0200),
    array<f32,6>(0.0024,-0.0898,-0.3286,0.9893,-0.5928,0.0200),
    array<f32,6>(0.0020,-0.0796,-0.3574,1.0161,-0.6006,0.0195),
    array<f32,6>(0.0015,-0.0698,-0.3843,1.0405,-0.6069,0.0190),
    array<f32,6>(0.0015,-0.0615,-0.4102,1.0659,-0.6138,0.0181),
    array<f32,6>(0.0010,-0.0532,-0.4346,1.0879,-0.6187,0.0176),
    array<f32,6>(0.0010,-0.0454,-0.4570,1.1089,-0.6235,0.0166),
    array<f32,6>(0.0005,-0.0386,-0.4790,1.1274,-0.6265,0.0156),
    array<f32,6>(0.0005,-0.0317,-0.5005,1.1455,-0.6284,0.0142),
    array<f32,6>(0.0005,-0.0259,-0.5181,1.1582,-0.6270,0.0122),
    array<f32,6>(0.0005,-0.0195,-0.5352,1.1699,-0.6265,0.0112),
    array<f32,6>(0.0,-0.0151,-0.5513,1.1802,-0.6235,0.0093),
    array<f32,6>(0.0,-0.0103,-0.5659,1.1890,-0.6196,0.0073),
    array<f32,6>(0.0005,-0.0068,-0.5791,1.1958,-0.6147,0.0049),
    array<f32,6>(0.0,-0.0029,-0.5903,1.1987,-0.6084,0.0029),
);
