// WebGPU renderer for the hero.
// Passes: physical cards (sharp resting text, intensity-gated cracks, clean emerald resolve) →
// SDF Bean head + Super Saiyan + Tommy-gun spin vortex + dizzy stars →
// 4-vertex Voronoi shards → motion-blurred words → rings/flashes/arcs/checkmark/muzzle/tracers → compute sparks & shell casings.
import { fx as shoutFx } from "./fx.js"

const COMMON = /* wgsl */ `
struct U {
  view: vec4f,  // w, h, shakeX, shakeY
  misc: vec4f,  // time, dt, shutter, roll
  h0: vec4f,    // cx, cy, R, tilt
  h1: vec4f,    // sx, sy, anger, mouth
  h2: vec4f,    // gazeX, gazeY, blink, happy
  h3: vec4f,    // faceX, faceY, browUp, mouthW
  h4: vec4f,    // glow, visible, flicker, rim
  phys: vec4f,  // shardDrag, shardGravity, sparkDrag, sparkGravity
  phys2: vec4f, // sparkStretch, aberration, heatHaze, burstAge
  h5: vec4f,    // lid, squint, browAsym, vein
  h6: vec4f,    // saiyan, pupilScale, mouthRound, jawSkew
  h7: vec4f,    // tongue, voiceOpen, speaking, variant (0=bean, 1=kit)
  h8: vec4f,    // googlyLX, googlyLY, googlyRX, googlyRY
  h9: vec4f,    // finaleSpin, finaleAngle, dizzy, hairX
  h10: vec4f,   // hairY, intensity, finalePower, -
  h11: vec4f,   // visemeFrom, visemeTo, visemeMix, saiyanFrame
  l0: vec4f,    // explosion light 0: x, y, r, k
  l1: vec4f,    // explosion light 1: x, y, r, k
}
@group(0) @binding(0) var<uniform> u: U;
const CORNERS = array<vec2f, 6>(vec2f(-1, -1), vec2f(1, -1), vec2f(-1, 1), vec2f(-1, 1), vec2f(1, -1), vec2f(1, 1));
fn rot(a: f32) -> mat2x2f { let c = cos(a); let s = sin(a); return mat2x2f(c, s, -s, c); }
fn toClip(p0: vec2f) -> vec4f {
  let c = u.view.xy * 0.5;
  let p = rot(u.misc.w) * (p0 - c) + c + u.view.zw;
  return vec4f(p.x / u.view.x * 2.0 - 1.0, 1.0 - p.y / u.view.y * 2.0, 0.0, 1.0);
}
fn ign(p: vec2f) -> f32 { return fract(52.9829189 * fract(dot(p, vec2f(0.06711056, 0.00583715)))); }
fn hash21(p: vec2f) -> f32 {
  var q = fract(p * vec2f(123.34, 456.21));
  q = q + dot(q, q + 45.32);
  return fract(q.x * q.y);
}
fn vnoise(p: vec2f) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let u2 = f * f * (3.0 - 2.0 * f);
  let a = hash21(i);
  let b = hash21(i + vec2f(1.0, 0.0));
  let c = hash21(i + vec2f(0.0, 1.0));
  let d = hash21(i + vec2f(1.0, 1.0));
  return mix(mix(a, b, u2.x), mix(c, d, u2.x), u2.y);
}
fn fbm(p0: vec2f) -> f32 {
  var p = p0;
  var v = 0.55 * vnoise(p);
  p = mat2x2f(1.6, -1.2, 1.2, 1.6) * p;
  v = v + 0.30 * vnoise(p);
  p = mat2x2f(1.6, -1.2, 1.2, 1.6) * p;
  v = v + 0.15 * vnoise(p);
  return v;
}
fn ember(h0: f32) -> vec3f {
  let h = clamp(h0, 0.0, 1.0);
  let c0 = vec3f(0.38, 0.02, 0.01);
  let c1 = vec3f(0.98, 0.17, 0.05);
  let c2 = vec3f(1.00, 0.62, 0.18);
  let c3 = vec3f(1.00, 0.97, 0.90);
  if (h < 0.333) { return mix(c0, c1, h * 3.0); }
  if (h < 0.666) { return mix(c1, c2, (h - 0.333) * 3.0); }
  return mix(c2, c3, (h - 0.666) * 3.0);
}
fn saiyanCol(h0: f32) -> vec3f {
  let h = clamp(h0, 0.0, 1.0);
  let c0 = vec3f(0.88, 0.14, 0.02);
  let c1 = vec3f(1.00, 0.56, 0.06);
  let c2 = vec3f(1.00, 0.88, 0.26);
  let c3 = vec3f(1.00, 0.99, 0.92);
  if (h < 0.333) { return mix(c0, c1, h * 3.0); }
  if (h < 0.666) { return mix(c1, c2, (h - 0.333) * 3.0); }
  return mix(c2, c3, (h - 0.666) * 3.0);
}
`

// ─── 1. Physical Cards Shader (Razor-Sharp Resting Text, Gated Cracks, Clean Resolve) ───
const PROBLEMS = /* wgsl */ `
${COMMON}
struct Prob {
  pos: vec2f, hs: vec2f,
  rot: f32, alpha: f32, flash: f32, dmg: f32,
  vel: vec2f, tilt: vec2f,
  uvr: vec4f,
  imp: vec4f, // impX, impY, rippleAge, seed (+100 if resolved)
}
@group(0) @binding(1) var<storage, read> ps: array<Prob>;
@group(0) @binding(2) var tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
struct VO {
  @builtin(position) pos: vec4f,
  @location(0) world: vec2f,
  @location(1) ppos: vec2f,
  @location(2) hs: vec2f,
  @location(3) vel: vec2f,
  @location(4) tilt: vec2f,
  @location(5) uvr: vec4f,
  @location(6) b: vec4f,   // rot, alpha, flash, dmg
  @location(7) imp: vec4f, // impX, impY, ripple, seed
}
@vertex fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
  var o: VO;
  let p = ps[ii];
  let c = CORNERS[vi];
  // Only expand quad for motion blur when card is actually flying off-screen (sent/resolving)
  let speed = length(p.vel);
  let blurFactor = smoothstep(260.0, 900.0, speed);
  let pad = length(p.vel * u.misc.z) * 0.45 * blurFactor + 8.0;
  let w = p.pos + rot(p.rot) * (c * (p.hs + vec2f(pad)));
  o.pos = toClip(w);
  o.world = w;
  o.ppos = p.pos;
  o.hs = p.hs;
  o.vel = p.vel * blurFactor;
  o.tilt = p.tilt;
  o.uvr = p.uvr;
  o.b = vec4f(p.rot, p.alpha, p.flash, p.dmg);
  o.imp = p.imp;
  return o;
}
@fragment fn fs(i: VO) -> @location(0) vec4f {
  let mi = rot(-i.b.x);
  let lp0 = mi * (i.world - i.ppos);
  let q0 = lp0 / i.hs;

  // Subtle 2.5D perspective foreshortening from impact tilt (kept gentle so text never distorts)
  let tilt = i.tilt;
  let persp = max(0.84, 1.0 + dot(q0, tilt) * 0.12);
  let q = q0 / persp;

  let isResolved = i.imp.w >= 50.0;
  let seed = select(i.imp.w, i.imp.w - 100.0, isResolved);
  let dmg = select(i.b.w, 0.0, isResolved);
  let impPos = i.imp.xy;
  let rippleAge = i.imp.z;
  let aspect = i.hs.x / max(i.hs.y, 1.0);
  let dImpVec = (q - impPos) * vec2f(aspect, 1.0);
  let rImp = length(dImpVec);
  let angImp = atan2(dImpVec.y, dImpVec.x);

  // Localized shock ripple wave expanding from impact point
  let waveFront = rippleAge * 1.65;
  let waveEnv = exp(-pow((rImp - waveFront) * 9.5, 2.0)) * clamp(1.0 - rippleAge * 1.15, 0.0, 1.0);
  let wave = waveEnv * (0.25 + 0.65 * i.b.z) * select(1.0, 0.0, isResolved);
  let rDir = dImpVec / max(rImp, 1e-3);
  let uvRipple = rDir * wave * 0.008;

  // Single crisp sample for resting cards so text is 100% razor-sharp!
  // Only multi-tap when the card is gliding/flying off-screen (length(i.vel) > 10).
  let vLen = length(i.vel);
  var t = vec4f(0.0);
  if (vLen < 10.0) {
    let u0 = q * 0.5 + 0.5 + uvRipple;
    if (u0.x >= 0.0 && u0.x <= 1.0 && u0.y >= 0.0 && u0.y <= 1.0) {
      let uv = mix(i.uvr.xy, i.uvr.zw, u0);
      t = textureSampleLevel(tex, samp, uv, 0.0);
    }
  } else {
    let N = 6;
    let j = ign(i.pos.xy);
    let bl = (mi * i.vel) * u.misc.z;
    for (var k = 0; k < N; k++) {
      let f = (f32(k) + j) / f32(N) - 0.5;
      let lp = (lp0 - bl * f) / persp;
      let u0 = (lp / i.hs) * 0.5 + 0.5 + uvRipple;
      if (u0.x >= 0.0 && u0.x <= 1.0 && u0.y >= 0.0 && u0.y <= 1.0) {
        let uv = mix(i.uvr.xy, i.uvr.zw, u0);
        t = t + textureSampleLevel(tex, samp, uv, 0.0);
      }
    }
    t = t / f32(N);
  }
  if (t.a <= 0.001) { return vec4f(0.0); }

  var rgb = t.rgb;
  let bodyMask = smoothstep(0.95, 0.80, max(abs(q.x), abs(q.y)));

  // Procedural radial & concentric spiderweb fracture cracks (gated by dmg > 0.02, localized near impact)
  var crackGlow = 0.0;
  if (dmg > 0.02 && bodyMask > 0.01) {
    let reach = min(1.15, dmg * 0.88);
    let radialReach = smoothstep(reach, 0.0, rImp);
    let spokes = 8.0;
    let warp1 = 0.20 * sin(rImp * 18.0 + seed * 6.1) + 0.10 * cos(rImp * 37.0 - seed * 3.7);
    let spokeDist = abs( fract((angImp + warp1) * spokes / 6.2831853) - 0.5 ) * 2.0 * (rImp + 0.05);
    let spokeMask = (1.0 - smoothstep(0.004, 0.018, spokeDist)) * radialReach;

    let ringPhase = fract(rImp * 6.5 + 0.20 * sin(angImp * 5.0 + seed * 4.3));
    let ringDist = abs(ringPhase - 0.5) * 2.0;
    let ringGate = step(0.42, hash21(vec2f(floor(rImp * 6.5), floor(angImp * 4.0 + seed))));
    let ringMask = (1.0 - smoothstep(0.03, 0.11, ringDist)) * radialReach * ringGate * smoothstep(0.05, 0.14, rImp);

    let crackLine = clamp(spokeMask + ringMask * 0.78, 0.0, 1.0) * min(1.0, dmg * 1.15);
    let hotLevel = clamp(i.b.z * 0.8 + u.h6.x * 0.55 + (dmg - 0.35) * 0.45, 0.0, 1.0);
    let crackCol = select(ember(0.45 + 0.45 * hotLevel), saiyanCol(0.5 + 0.45 * hotLevel), u.h6.x > 0.25);

    rgb = mix(rgb, rgb * 0.35 + crackCol * (0.42 + 0.75 * hotLevel), crackLine * 0.72 * t.a);
    crackGlow = crackLine;
  }

  // Localized impact flash OR clean OpenCode emerald resolution glow
  if (isResolved) {
    let rim = smoothstep(0.68, 0.91, max(abs(q.x) / 0.90, abs(q.y) / 0.80));
    let mintCol = vec3f(0.22, 0.96, 0.62);
    rgb = rgb + mintCol * (rim * 0.55 + i.b.z * 0.22) * bodyMask * t.a;
  } else {
    let flashCore = exp(-rImp * rImp * 8.5) * i.b.z;
    let flashRim = smoothstep(0.68, 0.92, max(abs(q.x) / 0.88, abs(q.y) / 0.78)) * i.b.z * 0.32;
    let flashCol = select(vec3f(1.0, 0.65, 0.38), vec3f(1.0, 0.88, 0.45), u.h6.x > 0.25);
    rgb = rgb + flashCol * (flashCore * 0.62 + wave * 0.38 + flashRim) * bodyMask * t.a;
  }

  // Dynamic light cast by the Super Saiyan aura onto facing card bevels ONLY (leaves interior text untouched!)
  let auraPower = clamp(u.h1.z * 0.38 + u.h6.x * 1.05, 0.0, 1.45);
  if (auraPower > 0.02 && bodyMask > 0.01 && !isResolved) {
    let toHead = (i.world - u.h0.xy) / max(u.h0.z, 1.0);
    let dHeadR = length(toHead);
    let lightDir = normalize(-toHead);
    let cardNrm = normalize(q * vec2f(1.0, aspect) + vec2f(1e-4));
    let facing = pow(max(0.0, dot(cardNrm, lightDir)), 1.55);
    let edgeBand = smoothstep(0.80, 0.93, max(abs(q.x) / 0.90, abs(q.y) / 0.80));
    let atten = exp(-max(0.0, dHeadR - 0.85) * 0.52);
    let flicker = 0.90 + 0.10 * sin(u.misc.x * 34.0 + dHeadR * 2.0);
    let castCol = mix(vec3f(1.0, 0.26, 0.08), vec3f(1.0, 0.74, 0.18), clamp(u.h6.x * 1.1, 0.0, 1.0));
    let rimLight = (edgeBand * (0.15 + 0.85 * facing) * 0.78 + crackGlow * 0.28) * auraPower * atten * flicker;
    rgb = rgb + castCol * rimLight * bodyMask * t.a;
  }

  // Dynamic light cast by nearby explosions (u.l0, u.l1) onto facing cards
  for (var li = 0; li < 2; li = li + 1) {
    let L = select(u.l0, u.l1, li == 1);
    if (L.w > 0.01 && bodyMask > 0.01 && !isResolved) {
      let toCard = i.world - L.xy;
      let dist = length(toCard);
      let atten = exp(-dist / max(L.z, 1.0) * 1.55) * L.w;
      if (atten > 0.005) {
        let lDir = -toCard / max(dist, 1.0);
        let cardNrm = normalize(q * vec2f(1.0, aspect) + vec2f(1e-4));
        let face = 0.25 + 0.75 * max(0.0, dot(cardNrm, lDir));
        let edgeBoost = smoothstep(0.55, 0.93, max(abs(q.x) / 0.90, abs(q.y) / 0.80));
        let lCol = select(vec3f(1.0, 0.58, 0.18), vec3f(1.0, 0.84, 0.28), u.h6.x > 0.25);
        rgb = rgb + lCol * atten * (edgeBoost * face * 0.95 + face * 0.30) * bodyMask * t.a;
      }
    }
  }

  // Specular sheen when tilted in 3D
  let tiltMag = clamp(length(tilt), 0.0, 0.6);
  let specSweep = exp(-pow((q.x * 0.7 + q.y * 0.5 - (tilt.x - tilt.y) * 0.65) * 3.2, 2.0)) * tiltMag * 0.14;
  rgb = rgb + vec3f(0.92, 0.95, 1.0) * specSweep * bodyMask * t.a;

  return vec4f(rgb, t.a) * i.b.y;
}
`

// ─── 2. Head Shader: Classic Glossy Bean OR ?head=kit 2.5D Projection-Mapped Portrait + Googly Physics + Spin Vortex ───
const HEAD = /* wgsl */ `
${COMMON}
struct VO { @builtin(position) pos: vec4f, @location(0) q: vec2f }
@vertex fn vs(@builtin(vertex_index) vi: u32) -> VO {
  var o: VO;
  let c = CORNERS[vi];
  let R = u.h0.z;
  let w = u.h0.xy + c * R * 3.1;
  o.pos = toClip(w);
  o.q = c * 3.1;
  return o;
}
fn sdEllipse(p: vec2f, r: vec2f) -> f32 {
  let k0 = length(p / r);
  let k1 = length(p / (r * r));
  return k0 * (k0 - 1.0) / max(k1, 1e-4);
}
fn sdSegment(p: vec2f, a: vec2f, b: vec2f) -> f32 {
  let pa = p - a; let ba = b - a;
  let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
  return length(pa - ba * h);
}
fn sdRoundBox(p: vec2f, b: vec2f, r: f32) -> f32 {
  let q = abs(p) - b + vec2f(r);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}
fn cov(d: f32, aa: f32) -> f32 { return 1.0 - smoothstep(-aa, aa, d); }

// 4-point arcade star SDF for dizzy post-spin halo
fn sdStar4(p0: vec2f, r: f32) -> f32 {
  let p = abs(p0);
  return (p.x + p.y - r) / 1.4142 + max(p.x, p.y) * 0.32 - r * 0.22;
}

// Sample a single 512x512 cell (0..15) from the 2048x2048 4x4 Kit sprite atlas (with spin motion blur)
fn evalMouth(mp: vec2f, mw: f32, mh: f32, mouth: f32, smile: f32, roundness: f32, anger: f32, saiyan: f32, tongueVib: f32, aa: f32, isKit: bool) -> vec4f {
  let mux = clamp(mp.x / max(mw, 0.01), -1.0, 1.0);
  let closedCurveFactor = 1.0 - smoothstep(0.0, 0.38, mouth);
  let yMid = smile * 0.038 * (1.0 - mux * mux) * closedCurveFactor;
  let archProfile = pow(max(0.0, 1.0 - mux * mux), mix(0.62, 0.88, roundness));
  let topArch = mh * (0.80 + 0.16 * roundness) * archProfile;
  let botArch = mh * (0.94 + 0.12 * smoothstep(0.35, 1.0, mouth)) * pow(max(0.0, 1.0 - mux * mux), 0.56);
  let yTop = yMid - topArch;
  let yBot = yMid + botArch;
  let dMouthY = max(yTop - mp.y, mp.y - yBot);
  let dMouthX = abs(mp.x) - mw;
  let dCavity = max(dMouthX, dMouthY) + 0.016 * (1.0 - smoothstep(0.0, 0.12, mouth));

  let openBlend = smoothstep(0.010, 0.048, mouth);
  let lineCap = smoothstep(mw + aa, mw - aa, abs(mp.x));
  let dSmileLine = abs(mp.y - yMid) - (0.014 + 0.008 * mouth);
  let closedStrokeMask = cov(dSmileLine, aa) * lineCap * (1.0 - openBlend);
  let openCavityMask = cov(dCavity, aa) * openBlend;

  // Roar factor: in Calm stage (low anger & small mouth), keep mouth friendly (no deep horror furnace/uvula/bared lower teeth)
  let roarFactor = smoothstep(0.32, 0.65, max(mouth, anger * 0.85 + saiyan));

  // Throat background: warm dark plum in calm speech, glowing furnace in rage/Saiyan
  var mcol = mix(vec3f(0.16, 0.03, 0.05), vec3f(0.08, 0.01, 0.015), smoothstep(yBot, yTop, mp.y));
  let throatPos = (mp - vec2f(0.0, mh * 0.08)) / max(vec2f(mw * 0.72, mh * 0.68), vec2f(0.02));
  let furnace = exp(-dot(throatPos, throatPos) * 1.45) * clamp(anger * 0.85 + saiyan * 1.35, 0.0, 1.5) * roarFactor;
  let fireCol = select(ember(0.35 + 0.62 * furnace), saiyanCol(0.4 + 0.6 * furnace), saiyan > 0.25);
  mcol = mcol + fireCol * furnace * 0.85;

  // Uvula at top-center of palate only during big yells/roars
  if (roarFactor > 0.15 && mh > 0.16) {
    let uvulaSway = sin(tongueVib * 6.28318) * 0.012 * (anger + saiyan);
    let uvulaP = mp - vec2f(uvulaSway, yTop + 0.045 * mh);
    let dUvula = sdEllipse(uvulaP, vec2f(0.032, 0.082 * min(1.0, mh * 2.4)));
    let uvMask = cov(dUvula, aa) * roarFactor;
    let uvCol = mix(vec3f(0.45, 0.06, 0.09), fireCol * 0.9, clamp(furnace * 0.6, 0.0, 1.0));
    mcol = mix(mcol, uvCol, uvMask);
  }

  // Bi-lobed tongue with central groove & wet highlight
  let tWobble = sin(tongueVib * 6.28318) * 0.010 * (0.2 + anger + saiyan);
  let tY = yBot + 0.015 - mh * (0.22 + 0.08 * roarFactor) + tWobble;
  let lobeL = sdEllipse(mp - vec2f(-mw * 0.22, tY), vec2f(mw * 0.46, max(0.03, mh * 0.44)));
  let lobeR = sdEllipse(mp - vec2f(mw * 0.22, tY), vec2f(mw * 0.46, max(0.03, mh * 0.44)));
  let dTongue = min(lobeL, lobeR);
  let groove = exp(-pow(mp.x / max(mw * 0.12, 0.01), 2.0)) * smoothstep(yBot, tY - mh * 0.25, mp.y);
  let tongueGrad = clamp((mp.y - (tY - mh * 0.35)) / max(mh * 0.5, 0.02), 0.0, 1.0);
  var tongueCol = mix(vec3f(0.94, 0.34, 0.38), vec3f(0.64, 0.11, 0.15), tongueGrad);
  tongueCol = tongueCol * (1.0 - 0.30 * groove);
  let tSpec = exp(-pow((mp.x - mw * 0.14) / max(mw * 0.14, 0.01), 2.0) - pow((mp.y - (tY - mh * 0.12)) / max(mh * 0.10, 0.01), 2.0));
  tongueCol = tongueCol + vec3f(1.0, 0.85, 0.86) * tSpec * 0.35;
  tongueCol = tongueCol + fireCol * furnace * 0.22;
  mcol = mix(mcol, tongueCol, cov(dTongue, aa) * smoothstep(0.03, 0.12, mouth));

  // Upper teeth arch with subtle individual tooth notches
  let toothH = min(0.068, mh * 0.32);
  let toothNotch = 0.005 * pow(abs(sin(mux * 3.14159 * 3.5)), 3.0);
  let dUpperTeeth = mp.y - (yTop + toothH - toothNotch);
  let toothShade = 1.0 - 0.18 * pow(abs(mux), 1.8) - 0.10 * clamp(abs(sin(mux * 3.14159 * 3.5)), 0.0, 1.0);
  let toothCol = vec3f(0.97, 0.97, 0.96) * toothShade;
  mcol = mix(mcol, toothCol, cov(dUpperTeeth, aa) * smoothstep(0.04, 0.12, mouth));

  // Lower teeth arch (only during loud shouts/roars)
  let botToothH = min(0.048, mh * 0.20) * roarFactor;
  if (botToothH > 0.004) {
    let dLowerTeeth = (yBot - botToothH + toothNotch * 0.8) - mp.y;
    let botMask = cov(dLowerTeeth, aa) * smoothstep(mw * 0.86, mw * 0.68, abs(mp.x));
    mcol = mix(mcol, toothCol * 0.92, botMask);
  }

  // Cavity rim / inner lip shadow
  let cavityAO = smoothstep(0.0, 0.035, -dCavity);
  mcol = mcol * (0.65 + 0.35 * cavityAO);

  // Subtle outer lip rim
  let dLipBorder = abs(dCavity) - (0.011 + 0.005 * roundness);
  let lipMask = cov(dLipBorder, aa) * openBlend;
  let lipCol = select(
    mix(vec3f(0.86, 0.86, 0.88), vec3f(1.0, 0.52, 0.36), clamp(anger + saiyan * 0.5, 0.0, 1.0)),
    mix(vec3f(0.58, 0.28, 0.26), vec3f(0.88, 0.42, 0.32), clamp(anger + saiyan * 0.5, 0.0, 1.0)),
    isKit
  );

  let totalMask = clamp(openCavityMask + closedStrokeMask + lipMask * 0.55, 0.0, 1.0);
  var finalCol = mix(mcol, lipCol, lipMask * (1.0 - openCavityMask * 0.65));
  let closedCol = select(vec3f(0.95, 0.95, 0.93), vec3f(0.32, 0.14, 0.13), isKit);
  finalCol = mix(finalCol, closedCol, closedStrokeMask);
  return vec4f(finalCol, totalMask);
}

@fragment fn fs(i: VO) -> @location(0) vec4f {
  let scale = u.h1.xy;
  let qScaled = i.q / scale;
  let p = rot(-u.h0.w) * qScaled;
  let aa = max(fwidth(p.x), fwidth(p.y)) * 1.15;
  let quadFade = smoothstep(3.1, 2.25, length(i.q));

  let anger = clamp(u.h1.z, 0.0, 1.0);
  let mouth = clamp(u.h1.w, 0.0, 1.35);
  let saiyan = clamp(u.h6.x, 0.0, 1.35);
  let pupilScale = clamp(u.h6.y, 0.35, 1.25);
  let roundness = clamp(u.h6.z, 0.0, 1.0);
  let jawSkew = u.h6.w;
  let tongueVib = u.h7.x;
  let isKit = false; // the sprite-rig head variant was left in the prototype
  let time = u.misc.x;
  let face = u.h3.xy;
  let lid = u.h5.x;
  let squint = u.h5.y;
  let asym = u.h5.z;
  let vein = u.h5.w;
  let smile = u.h2.w;
  let wide = clamp(u.h3.w, 0.48, 1.35);
  let finaleSpin = u.h9.x;
  let finaleAngle = u.h9.y;
  let dizzy = clamp(u.h9.z, 0.0, 1.0);
  let hairBounce = vec2f(u.h9.w, u.h10.x);

  // ─── Roar jaw stretch (gated so calm lines stay relaxed & round!) ─────────
  let roarStretch = smoothstep(0.35, 0.85, max(mouth, anger * 0.8 + saiyan));
  let jaw = smoothstep(-0.08, 0.95, p.y) * mouth * roarStretch;
  let cheekPinch = exp(-pow((p.y - 0.12) / 0.34, 2.0)) * smoothstep(0.35, 1.0, mouth) * 0.08 * roarStretch;
  let rx = (select(0.80, 0.92, isKit)) * (1.0 - 0.08 * p.y) - cheekPinch + 0.12 * jaw;
  let ry = select(1.0, 1.0 + 0.20 * mouth * roarStretch, p.y > 0.0);
  let pHead = vec2f(p.x - jawSkew * 0.03 * jaw, p.y - 0.05 * mouth * roarStretch);
  let dHead = sdEllipse(pHead, vec2f(rx, ry));

  var col = vec3f(0.0);
  var cover = 0.0;

  {
    cover = cov(dHead, aa);

    let nx = clamp(pHead.x / max(rx, 0.1) * 0.86, -0.96, 0.96);
    let ny = clamp(pHead.y / max(ry, 0.1) * 0.86, -0.96, 0.96);
    let nz = sqrt(max(0.03, 1.0 - nx * nx - ny * ny));
    let N = normalize(vec3f(nx, ny, nz));

    let studioLift = clamp(0.52 - 0.36 * N.y - 0.28 * N.x, 0.0, 1.0);
    col = mix(vec3f(0.018, 0.018, 0.022), vec3f(0.11, 0.115, 0.135), pow(studioLift, 1.5));

    // Subsurface heat rising from the jaw
    let heatRise = smoothstep(-0.72 + (1.0 - max(anger, saiyan * 0.85)) * 1.25, 0.92, p.y);
    let subCol = mix(ember(0.28 + 0.48 * heatRise), saiyanCol(0.35 + 0.55 * heatRise), clamp(saiyan, 0.0, 1.0));
    col = mix(col, subCol, clamp(anger * 0.88 + saiyan * 0.55, 0.0, 0.92) * pow(heatRise, 1.15));

    // Studio Softbox Window Reflection in (rimAng, -dHead) space
    let rimAng = atan2(pHead.y, pHead.x);
    let uArc = (rimAng + 2.08) * 0.76;
    let vArc = (-dHead) - 0.115;
    let dWindow = sdRoundBox(vec2f(uArc, vArc), vec2f(0.21, 0.028), 0.026);
    let winMask = (1.0 - smoothstep(-0.008, 0.015, dWindow)) * smoothstep(0.0, -0.18, pHead.y);
    let dCatch = sdRoundBox(vec2f(uArc + 0.29, vArc), vec2f(0.036, 0.024), 0.022);
    let catchMask = (1.0 - smoothstep(-0.008, 0.014, dCatch)) * 0.80;
    let satinSheen = exp(-(uArc * uArc) / 0.14 - (vArc * vArc) / 0.012) * 0.20;
    let specFlicker = 1.0 + 0.14 * sin(time * 37.0) * clamp(anger * 0.5 + saiyan, 0.0, 1.0);
    let specTone = mix(
      vec3f(0.95, 0.97, 1.0),
      mix(vec3f(1.0, 0.82, 0.76), vec3f(1.0, 0.94, 0.76), clamp(saiyan, 0.0, 1.0)),
      clamp(anger * 0.45 + saiyan * 0.65, 0.0, 1.0)
    );
    col = col + specTone * (winMask * 0.72 + catchMask * 0.56 + satinSheen) * specFlicker;

    // Fresnel Rim
    let rimDist = abs(dHead + 0.018);
    let fresnel = exp(-rimDist * rimDist / (0.0014 + 0.0012 * saiyan)) * u.h4.w;
    let topRimWeight = clamp(0.65 - 0.35 * N.y, 0.2, 1.0);
    let botRimWeight = clamp(0.45 + 0.55 * N.y, 0.0, 1.0);
    let coolRim = vec3f(0.92, 0.94, 0.98) * topRimWeight * 0.78;
    let hotRim = mix(vec3f(1.0, 0.32, 0.12), vec3f(1.0, 0.90, 0.42), clamp(saiyan, 0.0, 1.0)) * (0.45 + 0.65 * botRimWeight) * clamp(anger + saiyan, 0.0, 1.4);
    col = col + (coolRim + hotRim) * fresnel;

    // Eyes & Ballistic Saccades (with counter-rotating dizzy spirals!)
    let eyeBaseOpen = clamp(1.0 - u.h2.z, 0.04, 1.0);
    let eyeWide = 1.0 + 0.18 * smoothstep(0.1, 0.9, saiyan) + 0.08 * clamp(u.h3.z, 0.0, 1.0);
    for (var s = -1; s <= 1; s = s + 2) {
      let side = f32(s);
      let ec = vec2f(side * 0.275, -0.125) + face;
      let ep = p - ec;
      let er = vec2f(0.148, 0.208 * eyeWide);
      let dSocket = sdEllipse(ep, er);

      let browAng = -side * (anger * 0.64 + squint * 0.20 + saiyan * 0.14) + select(0.0, 0.24, s < 0) * asym;
      let lidSlope = tan(clamp(browAng * 0.55, -0.52, 0.52));
      let topCutY = -er.y + (lid * 0.95 + (1.0 - eyeBaseOpen) * 1.65 + anger * 0.26) * er.y + ep.x * lidSlope * 0.65;
      let botCutY = er.y - (squint * 0.72 + (1.0 - eyeBaseOpen) * 0.55 + max(0.0, smile) * 0.20) * er.y - ep.x * ep.x * 1.1;
      let dEyeOpen = max(dSocket, max(topCutY - ep.y, ep.y - botCutY));
      let eyeCov = cov(dEyeOpen, aa);

      let dCrease = abs(ep.y - (topCutY - 0.024)) - 0.008;
      let creaseMask = cov(dCrease, aa) * smoothstep(er.x * 1.05, er.x * 0.5, abs(ep.x)) * (1.0 - eyeCov);
      col = mix(col, col * 0.35, creaseMask * 0.65);

      let scleraTopAO = smoothstep(topCutY - 0.01, topCutY + 0.07, ep.y);
      let scleraSphere = clamp(1.0 - dot(ep / er, ep / er) * 0.28, 0.65, 1.0);
      let scleraBase = mix(vec3f(0.97, 0.97, 0.95), vec3f(1.0, 0.95, 0.86), clamp(anger * 0.4 + saiyan * 0.6, 0.0, 1.0));
      let scleraCol = scleraBase * scleraSphere * (0.72 + 0.28 * scleraTopAO);

      let gaze = mix(vec2f(u.h2.x * 0.062, u.h2.y * 0.072), vec2f(select(u.h8.x, u.h8.z, s > 0) * 0.075, select(u.h8.y, u.h8.w, s > 0) * 0.075), dizzy);
      let pp = ep - gaze;
      let pr = 0.076 * pupilScale;
      let dp = length(pp) - pr;
      let irisR = pr + 0.016 * clamp(anger + saiyan, 0.0, 1.2);
      let dIris = length(pp) - irisR;
      let irisCol = mix(vec3f(0.92, 0.18, 0.06), vec3f(1.0, 0.82, 0.22), clamp(saiyan, 0.0, 1.0));
      var eyeCol = mix(scleraCol, irisCol, cov(dIris, aa) * clamp(anger * 0.85 + saiyan, 0.0, 1.0));
      eyeCol = mix(eyeCol, vec3f(0.03, 0.03, 0.04), cov(dp, aa));

      // Dizzy spiral inside the pupil after Tommy-gun spin
      if (dizzy > 0.08 && dp < 0.01) {
        let rSp = length(pp) / max(pr, 0.01);
        let aSp = atan2(pp.y, pp.x) * side + time * 16.0;
        let spiralWave = sin(rSp * 16.0 - aSp * 2.0);
        let spiralMask = smoothstep(0.3, 0.8, spiralWave) * cov(dp, aa) * dizzy;
        eyeCol = mix(eyeCol, vec3f(0.98, 0.95, 0.82), spiralMask * 0.85);
      }

      let h1d = length(pp - vec2f(-0.022, -0.025) * pupilScale) - 0.019 * max(0.7, pupilScale);
      let h2d = length(pp - vec2f(0.020, 0.022) * pupilScale) - 0.009 * max(0.7, pupilScale);
      eyeCol = mix(eyeCol, vec3f(1.0), clamp(cov(h1d, aa) + cov(h2d, aa) * 0.85, 0.0, 1.0));
      col = mix(col, eyeCol, eyeCov);

      let by = -0.43 - u.h3.z * 0.085 + anger * 0.055 + select(0.0, -0.055, s < 0) * asym - saiyan * 0.02;
      let bc = vec2f(side * 0.275, by) + face;
      let dir = vec2f(cos(browAng), sin(browAng)) * 0.142;
      let bp = p - bc;
      let proj = clamp(dot(bp, dir) / max(dot(dir, dir), 1e-4), -1.0, 1.0);
      let taper = mix(0.044, 0.026, (proj * side + 1.0) * 0.5);
      let db = sdSegment(p, bc - dir, bc + dir) - taper;
      let browCol = mix(vec3f(0.97, 0.97, 0.95), vec3f(1.0, 0.93, 0.65), clamp(saiyan * 0.7, 0.0, 1.0));
      col = mix(col, browCol, cov(db, aa));
    }

    // Continuous Mouth Rig (Calm conversational vs Roar)
    let mc = vec2f(jawSkew * 0.03, 0.31 + 0.18 * mouth * roarStretch) + face * 0.92;
    let mp = p - mc;
    let jawFlare = 1.0 + 0.20 * clamp(mp.y / 0.34, -0.35, 1.0) * roarStretch;
    let effWide = mix(max(0.82, wide), mix(wide, max(wide, 1.15), roarStretch), smoothstep(0.04, 0.22, mouth));
    let mw = (0.165 + 0.255 * mouth) * effWide * jawFlare;
    let mh = (0.24 + 0.12 * roarStretch) * mouth;
    let mOut = evalMouth(mp, mw, mh, mouth, smile, roundness, anger, saiyan, tongueVib, aa, false);
    col = mix(col, mOut.rgb, mOut.a);

  }

  // ─── Dual Anime Anger Veins (bounded to temple radius 0.135 on the Bean; Kit sprites have native sculpted brows/temples) ───
  if (!isKit && vein > 0.01 && cover > 0.1) {
    let pulse = 1.0 + 0.16 * sin(time * 22.0);
    let vCol = mix(vec3f(1.0, 0.28, 0.12), vec3f(1.0, 0.90, 0.42), clamp(saiyan, 0.0, 1.0));
    for (var vIdx = 0; vIdx < 2; vIdx = vIdx + 1) {
      let vSide = select(1.0, -1.0, vIdx == 1);
      let vScale = select(1.0, 0.78, vIdx == 1) * pulse;
      let vp = (rot(-0.28 * vSide) * (p - (vec2f(0.42 * vSide, -0.44) + face * 0.55))) / vScale;
      var vd = 1e5;
      for (var arm = 0; arm < 4; arm = arm + 1) {
        let a = f32(arm) * 1.5707963;
        let q = rot(a) * vp - vec2f(0.052, 0.052);
        let d = max(max(abs(q.x + q.y) * 0.7071 - 0.012, -(length(q) - 0.038)), length(vp) - 0.135);
        vd = min(vd, d);
      }
      let vWeight = select(1.0, smoothstep(0.35, 0.85, vein), vIdx == 1);
      col = mix(col, vCol, cov(vd, aa / vScale) * vein * vWeight * 0.92);
    }
  }

  // ─── Tommy-Gun 360° Spin Vortex Blur & Muzzle Cone (when finaleSpin > 0.3 and not dizzy) ───
  let notDizzy = 1.0 - smoothstep(0.05, 0.35, dizzy);
  let spinDrive = clamp(finaleSpin / 8.5, 0.0, 1.25) * notDizzy;
  var vortexGlow = vec3f(0.0);
  if (spinDrive > 0.04) {
    let rSpin = length(pHead);
    let aSpin = atan2(pHead.y, pHead.x);
    // Horizontal turntable speed bands + azimuthal cyclone streaks across the spinning head
    let hBand = pow(sin(pHead.y * 26.0 + sin(time * 52.0) * 3.0) * 0.5 + 0.5, 3.0) * smoothstep(0.95, 0.2, abs(pHead.x));
    let swirlWave = sin(aSpin * 4.0 - rSpin * 9.0 + time * 48.0) * 0.5 + 0.5;
    let swirlStreaks = pow(swirlWave, 2.2) * smoothstep(0.12, 0.85, rSpin) * smoothstep(1.65, 0.45, rSpin);
    let spinCol = saiyanCol(0.38 + 0.55 * swirlWave);
    col = mix(col, col * 0.52 + spinCol * 0.88 + vec3f(1.0, 0.96, 0.82) * hBand * 0.45, clamp(spinDrive * (0.55 * swirlWave + 0.35 * hBand), 0.0, 0.78));
    vortexGlow = spinCol * (swirlStreaks + hBand * 0.35 * cover) * spinDrive * 1.35;

    // Bright directional Tommy-gun muzzle cone at finaleAngle on the rim
    let dAng = atan2(sin(aSpin - finaleAngle), cos(aSpin - finaleAngle));
    let coneMask = exp(-pow(dAng / 0.32, 2.0)) * smoothstep(0.55, 1.05, rSpin) * smoothstep(2.1, 0.9, rSpin);
    vortexGlow = vortexGlow + vec3f(1.0, 0.94, 0.72) * coneMask * spinDrive * 1.45;
  }

  // ─── Exterior Halo & Super Saiyan Spiky Crown (Sculpted Golden Hair Spikes for Kit!) ───
  let dKitBody = min(
    min(sdEllipse(pHead - vec2f(0.0, -0.22), vec2f(0.72, 0.56)), sdEllipse(pHead - vec2f(0.0, 0.16), vec2f(0.40, 0.50))),
    sdEllipse(pHead - vec2f(0.0, 0.72), vec2f(0.78, 0.24))
  );
  let outside = max(select(dHead, max(dKitBody, (0.5 - cover) * 0.14), isKit), 0.0);
  let low = smoothstep(-0.85, 0.95, p.y);
  let baseHaloCol = mix(vec3f(0.55, 0.58, 0.66) * 0.04, vec3f(1.0, 0.20, 0.06) * (0.24 + 0.38 * low), anger * notDizzy) * u.h4.x;
  var halo = baseHaloCol * exp(-outside * 3.2) + vortexGlow;

  let auraDrive = clamp(saiyan + max(0.0, anger - 0.72) * 0.50 + spinDrive * 0.45, 0.0, 1.35) * notDizzy;
  if (auraDrive > 0.01) {
    let ang = atan2(p.x, -p.y);
    let crownBias = select(0.52 + 0.48 * cos(ang * 0.75), pow(max(0.0, cos(ang * 0.65)), 1.35) * 1.25 + 0.22, isKit);
    let fp1 = vec2f(p.x * 1.75, p.y * 1.10 + time * 4.4);
    let fp2 = vec2f(p.x * 3.10 - sin(time * 7.0 + p.y * 3.0) * 0.25, p.y * 1.85 + time * 7.2);
    let n1 = fbm(fp1);
    let n2 = fbm(fp2);

    let spikeFreq = select(10.0, 12.0, isKit);
    let spikePhase = ang * (spikeFreq / 6.2831853) + (n1 - 0.5) * 0.52 + p.y * 0.16 * sin(ang);
    let spikeTri = 1.0 - abs(fract(spikePhase) - 0.5) * 2.0;
    let spikeSharp = pow(clamp(spikeTri, 0.0, 1.0), select(1.85, 1.55, isKit)) * crownBias;

    let tonguePhase = ang * (18.0 / 6.2831853) - (n2 - 0.5) * 0.62;
    let tongueSharp = pow(1.0 - abs(fract(tonguePhase) - 0.5) * 2.0, 2.1) * crownBias;

    let pulse = 0.90 + 0.14 * sin(time * 28.0 - outside * 4.0);
    let kitAuraScale = select(1.0, 0.42, isKit);
    let reach = (0.14 + 0.95 * spikeSharp * kitAuraScale + 0.42 * tongueSharp * kitAuraScale + 0.24 * n1) * auraDrive * pulse;
    let flameFactor = clamp(1.0 - outside / max(reach, 0.001), 0.0, 1.0);
    let coreFactor = exp(-outside / max(0.08 * auraDrive, 0.005)) * select(1.0, 0.28, isKit);

    let flameHeat = clamp(pow(flameFactor, 0.72) * 0.85 + coreFactor * 0.45, 0.0, 1.0);
    let auraCol = mix(ember(flameHeat), saiyanCol(flameHeat), clamp(saiyan * 1.1 + spinDrive, 0.0, 1.0));
    let flameAlpha = smoothstep(0.0, 0.16, flameFactor) * pow(flameFactor, 0.82) * select(1.15, 0.58, isKit);
    halo = halo + auraCol * (flameAlpha + coreFactor * 0.62 * auraDrive);

    if ((saiyan > 0.12 || spinDrive > 0.2) && outside < reach * 1.25) {
      let boltSeed = floor(time * 18.0);
      let boltWave = sin(ang * 3.0 + boltSeed * 2.1 + (n2 - 0.5) * 3.4) * 0.5 + 0.5;
      let boltRadius = (0.14 + 0.52 * boltWave) * auraDrive;
      let dBolt = abs(outside - boltRadius + (n1 - 0.5) * 0.22);
      let boltSeg = smoothstep(0.35, 0.65, sin(ang * 5.0 + boltSeed * 4.7));
      let boltCore = (1.0 - smoothstep(0.0, 0.016, dBolt)) * boltSeg * max(saiyan, spinDrive);
      let boltGlow = exp(-dBolt * 28.0) * boltSeg * max(saiyan, spinDrive) * 0.65;
      halo = halo + vec3f(0.88, 0.97, 1.0) * boltCore * 1.5 + vec3f(1.0, 0.82, 0.25) * boltGlow;
    }
  }

  // ─── Orbiting Golden Dizzy Stars Above the Crown (when dizzy > 0.04) ─────────
  if (dizzy > 0.04) {
    let ringCenter = vec2f(0.0, -1.06);
    let ringRadii = vec2f(0.68, 0.16);
    let dTrack = abs(sdEllipse(p - ringCenter, ringRadii)) - 0.008;
    halo = halo + vec3f(1.0, 0.82, 0.32) * exp(-max(dTrack, 0.0) * 38.0) * 0.35 * dizzy;
    for (var k = 0; k < 5; k = k + 1) {
      let aStar = time * 4.2 + f32(k) * 1.256637;
      let starPos = ringCenter + vec2f(cos(aStar) * ringRadii.x, sin(aStar) * ringRadii.y);
      let depthScale = 0.78 + 0.28 * sin(aStar);
      let sp = rot(time * 5.0 + f32(k)) * ((p - starPos) / depthScale);
      let dS = sdStar4(sp, 0.085);
      let starCore = cov(dS, aa / depthScale) * dizzy;
      let starGlow = exp(-max(dS, 0.0) * 24.0) * dizzy * 0.65;
      col = mix(col, vec3f(1.0, 0.96, 0.75), starCore * cover);
      halo = halo + vec3f(1.0, 0.85, 0.28) * (starCore * 1.2 + starGlow);
    }
  }

  let haloBlock = select(0.88, 0.96, isKit);
  let outRgb = (col * cover + halo * (1.0 - cover * haloBlock)) * quadFade;
  return vec4f(outRgb, cover * quadFade) * u.h4.y;
}
`

// ─── 3. 4-Vertex Convex Voronoi Shards (Compute + Render) ──────────────────────
const SHARD_T = /* wgsl */ `
struct Shard {
  pos: vec2f, vel: vec2f,
  ang: f32, spin: f32, life: f32, maxLife: f32,
  v0: vec2f, v1: vec2f,
  v2: vec2f, v3: vec2f,
  base: vec2f, hs: vec2f,
  uvr: vec4f
}
`

const SHARD_SIM = /* wgsl */ `
${COMMON}
${SHARD_T}
@group(0) @binding(1) var<storage, read_write> ss: array<Shard>;
@compute @workgroup_size(64) fn sim(@builtin(global_invocation_id) id: vec3u) {
  let i = id.x;
  if (i >= arrayLength(&ss)) { return; }
  var s = ss[i];
  if (s.life <= 0.0) { return; }
  let dt = u.misc.y;
  let isKeycap = s.hs.y < 0.0;
  let drag = select(u.phys.x, u.phys.x * 0.28, isKeycap);
  let grav = select(u.phys.y, u.phys.y * 1.75, isKeycap);
  s.vel = s.vel * exp(-drag * dt);
  s.vel.y = s.vel.y + grav * dt;
  s.pos = s.pos + s.vel * dt;
  s.ang = s.ang + s.spin * dt;
  s.spin = s.spin * exp(select(-0.95, -0.25, isKeycap) * dt);

  let floorY = select(u.view.y - 14.0, u.view.y + 260.0, isKeycap);
  if (s.pos.y > floorY && s.vel.y > 0.0) {
    s.pos.y = floorY;
    s.vel.y = -s.vel.y * 0.42;
    s.vel.x = s.vel.x * 0.78;
    s.spin = s.spin * 0.65 + s.vel.x * 0.012;
  }

  s.life = s.life - dt;
  ss[i] = s;
}
`

const SHARDS = /* wgsl */ `
${COMMON}
${SHARD_T}
@group(0) @binding(1) var<storage, read> ss: array<Shard>;
@group(0) @binding(2) var tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
struct VO { @builtin(position) pos: vec4f, @location(0) world: vec2f, @location(1) @interpolate(flat) idx: u32 }
@vertex fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
  var o: VO;
  let s = ss[ii];
  if (s.life <= 0.0) { o.pos = vec4f(9.0, 9.0, 9.0, 1.0); return o; }
  let m = rot(s.ang);
  let age = max(0.0, s.maxLife - s.life);
  let tumble = age * (abs(s.spin) * 0.55 + 2.8) + s.maxLife * 5.7;
  let tiltScale = vec2f(0.40 + 0.60 * abs(cos(tumble)), 1.0);
  let a = s.pos + m * (s.v0 * tiltScale);
  let b = s.pos + m * (s.v1 * tiltScale);
  let c = s.pos + m * (s.v2 * tiltScale);
  let d = s.pos + m * (s.v3 * tiltScale);
  let bl = s.vel * u.misc.z * 0.45;
  let lo = min(min(a, b), min(c, d)) - abs(bl) - vec2f(2.0);
  let hi = max(max(a, b), max(c, d)) + abs(bl) + vec2f(2.0);
  let k = CORNERS[vi] * 0.5 + 0.5;
  let w = mix(lo, hi, k);
  o.pos = toClip(w);
  o.world = w;
  o.idx = ii;
  return o;
}
fn cross2(a: vec2f, b: vec2f) -> f32 { return a.x * b.y - a.y * b.x; }
fn edgeDist(a: vec2f, b: vec2f, p: vec2f, sgn: f32) -> f32 {
  let e = b - a;
  let l = length(e);
  if (l < 1e-2) { return 999.0; }
  return sgn * cross2(e, p - a) / l;
}
@fragment fn fs(i: VO) -> @location(0) vec4f {
  let s = ss[i.idx];
  let mi = rot(-s.ang);
  let age = max(0.0, s.maxLife - s.life);
  let tumble = age * (abs(s.spin) * 0.55 + 2.8) + s.maxLife * 5.7;
  let heat = exp(-age / 0.28);
  let tiltScale = vec2f(0.40 + 0.60 * abs(cos(tumble)), 1.0);
  let bl = s.vel * u.misc.z * 0.85;
  let area = cross2(s.v1 - s.v0, s.v2 - s.v0);
  if (abs(area) < 1e-4) { return vec4f(0.0); }
  let sgn = select(-1.0, 1.0, area > 0.0);

  var acc = vec4f(0.0);
  var edgeGlowAcc = 0.0;
  let N = 6;
  let j = ign(i.pos.xy);
  for (var k = 0; k < N; k++) {
    let f = (f32(k) + j) / f32(N) - 0.5;
    let lp = (mi * (i.world - bl * f - s.pos)) / tiltScale;
    let d0 = edgeDist(s.v0, s.v1, lp, sgn);
    let d1 = edgeDist(s.v1, s.v2, lp, sgn);
    let d2 = edgeDist(s.v2, s.v3, lp, sgn);
    let d3 = edgeDist(s.v3, s.v0, lp, sgn);
    let dIn = min(min(d0, d1), min(d2, d3));
    if (dIn >= 0.0) {
      let sp = s.base + lp;
      let uv = mix(s.uvr.xy, s.uvr.zw, sp / (2.0 * abs(s.hs)) + 0.5);
      let sampCol = textureSampleLevel(tex, samp, uv, 0.0);
      acc = acc + sampCol;
      edgeGlowAcc = edgeGlowAcc + (1.0 - smoothstep(0.0, 2.2, dIn)) * sampCol.a;
    }
  }
  acc = acc / f32(N);
  if (acc.a <= 0.002) { return vec4f(0.0); }
  let edgeRim = edgeGlowAcc / f32(N);
  let isKeycap = s.hs.y < 0.0;

  let t = clamp(s.life / s.maxLife, 0.0, 1.0);
  let fade = smoothstep(0.0, select(0.35, 0.12, isKeycap), t);
  let glint = pow(max(0.0, sin(tumble * 2.0 + s.ang * 0.5)), select(10.0, 6.0, isKeycap)) * select(0.42, 0.65, isKeycap);
  let keycapAmber = mix(vec3f(1.0, 0.52, 0.12), vec3f(1.0, 0.88, 0.45), heat);
  let rimCol = select(select(ember(0.45 + 0.5 * heat), saiyanCol(0.55 + 0.45 * heat), u.h6.x > 0.25), keycapAmber, isKeycap);
  var rgb = acc.rgb + vec3f(0.95, 0.96, 1.0) * glint * acc.a;
  rgb = rgb + rimCol * edgeRim * (select(0.35, 0.65, isKeycap) + 1.25 * heat);

  return vec4f(rgb, acc.a) * fade;
}
`

// ─── 4. Flying Words Shader ────────────────────────────────────────────────────
const WORDS = /* wgsl */ `
${COMMON}
struct Word { pos: vec2f, hs: vec2f, ang: f32, alpha: f32, heat: f32, scale: f32, vel: vec2f, acc: vec2f, uvr: vec4f }
@group(0) @binding(1) var<storage, read> ws: array<Word>;
@group(0) @binding(2) var tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
struct VO {
  @builtin(position) pos: vec4f,
  @location(0) world: vec2f,
  @location(1) wpos: vec2f,
  @location(2) hs: vec2f,
  @location(3) vel: vec2f,
  @location(4) acc: vec2f,
  @location(5) uvr: vec4f,
  @location(6) b: vec4f,
}
@vertex fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
  var o: VO;
  let w = ws[ii];
  let c = CORNERS[vi];
  let dt = u.misc.z;
  let reach = length(w.vel) * dt * 0.5 + length(w.acc) * dt * dt * 0.125 + 6.0;
  let world = w.pos + rot(w.ang) * (c * (w.hs + vec2f(reach)));
  o.pos = toClip(world);
  o.world = world;
  o.wpos = w.pos;
  o.hs = w.hs;
  o.vel = w.vel;
  o.acc = w.acc;
  o.uvr = w.uvr;
  o.b = vec4f(w.ang, w.alpha, w.heat, w.scale);
  return o;
}
@fragment fn fs(i: VO) -> @location(0) vec4f {
  let N = 10;
  let j = ign(i.pos.xy);
  let mi = rot(-i.b.x);
  let shutter = u.misc.z * 0.85;
  var ink = 0.0;
  var glow = 0.0;
  for (var k = 0; k < N; k++) {
    let s = ((f32(k) + j) / f32(N) - 0.5) * shutter;
    let center = i.wpos - i.vel * s + 0.5 * i.acc * s * s;
    let sc = max(0.25, 1.0 - 0.65 * s);
    // hs already includes the word's scale (main.js agent fix, 2026-10-20: dividing by b.w again
    // cancelled it, so words drew at full size inside a scaled, clipping quad).
    let u0 = (mi * (i.world - center)) / (i.hs * sc) * 0.5 + 0.5;
    if (u0.x >= 0.0 && u0.x <= 1.0 && u0.y >= 0.0 && u0.y <= 1.0) {
      let uv = mix(i.uvr.xy, i.uvr.zw, u0);
      let s0 = textureSampleLevel(tex, samp, uv, 0.0);
      ink = ink + s0.r;
      glow = glow + s0.g;
    }
  }
  ink = ink / f32(N);
  glow = glow / f32(N);
  let h = i.b.z;
  let saiyan = u.h6.x;
  let inkCol = mix(vec3f(0.95, 0.95, 0.94), select(vec3f(1.0, 0.97, 0.90), vec3f(1.0, 0.98, 0.85), saiyan > 0.25), h);
  let glowPal = select(ember(0.38 + h * 0.45), saiyanCol(0.45 + h * 0.5), saiyan > 0.25);
  let glowCol = glowPal * glow * (0.28 + h * 1.15);
  let rgb = inkCol * ink + glowCol;
  let a = clamp(ink + glow * (0.14 + h * 0.35), 0.0, 1.0);
  return vec4f(rgb, a) * i.b.y;
}
`

// ─── 5. FX Pass: Rings, Flashes, Lightning (4), Emerald Checkmark (5), Tommy Muzzle Cone (6), Tracer Beam (7), Fireball (8), Smoke (9), Scorch (10) ───
const FX = /* wgsl */ `
${COMMON}
struct F { pos: vec2f, r: f32, kind: f32, t: f32, width: f32, k: f32, pad: f32 }
@group(0) @binding(1) var<storage, read> fs0: array<F>;
struct VO {
  @builtin(position) pos: vec4f,
  @location(0) q: vec2f,
  @location(1) @interpolate(flat) kind: i32,
  @location(2) p: vec4f,
  @location(3) @interpolate(flat) seed: f32,
}
@vertex fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
  var o: VO;
  let f = fs0[ii];
  let c = CORNERS[vi];
  let k = i32(f.kind);
  var w = f.pos + c * (f.r + f.width * 2.0 + 6.0);
  if (k == 2) { w = u.view.xy * 0.5 + c * u.view.xy; }
  else if (k == 4 || k == 7) {
    let dir = vec2f(cos(f.width), sin(f.width));
    let nrm = vec2f(-dir.y, dir.x);
    let halfW = select(f.r * 0.28 + 14.0, 16.0, k == 7);
    let local = dir * ((c.x * 0.5 + 0.5) * f.r) + nrm * (c.y * halfW);
    w = f.pos + local;
  } else if (k == 6) {
    let dir = vec2f(cos(f.width), sin(f.width));
    let nrm = vec2f(-dir.y, dir.x);
    let local = dir * ((c.x * 0.5 + 0.35) * f.r * 1.4) + nrm * (c.y * f.r * 0.85);
    w = f.pos + local;
  } else if (k >= 8) {
    w = f.pos + c * (f.r * 1.18 + 6.0);
  }
  o.pos = toClip(w);
  o.q = c;
  o.kind = k;
  o.p = vec4f(f.r, f.t, f.width, f.k);
  o.seed = f.pad;
  return o;
}
fn sdSeg2(p: vec2f, a: vec2f, b: vec2f) -> f32 {
  let pa = p - a; let ba = b - a;
  let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-4), 0.0, 1.0);
  return length(pa - ba * h);
}
@fragment fn fs(i: VO) -> @location(0) vec4f {
  let r = i.p.x;
  let t = i.p.y;
  let w = i.p.z;
  let k = i.p.w;
  let saiyan = u.h6.x;

  if (i.kind == 2) {
    let tint = select(vec3f(1.0, 0.92, 0.80), vec3f(1.0, 0.96, 0.78), saiyan > 0.25);
    return vec4f(tint * k * 0.48, 0.0);
  }
  if (i.kind == 4) {
    // Crackling lightning bolt
    let uCoord = i.q.x * 0.5 + 0.5;
    let vCoord = i.q.y;
    let seed = floor(u.misc.x * 32.0) + r * 0.13;
    let jag1 = (hash21(vec2f(floor(uCoord * 7.0), seed)) - 0.5) * 0.55;
    let jag2 = sin(uCoord * 28.0 + seed * 3.1) * 0.16;
    let envelope = sin(uCoord * 3.14159265);
    let centerV = (jag1 + jag2) * envelope;
    let dist = abs(vCoord - centerV);
    let core = (1.0 - smoothstep(0.0, 0.06, dist)) * envelope;
    let glow = exp(-dist * 9.0) * envelope * 0.68;
    let col = vec3f(0.90, 0.98, 1.0) * core * 1.5 + select(vec3f(1.0, 0.55, 0.2), vec3f(1.0, 0.85, 0.25), saiyan > 0.2) * glow;
    return vec4f(col * k * (1.0 - t), 0.0);
  }
  if (i.kind == 5) {
    // Clean OpenCode Emerald Checkmark Stroke-Draw + Soft Mint Verification Badge!
    let p = i.q;
    let fade = (1.0 - smoothstep(0.65, 1.0, t)) * k;
    let drawProg = clamp(t * 3.2, 0.0, 1.0);
    // Soft mint circle badge + thin ring
    let dCircle = length(p) - 0.62;
    let ring = exp(-pow(dCircle / 0.045, 2.0)) * 0.65;
    let fill = (1.0 - smoothstep(-0.04, 0.04, dCircle)) * 0.18;
    // Checkmark vertices: p0=(-0.26, 0.02) -> p1=(-0.05, 0.22) -> p2=(0.30, -0.20)
    let p0 = vec2f(-0.26, 0.02);
    let p1 = vec2f(-0.05, 0.22);
    let p2 = vec2f(0.30, -0.20);
    let pMid = mix(p0, p1, clamp(drawProg * 2.2, 0.0, 1.0));
    let pEnd = mix(p1, p2, clamp((drawProg - 0.42) * 1.75, 0.0, 1.0));
    var dCheck = sdSeg2(p, p0, pMid);
    if (drawProg > 0.42) {
      dCheck = min(dCheck, sdSeg2(p, p1, pEnd));
    }
    let checkCore = 1.0 - smoothstep(0.055, 0.085, dCheck);
    let checkGlow = exp(-max(dCheck - 0.04, 0.0) * 10.0) * 0.55;
    let mint = vec3f(0.22, 0.98, 0.62);
    let whiteMint = vec3f(0.88, 1.0, 0.94);
    let col = mint * (ring + fill + checkGlow) + whiteMint * checkCore * 1.25;
    return vec4f(col * fade, 0.0);
  }
  if (i.kind == 6) {
    // Directional Tommy-Gun Muzzle Flash Cone
    let uC = i.q.x * 0.5 + 0.5;
    let vC = i.q.y;
    let coneW = 0.12 + uC * 0.65;
    let dCone = abs(vC) / max(coneW, 0.05);
    let axial = exp(-pow((uC - 0.25) / 0.42, 2.0)) * (1.0 - smoothstep(0.75, 1.0, uC));
    let crossSpike = exp(-pow(uC / 0.14, 2.0)) * exp(-abs(vC) * 3.2);
    let core = exp(-dCone * dCone * 3.5) * axial + crossSpike * 0.85;
    let col = mix(vec3f(1.0, 0.62, 0.14), vec3f(1.0, 0.98, 0.90), clamp(core * 0.85, 0.0, 1.0));
    return vec4f(col * core * k * (1.0 - t), 0.0);
  }
  if (i.kind == 7) {
    // High-Velocity Ballistic Tracer Beam
    let uC = i.q.x * 0.5 + 0.5;
    let vC = abs(i.q.y);
    // Travels forward rapidly along the beam as t goes 0 -> 1
    let headPos = clamp(t * 1.45, 0.15, 1.0);
    let trail = smoothstep(max(0.0, headPos - 0.75), headPos, uC) * (1.0 - smoothstep(headPos, headPos + 0.06, uC));
    let beamCore = (1.0 - smoothstep(0.0, 0.14, vC)) * trail;
    let beamGlow = exp(-vC * 5.5) * trail * 0.72;
    let col = vec3f(1.0, 0.98, 0.90) * beamCore * 1.4 + vec3f(1.0, 0.72, 0.18) * beamGlow;
    return vec4f(col * k * (1.0 - t * 0.65), 0.0);
  }
  if (i.kind == 8) {
    // Volumetric noise-driven fireball expanding and cooling into dark billowing smoke
    let p = rot(i.seed * 1.7) * i.q;
    let r0 = length(p);
    if (r0 > 1.08) { return vec4f(0.0); }
    let adv = vec2f(0.0, t * 2.4);
    let n1 = fbm(p * 2.7 - adv + vec2f(i.seed * 3.1, i.seed * 5.3));
    let n2 = fbm(p * 5.8 + vec2f(n1 * 1.35, -t * 3.6) + vec2f(i.seed * 7.9));
    let billow = n1 * 0.60 + n2 * 0.40;
    let lobeDist = r0 + (billow - 0.52) * (0.36 + 0.24 * t);
    let density = clamp((0.80 - lobeDist) / 0.30, 0.0, 1.0) * smoothstep(1.05, 0.75, r0);
    if (density <= 0.002) { return vec4f(0.0); }

    let cool = smoothstep(0.14, 0.86, t);
    let coreHeat = clamp((1.05 - r0 * 0.92) * (0.55 + 0.75 * billow) - cool * 0.82, 0.0, 1.0);
    let firePal = select(ember(0.22 + 0.78 * coreHeat), saiyanCol(0.28 + 0.72 * coreHeat), saiyan > 0.35);
    let whiteHot = vec3f(1.0, 0.98, 0.90) * pow(coreHeat, 3.2) * (1.0 - smoothstep(0.0, 0.52, t));
    let fireEmit = (firePal * pow(coreHeat, 1.25) * 1.65 + whiteHot * 1.4) * density * (1.0 - smoothstep(0.75, 1.0, t)) * k;

    // Dark charcoal smoke shell forming as the outer billows cool
    let smokeForm = smoothstep(0.16, 0.58, t) * (1.0 - smoothstep(0.72, 1.0, t));
    let shade = 0.045 + 0.045 * billow + 0.025 * max(0.0, -i.q.y);
    let underRim = max(0.0, i.q.y * 0.6 + 0.4) * pow(1.0 - coreHeat, 1.5) * (1.0 - t);
    let smokeRgb = vec3f(shade, shade * 0.96, shade * 1.06) + firePal * underRim * 0.22;
    let smokeAlpha = clamp(density * smokeForm * (1.0 - coreHeat * 0.65) * 0.78 * min(1.0, k), 0.0, 0.82);

    return vec4f(smokeRgb * smokeAlpha + fireEmit, smokeAlpha);
  }
  if (i.kind == 9) {
    // Rising smoke plume that lingers, drifts, and catches warm under-light
    let p = rot(i.seed * 0.9) * i.q;
    let r0 = length(p);
    if (r0 > 1.05) { return vec4f(0.0); }
    let n1 = fbm(p * 2.4 - vec2f(0.0, t * 1.6) + vec2f(i.seed * 4.3, i.seed * 2.1));
    let n2 = fbm(p * 4.8 + vec2f(n1 * 1.1, -t * 2.4) + vec2f(i.seed * 6.7));
    let cloud = n1 * 0.65 + n2 * 0.35;
    let edge = r0 + (cloud - 0.5) * 0.42;
    let density = clamp((0.76 - edge) / 0.34, 0.0, 1.0) * smoothstep(1.02, 0.68, r0);
    if (density <= 0.002) { return vec4f(0.0); }

    let env = smoothstep(0.0, 0.14, t) * pow(1.0 - t, 1.45);
    let shade = 0.065 + 0.055 * cloud + 0.025 * (0.5 - i.q.y * 0.5);
    let warmBounce = max(0.0, i.q.y * 0.7 + 0.35) * exp(-t * 2.6) * (0.35 + 0.65 * cloud);
    let warmCol = select(vec3f(1.0, 0.48, 0.16), vec3f(1.0, 0.78, 0.24), saiyan > 0.3);
    let smokeCol = vec3f(shade, shade * 0.98, shade * 1.05);
    let alpha = clamp(density * env * 0.62 * k, 0.0, 0.72);
    let rimEmit = warmCol * warmBounce * density * env * 0.32 * k;
    return vec4f(smokeCol * alpha + rimEmit, alpha);
  }
  if (i.kind == 10) {
    // Scorch heat glow & double shockwave pressure distortion ring
    let r0 = length(i.q);
    if (r0 > 1.05) { return vec4f(0.0); }
    let n = fbm(i.q * 4.2 + vec2f(i.seed * 2.7, -t * 1.8));
    let waveR = 0.25 + 0.70 * (1.0 - pow(1.0 - t, 2.4));
    let ring1 = exp(-pow((r0 - waveR) / 0.055, 2.0)) * (1.0 - t);
    let ring2 = exp(-pow((r0 - waveR * 0.76) / 0.08, 2.0)) * (1.0 - t) * 0.55;
    let scorchCore = pow(max(0.0, 1.0 - r0 * 1.25), 2.0) * (0.5 + 0.8 * n) * pow(1.0 - t, 1.8);
    let pal = select(ember(0.35 + 0.55 * (ring1 + scorchCore)), saiyanCol(0.45 + 0.5 * (ring1 + scorchCore)), saiyan > 0.3);
    let col = pal * (scorchCore * 0.75 + ring2 * 0.85) + vec3f(1.0, 0.94, 0.82) * ring1 * 1.15;
    return vec4f(col * k, 0.0);
  }

  let span = r + w * 2.0 + 6.0;
  let d = length(i.q) * span;
  if (i.kind == 0) {
    // Crisp shockwave ring (no blurry chromatic mess over card text!)
    let s = max(w, 1.2);
    let a = exp(-pow((d - r) / s, 2.0)) * k;
    let baseCol = select(mix(vec3f(1.0, 0.96, 0.90), vec3f(1.0, 0.48, 0.22), t), saiyanCol(1.0 - t * 0.55), saiyan > 0.25);
    return vec4f(baseCol * a, 0.0);
  }
  let u0 = d / max(r, 1.0);
  if (u0 > 1.0) { return vec4f(0.0); }
  let fall = pow(1.0 - u0, 2.2) * k;
  let c = select(select(ember(0.55), saiyanCol(0.65), saiyan > 0.25), vec3f(1.0, 0.97, 0.92), i.kind == 1);
  return vec4f(c * fall, 0.0);
}
`

// ─── 6. Compute Sparks & Tumbling Brass Shell Casings ──────────────────────────
const SPARK_T = /* wgsl */ `
struct P { pos: vec2f, vel: vec2f, life: f32, maxLife: f32, heat: f32, size: f32 }
`

const SPARK_SIM = /* wgsl */ `
${COMMON}
${SPARK_T}
@group(0) @binding(1) var<storage, read_write> ps: array<P>;
@compute @workgroup_size(64) fn sim(@builtin(global_invocation_id) id: vec3u) {
  let i = id.x;
  if (i >= arrayLength(&ps)) { return; }
  var p = ps[i];
  if (p.life <= 0.0) { return; }
  let dt = u.misc.y;
  p.vel = p.vel * exp(-u.phys.z * dt);
  if (p.heat < -0.5 && p.heat > -1.4) {
    // Rising Super Saiyan golden plasma sparks: upward buoyancy + spiral turbulence
    p.vel.y = p.vel.y - u.phys.w * 0.65 * dt;
    p.vel.x = p.vel.x + sin(u.misc.x * 14.0 + p.pos.y * 0.04 + f32(i) * 0.7) * 240.0 * dt;
  } else {
    // Normal sparks & brass shell casings (p.heat <= -1.5) fall with gravity
    p.vel.y = p.vel.y + u.phys.w * select(1.0, 1.65, p.heat <= -1.5) * dt;
  }
  p.pos = p.pos + p.vel * dt;

  let floorY = u.view.y - 10.0;
  if (p.pos.y > floorY && p.vel.y > 0.0) {
    p.pos.y = floorY;
    p.vel.y = -p.vel.y * 0.48;
    p.vel.x = p.vel.x * 0.80;
  }

  p.life = p.life - dt;
  ps[i] = p;
}
`

const SPARKS = /* wgsl */ `
${COMMON}
${SPARK_T}
@group(0) @binding(1) var<storage, read> ps: array<P>;
struct VO {
  @builtin(position) pos: vec4f,
  @location(0) col: vec3f,
  @location(1) c: vec2f,
  @location(2) @interpolate(flat) kind: f32, // 0=spark, 1=glassChip, 2=brassCasing
}
@vertex fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
  var o: VO;
  let p = ps[ii];
  if (p.life <= 0.0) { o.pos = vec4f(9.0, 9.0, 9.0, 1.0); return o; }
  let c = CORNERS[vi];
  let sp = length(p.vel);
  var d = vec2f(1.0, 0.0);
  if (sp > 0.001) { d = p.vel / sp; }

  let isCasing = step(p.heat, -1.5);
  let isChip = step(3.0, p.size) * step(0.0, p.heat) * (1.0 - isCasing);
  let effShutter = max(u.misc.z, 0.024 * u.h6.x);

  var len = max(p.size * 2.4, sp * effShutter * u.phys2.x);
  var w = p.size * 0.5 + 0.6;
  let spinRot = u.misc.x * 22.0 + f32(ii) * 1.9;
  var dir = d;

  if (isCasing > 0.5) {
    len = p.size * 2.6;
    w = p.size * 1.05;
    dir = vec2f(cos(spinRot), sin(spinRot));
  } else if (isChip > 0.5) {
    len = p.size * 2.1;
    w = p.size * 1.15;
    dir = vec2f(cos(spinRot), sin(spinRot));
  }

  o.pos = toClip(p.pos + dir * c.x * len * 0.5 + vec2f(-dir.y, dir.x) * c.y * w);

  let t = clamp(p.life / p.maxLife, 0.0, 1.0);
  var col = vec3f(0.58, 0.60, 0.66) * 0.72;
  if (isCasing > 0.5) {
    // Tumbling brass Tommy-gun shell casing with specular glint!
    let glint = pow(max(0.0, sin(spinRot * 2.0)), 4.0);
    col = mix(vec3f(0.85, 0.58, 0.16), vec3f(1.0, 0.96, 0.80), glint) * 1.35;
  } else if (p.heat < -0.5) {
    col = saiyanCol(abs(p.heat) * (0.45 + 0.55 * t)) * 1.25;
  } else if (p.heat < -0.1) {
    col = vec3f(0.35, 0.96, 0.65);
  } else if (isChip > 0.5) {
    let glint = pow(max(0.0, sin(spinRot * 2.0)), 6.0);
    col = mix(vec3f(0.28, 0.30, 0.36), vec3f(1.0, 0.95, 0.88), glint);
  } else if (p.heat > 0.001) {
    col = select(ember(p.heat * t * 1.1), saiyanCol(p.heat * t * 1.1), u.h6.x > 0.3);
  }
  o.col = col * t * (p.size / (p.size + 0.55));
  o.c = c;
  o.kind = select(select(0.0, 1.0, isChip > 0.5), 2.0, isCasing > 0.5);
  return o;
}
@fragment fn fs(i: VO) -> @location(0) vec4f {
  if (i.kind > 1.5) {
    // Cylindrical brass shell casing silhouette with rim highlight
    let d = max(abs(i.c.x) - 0.78, abs(i.c.y) - 0.62);
    let m = 1.0 - smoothstep(-0.08, 0.12, d);
    let cyl = 1.0 - 0.35 * i.c.y * i.c.y;
    return vec4f(i.col * m * cyl, 0.0);
  }
  if (i.kind > 0.5) {
    let d = abs(i.c.x) * 0.85 + abs(i.c.y);
    let m = 1.0 - smoothstep(0.75, 1.0, d);
    return vec4f(i.col * m, 0.0);
  }
  let r2 = dot(i.c, i.c);
  let a = max(0.0, 1.0 - r2) * (1.0 - i.c.y * i.c.y);
  return vec4f(i.col * a, 0.0);
}
`

const MAX_PROBLEMS = 96
const MAX_WORDS = 256
const MAX_SHARDS = 4096
const MAX_SPARKS = 16384
const MAX_FX = 256
const SHARD_FLOATS = 24
const PROB_FLOATS = 20

export class GpuRenderer {
  static async create(canvas, sprites) {
    try {
      if (!navigator.gpu) return null
      const adapter = await navigator.gpu.requestAdapter()
      if (!adapter) return null
      const device = await adapter.requestDevice()
      const ctx = canvas.getContext("webgpu")
      if (!ctx) return null
      const format = navigator.gpu.getPreferredCanvasFormat()
      const renderer = new GpuRenderer(canvas, ctx, device, format, sprites)
      return renderer
    } catch (e) {
      return null
    }
  }

  constructor(canvas, ctx, device, format, sprites) {
    this.kind = "webgpu"
    this.canvas = canvas
    this.ctx = ctx
    this.device = device
    this.format = format
    this.sprites = sprites
    this.fx = shoutFx
    this.fx.onSpawnShards = (list) => this.rawSpawnShards(list)
    this.fx.onSpawnSparks = (list) => this.spawnSparks(list)
    this.ctx.configure({ device, format, alphaMode: "premultiplied" })

    const U = GPUBufferUsage
    this.uni = device.createBuffer({ size: 288, usage: U.UNIFORM | U.COPY_DST })
    this.probBuf = device.createBuffer({ size: MAX_PROBLEMS * PROB_FLOATS * 4, usage: U.STORAGE | U.COPY_DST })
    this.wordBuf = device.createBuffer({ size: MAX_WORDS * 64, usage: U.STORAGE | U.COPY_DST })
    this.shards = device.createBuffer({ size: MAX_SHARDS * SHARD_FLOATS * 4, usage: U.STORAGE | U.COPY_DST })
    this.sparks = device.createBuffer({ size: MAX_SPARKS * 32, usage: U.STORAGE | U.COPY_DST })
    this.fxBuf = device.createBuffer({ size: MAX_FX * 32, usage: U.STORAGE | U.COPY_DST })

    const T = GPUTextureUsage
    const cw = sprites.cards.canvas?.width || 4096
    const ww = sprites.words.canvas?.width || 4096
    this.probTex = device.createTexture({ size: [cw, cw], format: "rgba8unorm", usage: T.TEXTURE_BINDING | T.COPY_DST | T.RENDER_ATTACHMENT })
    this.wordTex = device.createTexture({ size: [ww, ww], format: "rgba8unorm", usage: T.TEXTURE_BINDING | T.COPY_DST | T.RENDER_ATTACHMENT })
    this.samp = device.createSampler({ magFilter: "linear", minFilter: "linear" })


    const pre = { color: { srcFactor: "one", dstFactor: "one-minus-src-alpha" }, alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha" } }
    const add = { color: { srcFactor: "one", dstFactor: "one" }, alpha: { srcFactor: "zero", dstFactor: "one" } }
    const pipe = (code, blend) => {
      const m = device.createShaderModule({ code })
      return device.createRenderPipeline({
        layout: "auto",
        vertex: { module: m, entryPoint: "vs" },
        fragment: { module: m, entryPoint: "fs", targets: [{ format, blend }] },
        primitive: { topology: "triangle-list" },
      })
    }
    const cpipe = (code) => device.createComputePipeline({ layout: "auto", compute: { module: device.createShaderModule({ code }), entryPoint: "sim" } })

    this.probPipe = pipe(PROBLEMS, pre)
    this.headPipe = pipe(HEAD, pre)
    this.shardPipe = pipe(SHARDS, pre)
    this.wordPipe = pipe(WORDS, pre)
    this.fxPipe = pipe(FX, pre)
    this.sparkPipe = pipe(SPARKS, add)
    this.shardSim = cpipe(SHARD_SIM)
    this.sparkSim = cpipe(SPARK_SIM)

    const bg = (p, entries) =>
      device.createBindGroup({
        layout: p.getBindGroupLayout(0),
        entries: [{ binding: 0, resource: { buffer: this.uni } }, ...entries.map((r, i) => ({ binding: i + 1, resource: r }))],
      })
    const B = (b) => ({ buffer: b })
    this.probBind = bg(this.probPipe, [B(this.probBuf), this.probTex.createView(), this.samp])
    this.headBind = bg(this.headPipe, [])
    this.shardBind = bg(this.shardPipe, [B(this.shards), this.probTex.createView(), this.samp])
    this.wordBind = bg(this.wordPipe, [B(this.wordBuf), this.wordTex.createView(), this.samp])
    this.fxBind = bg(this.fxPipe, [B(this.fxBuf)])
    this.sparkBind = bg(this.sparkPipe, [B(this.sparks)])
    this.shardSimBind = bg(this.shardSim, [B(this.shards)])
    this.sparkSimBind = bg(this.sparkSim, [B(this.sparks)])

    this.u = new Float32Array(72)
    this.probData = new Float32Array(MAX_PROBLEMS * PROB_FLOATS)
    this.wordData = new Float32Array(MAX_WORDS * 16)
    this.fxData = new Float32Array(MAX_FX * 8)
    this.shardRing = 0
    this.sparkRing = 0
    this.pendingShards = []
    this.pendingSparks = []
  }

  setSaiyan(level) {
    this.fx.setSaiyan(level)
  }
  saiyanBurst() {
    this.fx.saiyanBurst()
  }
  resolveCard(card) {
    return this.fx.resolveCard(card)
  }
  ocIntro(card) {
    this.fx.ocIntro(card)
  }
  ocSubmit(card) {
    this.fx.ocSubmit(card)
  }
  setFinale(state) {
    this.fx.setFinale(state)
  }
  setSpinFinale(state) {
    this.fx.setFinale(state)
  }
  finaleShot(shot) {
    this.fx.finaleShot(shot)
  }
  fireTommyShot(shot) {
    this.fx.finaleShot(shot)
  }
  hitCard(card, impulse, point) {
    this.fx.hitCard(card, impulse, point)
  }

  rawSpawnShards(list) {
    for (const s of list) this.pendingShards.push(s)
  }

  spawnShards(list) {
    const voronoi = this.fx.fractureVoronoi(list)
    this.rawSpawnShards(voronoi)
  }

  spawnSparks(list) {
    for (const s of list) this.pendingSparks.push(s)
  }

  resize(w, h, dpr) {
    const cw = Math.max(1, Math.round(w * dpr))
    const ch = Math.max(1, Math.round(h * dpr))
    if (this.canvas.width !== cw) this.canvas.width = cw
    if (this.canvas.height !== ch) this.canvas.height = ch
  }

  uploadAtlases() {
    const q = this.device.queue
    const up = (src, tex, premultipliedAlpha) => {
      for (const r of src.dirty) {
        q.copyExternalImageToTexture({ source: src.canvas, origin: { x: r.x, y: r.y } }, { texture: tex, origin: { x: r.x, y: r.y }, premultipliedAlpha }, [r.w, r.h])
      }
      src.dirty.length = 0
    }
    up(this.sprites.cards, this.probTex, true)
    up(this.sprites.words, this.wordTex, false)
  }

  flushRing(list, buffer, max, floats, cursorKey, pack) {
    if (!list.length) return
    const n = Math.min(list.length, max)
    let start = this[cursorKey]
    let run = []
    const flush = () => {
      if (!run.length) return
      const buf = new Float32Array(run.length * floats)
      run.forEach((s, i) => buf.set(pack(s), i * floats))
      this.device.queue.writeBuffer(buffer, start * floats * 4, buf)
      run = []
    }
    for (let i = 0; i < n; i++) {
      run.push(list[i])
      this[cursorKey]++
      if (this[cursorKey] >= max) {
        flush()
        this[cursorKey] = 0
        start = 0
      }
    }
    flush()
    list.length = 0
  }

  draw(scene) {
    this.fx.step(scene)
    const { w, h, head, problems, words, fx, look } = scene
    const u = this.u
    u.fill(0)
    const mouth = head._fxMouth ?? head.mouth
    const mouthW = head._fxMouthW ?? head.mouthW
    const gazeX = head._fxGazeX ?? head.gazeX
    const gazeY = head._fxGazeY ?? head.gazeY
    const variantNum = 0
    const finSpin = this.fx.finale?.spin ?? 0
    const finAng = this.fx.finale?.angle ?? 0
    const finPow = this.fx.finale?.power ?? 0

    const l0 = scene.lights?.[0]
    const l1 = scene.lights?.[1]
    u.set([w, h, scene.shakeX, scene.shakeY, scene.time, scene.dt, look.shutter, scene.roll], 0)
    u.set([head.x, head.y, head.R, head.tilt, head.sx, head.sy, head.anger, mouth], 8)
    u.set([gazeX, gazeY, head.blink, head.happy, head.faceX, head.faceY, head.browUp, mouthW], 16)
    u.set([look.halo, head.visible, head.flicker, look.rim, look.shardDrag, look.shardGravity, look.sparkDrag, look.sparkGravity], 24)
    u.set([look.sparkStretch, this.fx.aberration, this.fx.heatHaze, this.fx.saiyanBurstAge, head.lid, head.squint, head.asym, head.vein], 32)
    u.set([this.fx.saiyan, head._fxPupil ?? 1, head._fxMouthRound ?? 0, head._fxJawSkew ?? 0, head._fxTongue ?? 0, head.mouthOpen ?? 0, head.speaking ? 1 : 0, variantNum], 40)
    u.set([head._fxGooglyLX ?? 0, head._fxGooglyLY ?? 0, head._fxGooglyRX ?? 0, head._fxGooglyRY ?? 0, finSpin, finAng, head._fxDizzy ?? 0, head._fxHairX ?? 0], 48)
    u.set([head._fxHairY ?? 0, this.fx.intensity ?? 0, finPow, 0], 56)
    u.set([head._fxVisemeFrom ?? 0, head._fxVisemeTo ?? 0, head._fxVisemeMix ?? 1, head._fxSaiyanFrame ?? 13], 60)
    u.set([l0?.x ?? 0, l0?.y ?? 0, l0?.r ?? 0, l0?.k ?? 0, l1?.x ?? 0, l1?.y ?? 0, l1?.r ?? 0, l1?.k ?? 0], 64)

    const q = this.device.queue
    q.writeBuffer(this.uni, 0, u)
    this.uploadAtlases()

    this.flushRing(this.pendingShards, this.shards, MAX_SHARDS, SHARD_FLOATS, "shardRing", (s) => {
      const v3 = s.v3 ?? s.v2
      return [
        s.x, s.y, s.vx, s.vy, s.ang, s.spin, s.life, s.life,
        s.v0[0], s.v0[1], s.v1[0], s.v1[1], s.v2[0], s.v2[1], v3[0], v3[1],
        s.base[0], s.base[1], s.half[0], s.half[1], ...s.uv,
      ]
    })
    this.flushRing(this.pendingSparks, this.sparks, MAX_SPARKS, 8, "sparkRing", (s) => [s.x, s.y, s.vx, s.vy, s.life, s.life, s.heat ?? 0, s.size ?? 1.6])

    const np = Math.min(problems.length, MAX_PROBLEMS)
    const pd = this.probData
    for (let i = 0; i < np; i++) {
      const p = problems[i]
      const sc = p._fxScale ?? p.scale
      pd.set(
        [
          p._fxX ?? p.x,
          p._fxY ?? p.y,
          (p.sw / 2) * sc,
          (p.sh / 2) * sc,
          p._fxRot ?? p.rot,
          p.alpha,
          p._fxFlash ?? p.flash,
          p._fxDamage ?? 0,
          p.vx ?? 0,
          p.vy ?? 0,
          p._fxTiltX ?? 0,
          p._fxTiltY ?? 0,
          ...p.uv,
          p._fxImpX ?? 0,
          p._fxImpY ?? 0.2,
          p._fxRipple ?? 1,
          p._fxSeed ?? 0.5,
        ],
        i * PROB_FLOATS,
      )
    }
    if (np) q.writeBuffer(this.probBuf, 0, pd, 0, np * PROB_FLOATS)

    const nw = Math.min(words.length, MAX_WORDS)
    const wd = this.wordData
    for (let i = 0; i < nw; i++) {
      const o = words[i]
      const e = o.entry
      wd.set([o.x, o.y, (e.w / 2) * o.scale * o.k, (e.h / 2) * o.scale * o.k, o.angle, o.alpha, o.heat, o.scale, o.vx, o.vy, o.ax, o.ay, ...e.uv], i * 16)
    }
    if (nw) q.writeBuffer(this.wordBuf, 0, wd, 0, nw * 16)

    const nf = Math.min(fx.length, MAX_FX)
    const fd = this.fxData
    for (let i = 0; i < nf; i++) {
      const f = fx[i]
      fd.set([f.x, f.y, f.r, f.kind, f.t, f.width ?? 0, f.k, f.seed ?? 0], i * 8)
    }
    if (nf) q.writeBuffer(this.fxBuf, 0, fd, 0, nf * 8)

    const enc = this.device.createCommandEncoder()
    const cp = enc.beginComputePass()
    cp.setPipeline(this.shardSim)
    cp.setBindGroup(0, this.shardSimBind)
    cp.dispatchWorkgroups(MAX_SHARDS / 64)
    cp.setPipeline(this.sparkSim)
    cp.setBindGroup(0, this.sparkSimBind)
    cp.dispatchWorkgroups(MAX_SPARKS / 64)
    cp.end()

    const rp = enc.beginRenderPass({
      colorAttachments: [
        {
          view: this.ctx.getCurrentTexture().createView(),
          loadOp: "clear",
          storeOp: "store",
          clearValue: { r: 7 / 255, g: 7 / 255, b: 7 / 255, a: 1 },
        },
      ],
    })
    const pass = (pipe, bind, count) => {
      if (!count) return
      rp.setPipeline(pipe)
      rp.setBindGroup(0, bind)
      rp.draw(6, count)
    }
    pass(this.probPipe, this.probBind, np)
    pass(this.headPipe, this.headBind, head.visible > 0 ? 1 : 0)
    pass(this.shardPipe, this.shardBind, MAX_SHARDS)
    pass(this.wordPipe, this.wordBind, nw)
    pass(this.fxPipe, this.fxBind, nf)
    pass(this.sparkPipe, this.sparkBind, MAX_SPARKS)
    rp.end()
    q.submit([enc.finish()])
  }

  done() {
    return this.device.queue.onSubmittedWorkDone()
  }
}
export default GpuRenderer
