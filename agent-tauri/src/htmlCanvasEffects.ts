import { reportTrayEffectDiagnostic } from './api/commands';

export type HtmlCanvasTrayEffect =
  | 'fold'
  | 'glass'
  | 'swirl'
  | 'suction'
  | 'page-curl'
  | 'curtain'
  | 'shards'
  | 'book-return'
  | 'book-return-v2';

type HtmlCanvasTexElementImage2D = {
  // Current HTML-in-Canvas syntax (Chromium M145+).
  (target: number, internalFormat: number, source: Element): void;
  // Legacy developer-trial syntax, kept as a fallback for older WebView2 runtimes.
  (
    target: number,
    level: number,
    internalFormat: number,
    format: number,
    type: number,
    source: Element,
  ): void;
};

type HtmlCanvasWebGlContext = WebGL2RenderingContext & {
  texElementImage2D?: HtmlCanvasTexElementImage2D;
};

// RGBA8 is the required sized internal format for the current API. WebGL1 does
// not expose gl.RGBA8 as a property, but Chromium's HTML-in-Canvas method accepts
// the GLenum value directly.
const GL_RGBA8 = 0x8058;

type EffectSpec = {
  mode: number;
  duration: number;
  grid: [number, number];
};

const SNAPSHOT_TIMEOUT_MS = 220;
const REPORTED_DIAGNOSTICS = new Set<string>();
const EFFECTS: Record<HtmlCanvasTrayEffect, EffectSpec> = {
  fold: { mode: 0, duration: 620, grid: [24, 24] },
  glass: { mode: 1, duration: 760, grid: [24, 24] },
  swirl: { mode: 2, duration: 850, grid: [28, 28] },
  suction: { mode: 3, duration: 780, grid: [28, 28] },
  'page-curl': { mode: 4, duration: 830, grid: [30, 24] },
  curtain: { mode: 5, duration: 790, grid: [28, 28] },
  shards: { mode: 6, duration: 800, grid: [6, 4] },
  'book-return': { mode: 7, duration: 1040, grid: [32, 26] },
  'book-return-v2': { mode: 8, duration: 900, grid: [32, 26] },
};

const VERTEX_SHADER = `#version 300 es
precision highp float;
in vec2 a_position;
in vec2 a_uv;
in vec2 a_cell;
uniform float u_progress;
uniform float u_mode;
out vec2 v_uv;
out float v_shade;

const float PI = 3.141592653589793;

float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

vec2 rotate2d(vec2 value, float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return mat2(c, -s, s, c) * value;
}

void main() {
  float t = smoothstep(0.0, 1.0, u_progress);
  vec2 p = a_position;
  vec2 anchor = vec2(1.0, -1.0);
  v_shade = 0.0;
  float vertexDepth = 0.0;

  if (u_mode < 0.5) {
    // Cloth fold: loose edges ripple while the whole sheet gathers to the tray corner.
    float shrink = 1.0 - 0.93 * pow(t, 1.28);
    p = anchor + (p - anchor) * shrink;
    float wave = sin(PI * t);
    float looseX = 1.0 - a_uv.x;
    float looseY = 1.0 - a_uv.y;
    p.x += sin((a_uv.y * 3.2 + t * 1.45) * PI) * 0.078 * wave * looseX;
    p.y += cos((a_uv.x * 2.45 - t * 0.85) * PI) * 0.052 * wave * looseY;
    p.x += sin((a_uv.x + a_uv.y) * 2.0 * PI) * 0.035 * wave * looseX;
  } else if (u_mode < 1.5) {
    // Glass wave: alternating horizontal bands refract before collapsing.
    float band = floor(a_uv.y * 8.0);
    float direction = mod(band, 2.0) < 1.0 ? 1.0 : -1.0;
    float wave = sin(PI * t);
    p.x += direction * wave * (0.034 + a_uv.y * 0.022);
    p.y += sin(a_uv.x * 10.0 + band * 1.7 + t * 4.0) * 0.018 * wave;
    float shrink = 1.0 - 0.90 * pow(t, 1.42);
    p = anchor + (p - anchor) * shrink;
  } else if (u_mode < 2.5) {
    // Swirl: rotate progressively harder near the destination while shrinking inward.
    vec2 d = p - anchor;
    float radius = min(1.0, length(d) / 2.7);
    float angle = (1.05 + (1.0 - radius) * 2.4) * PI * pow(t, 1.22);
    d = rotate2d(d, angle);
    float shrink = 1.0 - 0.965 * pow(t, 1.22);
    p = anchor + d * shrink;
    p += vec2(sin(a_uv.y * 6.0 + t * 9.0), cos(a_uv.x * 5.0 + t * 8.0)) * 0.018 * sin(PI * t);
  } else if (u_mode < 3.5) {
    // Liquid suction: points closer to the tray move first and stretch the remaining surface.
    float distanceDelay = (1.0 - a_uv.x) * 0.24 + a_uv.y * 0.18;
    float localT = smoothstep(0.0, 1.0, clamp(t * 1.28 - distanceDelay, 0.0, 1.0));
    vec2 d = p - anchor;
    float stretch = 1.0 - 0.975 * pow(localT, 1.18);
    p = anchor + d * stretch;
    float neck = sin(PI * localT) * (1.0 - localT);
    p.x += sin(a_uv.y * PI * 2.0) * 0.055 * neck * (1.0 - a_uv.x);
    p.y += cos(a_uv.x * PI * 2.0) * 0.032 * neck * a_uv.y;
  } else if (u_mode < 4.5) {
    // Page curl: the right edge rolls first, then the folded sheet is pulled into the tray.
    float front = 1.12 - t * 1.42;
    float curl = smoothstep(front - 0.24, front + 0.05, a_uv.x);
    float theta = curl * PI * 1.18;
    p.x -= curl * (0.22 + 0.34 * t);
    p.y += sin(theta) * 0.12 * (0.35 + 0.65 * (1.0 - a_uv.y));
    p.x += (1.0 - cos(theta)) * 0.085;
    v_shade = curl * sin(theta) * 0.58;
    float gather = smoothstep(0.58, 1.0, t);
    p = mix(p, anchor + (p - anchor) * 0.055, gather);
  } else if (u_mode < 5.5) {
    // Curtain gather: vertical pleats form as the sheet is pulled toward the bottom-right.
    float gather = pow(t, 1.18);
    float pleat = sin(a_uv.x * 12.0 * PI) * 0.052 * sin(PI * t);
    vec2 d = p - anchor;
    d.x *= 1.0 - 0.955 * gather;
    d.y *= 1.0 - 0.90 * pow(t, 1.34);
    p = anchor + d;
    p.x += pleat * (1.0 - gather * 0.45);
    p.y += abs(pleat) * 0.24 * sin(PI * t);
    v_shade = pleat * 3.2;
  } else if (u_mode < 6.5) {
    // GPU shards: each independent quad gets its own rotation/scatter, no DOM cloning.
    vec2 center = vec2(a_cell.x * 2.0 - 1.0, 1.0 - a_cell.y * 2.0);
    vec2 local = p - center;
    float seed = hash21(a_cell * 37.0 + vec2(0.17, 0.61));
    float seed2 = hash21(a_cell * 71.0 + vec2(0.83, 0.29));
    float breakT = smoothstep(0.04, 0.48, t);
    float gatherT = smoothstep(0.30, 1.0, t);
    float angle = (seed - 0.5) * 2.4 * breakT;
    local = rotate2d(local, angle) * (1.0 - 0.16 * breakT);
    vec2 scatter = vec2(seed - 0.5, seed2 - 0.5) * 0.34 * sin(PI * gatherT);
    vec2 movedCenter = mix(center + scatter, anchor, pow(gatherT, 1.26));
    p = movedCenter + local * (1.0 - 0.91 * pow(gatherT, 1.12));
    v_shade = (seed - 0.5) * 0.22 * sin(PI * t);
  } else {
    // Yummi book return: the current UI becomes a page, folds into Yuumi's
    // recognizable book cover, then rides a curved path back toward the tray corner.
    float wake = smoothstep(0.00, 0.18, t);
    float turn = smoothstep(0.12, 0.56, t);
    float closeBook = smoothstep(0.46, 0.72, t);
    float returnHome = smoothstep(0.80, 1.00, t);

    // Phase 1: a soft central spine appears and the outer page edges breathe.
    float spineDistance = abs(a_uv.x - 0.5);
    float spineBend = exp(-spineDistance * 11.0) * 0.055 * sin(PI * wake) * (1.0 - closeBook);
    p.y += spineBend * (0.25 + 0.75 * sin(a_uv.y * PI));
    p.x += sin(a_uv.y * PI * 1.6) * 0.018 * wake * (1.0 - closeBook);

    if (u_mode < 7.5) {
      // Current version: both page blocks close continuously around the spine.
      float pageSide = a_uv.x < 0.5 ? -1.0 : 1.0;
      float pageDistance = abs(a_uv.x - 0.5) * 2.0;
      float pageClose = turn * (1.0 - closeBook * 0.18);
      float inward = pow(pageDistance, 1.18) * 0.24 * pageClose;
      p.x -= pageSide * inward;
      float pageArch = sin(pageDistance * PI) * sin(PI * turn);
      p.y += pageArch * 0.048 * (0.35 + 0.65 * (1.0 - a_uv.y));
      v_shade += pageSide * pageArch * 0.075;
    } else {
      // V2: rotate the complete right page around a shared center hinge. A
      // perspective scale and moving face shade make the page read as one
      // continuous 3D sheet instead of independent jagged mesh strips.
      float rightPage = step(0.5, a_uv.x);
      float pageX = max(a_position.x, 0.0);
      float baseX = p.x - a_position.x;
      // Rotate toward the viewer first (negative Z) before the page settles on
      // the left. The previous positive angle made the page recede, which read
      // as opening in the opposite direction.
      float hingeAngle = -turn * PI * mix(0.92, 1.0, closeBook);
      float pageDepth = pageX * sin(hingeAngle) * rightPage;
      float perspective = 1.0 / (1.0 + pageDepth * 0.34);
      p.x = mix(p.x, baseX + pageX * cos(hingeAngle), rightPage);
      p.x *= perspective;
      p.y *= perspective;
      p.y += pageDepth * (0.5 - a_uv.y) * 0.085;
      float faceShade = abs(sin(hingeAngle)) * (0.10 + pageX * 0.13);
      v_shade -= rightPage * faceShade;
      vertexDepth = pageDepth * 0.42 - rightPage * 0.001;

      // Once the page has crossed the spine, resolve both halves into one
      // complete cover plane. Leaving the rotated half fully overlapped only
      // exposed half of the cover texture at the end of the turn.
      float coverSettle = smoothstep(0.56, 0.74, t);
      p = mix(p, a_position, coverSettle);
      vertexDepth = mix(vertexDepth, -0.001, coverSettle);
    }

    // Phase 3: settle into a compact front-cover shape instead of collapsing to a thin strip.
    // This leaves enough visible area for the burgundy cover, gold trim and blue gem.
    vec2 bookCenter = vec2(0.02, -0.02);
    vec2 d = p - bookCenter;
    float side = a_uv.x < 0.5 ? -1.0 : 1.0;
    float coverDepth = sin(PI * clamp(closeBook, 0.0, 1.0));
    d.x *= 1.0 - 0.40 * closeBook;
    d.y *= 1.0 - 0.28 * closeBook;
    d.x += side * 0.020 * coverDepth;
    d.y += (0.5 - a_uv.y) * 0.026 * coverDepth;
    p = bookCenter + d;

    // Mild perspective gives the closed form the chunky, angled feel of Yuumi's book.
    p.x += (0.5 - a_uv.y) * 0.075 * closeBook;
    p.y += (a_uv.x - 0.5) * 0.035 * closeBook;
    p = bookCenter + rotate2d(p - bookCenter, -0.095 * closeBook);
    v_shade += (0.5 - spineDistance) * 0.12 * closeBook;

    if (u_mode > 7.5) {
      // V2 is one extruded book object: front cover, back cover, spine and
      // three page-block faces. The live UI remains the front/page surface
      // during the fold and settles onto the solid front cover without a swap.
      float fall = smoothstep(0.74, 0.98, t);
      float gravity = fall * fall;
      float impact = smoothstep(0.90, 0.985, t);
      float rebound = sin(PI * impact) * 0.085;
      float impactCompression = sin(PI * impact);
      float vanish = smoothstep(0.90, 1.0, t);
      float flightScale = mix(1.0, 0.52, smoothstep(0.0, 0.38, fall));
      float dropScale = flightScale * (1.0 - 0.68 * pow(vanish, 1.30));
      vec2 dropOffset = vec2(
        0.28 * fall + 0.48 * fall * fall * fall,
        -0.74 * gravity + rebound
      );

      // Keep the original page fold as the front surface until the cover is
      // nearly closed. This is the continuous bridge from live window to book.
      vec2 fallingBook = p - bookCenter;
      fallingBook = rotate2d(fallingBook, -0.58 * fall + 0.045 * sin(fall * PI * 2.0));
      fallingBook.x *= 1.0 + 0.10 * impactCompression;
      fallingBook.y *= 1.0 - 0.16 * impactCompression;
      fallingBook *= dropScale;
      p = bookCenter + fallingBook + dropOffset;

    } else {
      // Original version keeps its softer curved return path.
      vec2 returnAnchor = vec2(1.03, -1.03);
      vec2 closed = p;
      float shrink = 1.0 - 0.94 * pow(returnHome, 1.18);
      p = mix(closed, returnAnchor + (closed - returnAnchor) * shrink, returnHome);
      float trail = sin(PI * returnHome);
      p.x += 0.070 * trail * (1.0 - returnHome);
      p.y += 0.038 * sin(returnHome * PI * 2.0) * trail;
    }
  }

  gl_Position = vec4(p, vertexDepth, 1.0);
  v_uv = a_uv;
}
`;

const FRAGMENT_SHADER = `#version 300 es
precision highp float;
uniform sampler2D u_texture;
uniform float u_progress;
uniform float u_mode;
in vec2 v_uv;
in float v_shade;
out vec4 fragColor;

const float PI = 3.141592653589793;

void main() {
  float t = smoothstep(0.0, 1.0, u_progress);
  vec2 uv = vec2(v_uv.x, 1.0 - v_uv.y);
  vec4 color;

  if (u_mode < 0.5) {
    float ripple = sin((uv.y * 5.0 + t * 1.6) * PI) * 0.0045 * sin(PI * t);
    color = texture(u_texture, vec2(clamp(uv.x + ripple, 0.0, 1.0), uv.y));
  } else if (u_mode < 1.5) {
    float split = 0.0045 * sin(PI * t);
    vec4 center = texture(u_texture, uv);
    float red = texture(u_texture, vec2(clamp(uv.x + split, 0.0, 1.0), uv.y)).r;
    float blue = texture(u_texture, vec2(clamp(uv.x - split, 0.0, 1.0), uv.y)).b;
    float sheen = sin((uv.y * 8.0 + t * 2.0) * PI) * 0.018 * sin(PI * t);
    color = vec4(red + sheen, center.g + sheen, blue + sheen, center.a);
  } else if (u_mode < 2.5) {
    float split = 0.0032 * sin(PI * t);
    vec4 center = texture(u_texture, uv);
    color = vec4(
      texture(u_texture, vec2(clamp(uv.x + split, 0.0, 1.0), uv.y)).r,
      center.g,
      texture(u_texture, vec2(clamp(uv.x - split, 0.0, 1.0), uv.y)).b,
      center.a
    );
  } else if (u_mode < 6.5) {
    color = texture(u_texture, uv);
  } else {
    // Book return: keep the live GUI while the page turns, then reveal a stylized
    // Yuumi book cover: burgundy leather, thick gold trim and a blue center gem.
    vec4 page = texture(u_texture, uv);
    float wake = smoothstep(0.00, 0.18, t);
    float closeBook = smoothstep(0.46, 0.72, t);
    float shellMix = smoothstep(0.04, 0.18, t);
    float coverFillMix = smoothstep(0.46, 0.70, t);
    float returnHome = smoothstep(0.80, 1.00, t);
    float edgeDistance = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    float spine = exp(-abs(uv.x - 0.5) * 34.0);

    // Warm the page very slightly before it closes.
    vec3 pageColor = page.rgb * vec3(1.025, 1.012, 0.985);
    pageColor -= spine * 0.065 * (1.0 - closeBook);

    // Deep reddish-brown leather with a subtle uneven sheen.
    vec3 burgundyDark = vec3(0.145, 0.043, 0.032);
    vec3 burgundy = vec3(0.315, 0.095, 0.067);
    float leather = 0.5 + 0.5 * sin(uv.x * 18.0 + sin(uv.y * 13.0) * 0.8);
    float vignette = 1.0 - smoothstep(0.18, 0.72, length(uv - vec2(0.5)));
    vec3 cover = mix(burgundyDark, burgundy, 0.54 + leather * 0.10 + vignette * 0.12);

    // Heavy gold frame and an inner ornamental rail.
    vec3 goldDark = vec3(0.40, 0.275, 0.055);
    vec3 gold = vec3(0.82, 0.645, 0.185);
    vec3 goldHighlight = vec3(1.00, 0.825, 0.34);
    float outerFrame = 1.0 - smoothstep(0.025, 0.062, edgeDistance);
    float innerRail = 1.0 - smoothstep(0.010, 0.025, abs(edgeDistance - 0.105));
    float frameLight = clamp(0.62 + (1.0 - uv.y) * 0.30 + uv.x * 0.10, 0.0, 1.0);
    vec3 frameColor = mix(goldDark, goldHighlight, frameLight);
    cover = mix(cover, frameColor, clamp(max(outerFrame, innerRail * 0.92), 0.0, 1.0));

    // Curving gold ornament around the central gem. It stays stylized so it reads at tray size.
    vec2 ornamentP = (uv - vec2(0.50, 0.49)) * vec2(1.0, 1.18);
    float ornamentR = length(ornamentP);
    float ornamentAngle = atan(ornamentP.y, ornamentP.x);
    float ornamentTarget = 0.275 + sin(ornamentAngle * 2.0 + 0.65) * 0.022;
    float ornament = 1.0 - smoothstep(0.010, 0.026, abs(ornamentR - ornamentTarget));
    ornament *= smoothstep(0.16, 0.22, ornamentR) * (1.0 - smoothstep(0.35, 0.39, ornamentR));
    cover = mix(cover, gold, ornament * 0.88);

    // Gold bezel and saturated blue magical gem.
    vec2 gemP = (uv - vec2(0.50, 0.49)) * vec2(1.45, 1.0);
    float gemR = length(gemP);
    float bezelOuter = 1.0 - smoothstep(0.152, 0.175, gemR);
    float gemMask = 1.0 - smoothstep(0.112, 0.142, gemR);
    float bezelRing = clamp(bezelOuter - gemMask, 0.0, 1.0);
    cover = mix(cover, mix(goldDark, goldHighlight, 0.68 + 0.28 * (1.0 - uv.y)), bezelRing);

    vec3 gemDeep = vec3(0.018, 0.105, 0.46);
    vec3 gemBlue = vec3(0.025, 0.31, 0.94);
    vec3 gemCyan = vec3(0.22, 0.72, 1.00);
    float gemDepth = clamp(1.0 - gemR / 0.145, 0.0, 1.0);
    vec3 gemColor = mix(gemDeep, gemBlue, gemDepth);
    float gemHighlight = exp(-length((uv - vec2(0.465, 0.445)) * vec2(2.0, 2.8)) * 34.0);
    gemColor = mix(gemColor, gemCyan, gemHighlight * 0.92);
    cover = mix(cover, gemColor, gemMask);

    // The gem flashes after the cover has formed, then leads the book out.
    float gemPulse = sin(PI * smoothstep(0.66, 0.88, t)) * (1.0 - returnHome);
    cover += vec3(0.05, 0.22, 0.82) * gemMask * gemPulse * 0.42;
    float halo = exp(-gemR * 9.0) * gemPulse;
    cover += vec3(0.04, 0.12, 0.42) * halo * 0.28;

    // Pale lower strip hints at the thick page block visible on the reference book.
    float pageBlock = smoothstep(0.84, 0.90, uv.y) * (1.0 - smoothstep(0.94, 0.985, uv.y));
    pageBlock *= smoothstep(0.08, 0.14, uv.x) * (1.0 - smoothstep(0.86, 0.92, uv.x));
    cover = mix(cover, vec3(0.70, 0.60, 0.33), pageBlock * 0.72);

    color = vec4(mix(pageColor, cover, coverFillMix), page.a);
    if (u_mode > 7.5) {
      // V2 starts as an already-bound open book. The folding right half owns
      // its leather cover tint and gold rim from the first phase, so the final
      // cover is a continuation of that same surface rather than a late swap.
      float rightCover = smoothstep(0.49, 0.515, uv.x);
      vec2 rightUv = vec2(clamp((uv.x - 0.5) * 2.0, 0.0, 1.0), uv.y);
      float rightEdge = min(min(rightUv.x, 1.0 - rightUv.x), min(rightUv.y, 1.0 - rightUv.y));
      float rightOuterFrame = 1.0 - smoothstep(0.030, 0.085, rightEdge);
      vec3 boundPage = mix(color.rgb, vec3(0.27, 0.072, 0.050), 0.18 * shellMix);
      color.rgb = mix(color.rgb, boundPage, rightCover * (1.0 - coverFillMix));
      color.rgb = mix(color.rgb, frameColor, rightCover * rightOuterFrame * shellMix);
      float earlySpine = exp(-abs(uv.x - 0.5) * 46.0) * shellMix;
      color.rgb = mix(color.rgb, vec3(0.49, 0.31, 0.075), earlySpine * 0.72);
    } else {
      float earlyShell = clamp(max(outerFrame, innerRail * 0.92), 0.0, 1.0) * shellMix;
      color.rgb = mix(color.rgb, frameColor, earlyShell);
    }
    color.rgb += vec3(0.10, 0.07, 0.025) * outerFrame * coverFillMix * 0.12;
    color.rgb += vec3(0.12, 0.20, 0.55) * gemMask * gemPulse * 0.18;
  }

  color.rgb = clamp(color.rgb + v_shade, 0.0, 1.0);
  float alpha = 1.0 - smoothstep(0.76, 1.0, t);
  if (u_mode > 5.5 && u_mode < 6.5) alpha = 1.0 - smoothstep(0.70, 1.0, t);
  if (u_mode > 6.5) alpha = 1.0 - smoothstep(0.90, 1.0, t);
  if (u_mode > 7.5) alpha *= 1.0 - smoothstep(0.58, 0.72, t);
  fragColor = vec4(color.rgb, color.a * alpha);
}
`;

const BOOK_VERTEX_SHADER = `#version 300 es
precision highp float;
in vec3 a_book_position;
in vec2 a_book_uv;
in float a_book_face;
uniform float u_progress;
out vec2 v_book_uv;
flat out float v_book_face;
out float v_solid_reveal;

const float PI = 3.141592653589793;

vec2 rotate2d(vec2 value, float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return mat2(c, -s, s, c) * value;
}

vec3 rotateX3d(vec3 value, float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return vec3(value.x, value.y * c - value.z * s, value.y * s + value.z * c);
}

vec3 rotateY3d(vec3 value, float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return vec3(value.x * c + value.z * s, value.y, -value.x * s + value.z * c);
}

vec3 rotateAxis3d(vec3 value, vec3 axis, float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return value * c + cross(axis, value) * s + axis * dot(axis, value) * (1.0 - c);
}

void main() {
  // Front-load the motion so the close button receives an immediate visual
  // response, while retaining a little more time for the airborne silhouette.
  float t = pow(smoothstep(0.0, 1.0, u_progress), 0.82);
  // Begin as a shallow solid instead of a mathematically flat sheet. A tiny
  // inset and camera pose expose the cover edge immediately while keeping the
  // captured WebView aligned closely enough to hide the hand-off.
  float formBook = mix(0.06, 1.0, smoothstep(0.02, 0.26, t));
  float solidReveal = mix(0.45, 1.0, smoothstep(0.01, 0.30, t));
  float pose3d = mix(0.10, 1.0, smoothstep(0.00, 0.26, t));
  float entryLeanPhase = smoothstep(0.00, 0.22, t);
  float entryLean = sin(PI * entryLeanPhase);
  float wake = smoothstep(0.03, 0.16, t);
  // Close quickly, but release the book almost immediately. Folding, upward
  // momentum and tumbling now overlap as one gesture instead of playing as a
  // close animation followed by a separate drop animation.
  float turn = smoothstep(0.02, 0.32, t);
  float closeBook = smoothstep(0.05, 0.35, t);
  float fall = smoothstep(0.00, 0.80, t);
  float gravity = fall * fall;
  float impact = smoothstep(0.71, 0.80, t);
  float rebound = sin(PI * impact) * 0.055;
  float impactCompression = sin(PI * impact);
  float vanish = smoothstep(0.74, 0.80, t);
  // Keep perspective shrinkage moving throughout the fall. Finishing the
  // shrink halfway through the flight made the book appear to hit an
  // invisible depth plane before it landed.
  float flightScale = 1.0 / (1.0 + 0.78 * fall);
  float dropScale = flightScale * (1.0 - 0.28 * pow(vanish, 1.30));
  // Monotonic angular travel: the sine term adds an energetic middle phase,
  // but its derivative stays positive so the book never subtly reverses.
  float angularTravel = fall + 0.20 * sin(PI * fall);
  // A small, bounded sideways glide reads as release momentum. The previous
  // quadratic term accelerated hard to the right near the end and looked like
  // the book was being pulled toward an off-screen target.
  float horizontalGlide = 0.22 * (fall + 0.12 * sin(PI * fall));
  vec2 dropOffset = vec2(
    horizontalGlide,
    0.98 * fall - 1.68 * gravity + rebound
  );

  // The front remains the captured WebView while the already-solid mesh
  // contracts into the final book proportions.
  vec3 p = a_book_position;
  p.xy *= mix(vec2(1.0), vec2(0.86, 0.72), formBook);
  p.z *= solidReveal;
  // Every vertex on the right half participates in the same hinge, including
  // the page, cover and thickness faces. At 180 degrees the cover that was
  // physically beneath the page becomes the visible outer face; the captured
  // page itself never changes material.
  float rightHalf = step(0.0001, p.x);
  float hingeAngle = -turn * PI;
  vec3 folded = p;
  folded.x = p.x * cos(hingeAngle) - p.z * sin(hingeAngle);
  folded.z = p.x * sin(hingeAngle) + p.z * cos(hingeAngle);
  folded.y += p.x * sin(hingeAngle) * (0.5 - a_book_uv.y) * 0.070;
  p = mix(p, folded, rightHalf);

  if (a_book_face > 0.5 && a_book_face < 1.5 && rightHalf > 0.5) {
    // Let the physical cover overlap the page block by a small lip once shut.
    // This removes the artificial white seam without recoloring the page.
    float coverSeal = smoothstep(0.28, 0.42, t);
    p.x = mix(p.x, -0.43 + (p.x + 0.43) * 1.055, coverSeal);
    p.y *= mix(1.0, 1.020, coverSeal);
    p.z -= 0.010 * coverSeal;
  }

  if (a_book_face < 0.5) {
    float spineDistance = abs(a_book_uv.x - 0.5);
    p.z -= exp(-spineDistance * 13.0) * 0.040 * sin(PI * wake) * (1.0 - closeBook);
  }

  // Once the right cover folds onto the left, the book spans x=-0.86..0.
  // Move that physical centre of mass to the origin before every rigid-body
  // transform so it tumbles as one book instead of orbiting its hinge.
  p.x += 0.43 * closeBook;
  p.x *= 1.0 + 0.10 * impactCompression;
  p.y *= 1.0 - 0.16 * impactCompression;
  // Establish the closed-book pose, then drive the flight with one diagonal
  // inertial axis. A single rigid-body rotation avoids the mechanical look of
  // three unrelated Euler curves while still exposing spine and page depth.
  p = rotateX3d(p, pose3d * 0.10 + entryLean * 0.035);
  p = rotateY3d(p, pose3d * -0.18 - entryLean * 0.045);
  p.xy = rotate2d(p.xy, pose3d * -0.095 - entryLean * 0.018);
  // The reference motion is an upward toss followed by an end-over-end fall,
  // not a flat clockwise spin. Biasing the axis into X/Y exposes the page
  // block and spine, while a short secondary kick prevents a robotic arc.
  vec3 tumbleAxis = normalize(vec3(0.72, -0.44, 0.53));
  float tumbleAngle = 2.30 * angularTravel;
  p = rotateAxis3d(p, tumbleAxis, tumbleAngle);
  vec3 kickAxis = normalize(vec3(-0.12, 0.92, 0.35));
  p = rotateAxis3d(p, kickAxis, 0.32 * sin(PI * fall));
  p *= dropScale;

  float perspective = 1.0 / (1.0 + p.z * 0.42);
  vec2 bookCenter = vec2(0.02 * formBook, -0.02 * formBook);
  vec2 projected = bookCenter + p.xy * perspective + dropOffset;
  gl_Position = vec4(projected, p.z * 0.45, 1.0);
  v_book_uv = a_book_uv;
  v_book_face = a_book_face;
  v_solid_reveal = solidReveal;
}
`;

const BOOK_FRAGMENT_SHADER = `#version 300 es
precision highp float;
uniform sampler2D u_texture;
uniform float u_progress;
in vec2 v_book_uv;
flat in float v_book_face;
in float v_solid_reveal;
out vec4 fragColor;

const float PI = 3.141592653589793;

float leafShape(vec2 p, vec2 center, vec2 radius, float angle) {
  float c = cos(angle);
  float s = sin(angle);
  vec2 q = p - center;
  q = mat2(c, -s, s, c) * q;
  q /= radius;
  // Taper both ends so the mark reads as a forged leaf, not an oval.
  float body = length(q) + 0.20 * abs(q.y) * abs(q.x);
  return 1.0 - smoothstep(0.80, 1.0, body);
}

vec3 bookCover(vec2 uv) {
  float edgeDistance = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
  float leather = 0.5 + 0.5 * sin(uv.x * 18.0 + sin(uv.y * 13.0) * 0.8);
  vec3 cover = mix(
    vec3(0.145, 0.043, 0.032),
    vec3(0.315, 0.095, 0.067),
    0.56 + leather * 0.10
  );
  float outerFrame = 1.0 - smoothstep(0.025, 0.070, edgeDistance);
  float innerRail = 1.0 - smoothstep(0.010, 0.026, abs(edgeDistance - 0.105));
  vec3 gold = mix(
    vec3(0.34, 0.205, 0.040),
    vec3(0.86, 0.665, 0.20),
    0.55 + uv.x * 0.18 + (1.0 - uv.y) * 0.20
  );
  float metalGrain = 0.94 + 0.06 * sin((uv.x + uv.y) * 52.0);
  gold *= metalGrain;
  cover = mix(cover, gold, clamp(max(outerFrame, innerRail * 0.92), 0.0, 1.0));

  // Subtle pressed leather scrollwork, visible mainly while the cover is
  // close. It adds material detail without competing with the centre jewel.
  float leatherScroll = pow(
    0.5 + 0.5 * sin(uv.x * 31.0 + sin(uv.y * 22.0) * 1.35),
    14.0
  ) * smoothstep(0.12, 0.20, edgeDistance);
  cover += vec3(0.040, 0.012, 0.006) * leatherScroll;

  // Mirrored, connected filigree along the outer rails. Only the rims of the
  // leaves are gilded so the ornament feels engraved into the cover instead
  // of looking like flat pieces pasted on top.
  vec2 sideUv = vec2(min(uv.x, 1.0 - uv.x), uv.y);
  float leafOuter = 0.0;
  float leafInner = 0.0;
  leafOuter = max(leafOuter, leafShape(sideUv, vec2(0.108, 0.26), vec2(0.025, 0.060), -0.52));
  leafOuter = max(leafOuter, leafShape(sideUv, vec2(0.108, 0.39), vec2(0.022, 0.052),  0.52));
  leafOuter = max(leafOuter, leafShape(sideUv, vec2(0.108, 0.61), vec2(0.022, 0.052), -0.52));
  leafOuter = max(leafOuter, leafShape(sideUv, vec2(0.108, 0.74), vec2(0.025, 0.060),  0.52));
  leafInner = max(leafInner, leafShape(sideUv, vec2(0.108, 0.26), vec2(0.017, 0.043), -0.52));
  leafInner = max(leafInner, leafShape(sideUv, vec2(0.108, 0.39), vec2(0.015, 0.037),  0.52));
  leafInner = max(leafInner, leafShape(sideUv, vec2(0.108, 0.61), vec2(0.015, 0.037), -0.52));
  leafInner = max(leafInner, leafShape(sideUv, vec2(0.108, 0.74), vec2(0.017, 0.043),  0.52));
  float leafRim = clamp(leafOuter - leafInner * 0.90, 0.0, 1.0);
  float stemX = 0.095 + 0.010 * sin((sideUv.y - 0.5) * PI * 3.0);
  float stem = 1.0 - smoothstep(0.004, 0.010, abs(sideUv.x - stemX));
  stem *= smoothstep(0.19, 0.27, sideUv.y) * (1.0 - smoothstep(0.73, 0.81, sideUv.y));
  vec3 antiqueGold = mix(cover, gold, 0.66);
  cover = mix(cover, antiqueGold, clamp(leafRim * 0.82 + stem * 0.58, 0.0, 1.0));

  vec2 cornerUv = vec2(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
  float cornerStud = 1.0 - smoothstep(0.010, 0.022, length(cornerUv - vec2(0.047)));
  cover = mix(cover, antiqueGold, cornerStud * 0.62);

  vec2 ornamentP = (uv - vec2(0.50, 0.49)) * vec2(1.0, 1.18);
  float ornamentR = length(ornamentP);
  float ornamentAngle = atan(ornamentP.y, ornamentP.x);
  float ornamentTarget = 0.275 + sin(ornamentAngle * 2.0 + 0.65) * 0.022;
  float ornament = 1.0 - smoothstep(0.010, 0.026, abs(ornamentR - ornamentTarget));
  cover = mix(cover, vec3(0.82, 0.645, 0.185), ornament * 0.86);

  vec2 gemP = (uv - vec2(0.50, 0.49)) * vec2(1.45, 1.0);
  float gemR = length(gemP);
  float bezel = 1.0 - smoothstep(0.142, 0.175, gemR);
  float gem = 1.0 - smoothstep(0.112, 0.142, gemR);
  cover = mix(cover, gold, bezel);
  vec3 gemColor = mix(
    vec3(0.018, 0.105, 0.46),
    vec3(0.025, 0.31, 0.94),
    clamp(1.0 - gemR / 0.145, 0.0, 1.0)
  );
  float gemHighlight = exp(-length((uv - vec2(0.465, 0.445)) * vec2(2.0, 2.8)) * 34.0);
  gemColor = mix(gemColor, vec3(0.22, 0.72, 1.00), gemHighlight * 0.92);
  return mix(cover, gemColor, gem);
}

void main() {
  float t = pow(smoothstep(0.0, 1.0, u_progress), 0.82);
  vec2 uv = v_book_uv;
  float edgeDistance = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
  vec3 color;
  float faceAlpha = 1.0;

  // After the right half closes, its side faces become the outside of the
  // book. Matching left-half faces are now internal; drawing both at the same
  // depth creates triangular z-fighting during a dynamic tumble.
  bool foldedInteriorFace = (v_book_face > 1.5 && v_book_face < 2.5)
    || (v_book_face > 3.5 && v_book_face < 5.5);
  if (foldedInteriorFace && t > 0.38) discard;

  if (v_book_face < 0.5) {
    // The captured WebView remains a page for its entire lifetime. It never
    // cross-fades or recolors into leather.
    vec4 page = texture(u_texture, vec2(uv.x, 1.0 - uv.y));
    color = page.rgb;
    // Once the physical cover has swept over the page, only the page-block
    // side faces remain visible. This is occlusion, not a page-to-cover tint.
    faceAlpha = page.a * (1.0 - smoothstep(0.22, 0.36, t));
    // A zero-alpha page must not keep writing depth in front of the cover.
    // Discarding it also prevents a pale texture flash during the final shrink.
    if (faceAlpha < 0.012) discard;
  } else if (v_book_face < 1.5) {
    // The real cover lives underneath the page from the beginning. The right
    // half's underside becomes the front cover as the one solid book closes.
    vec2 coverUv = vec2(clamp((uv.x - 0.5) * 2.0, 0.0, 1.0), uv.y);
    color = bookCover(coverUv);
  } else if (v_book_face < 2.5) {
    // The left side is the structural spine of the same object.
    float spineRail = 0.5 + 0.5 * cos(uv.y * PI * 8.0);
    color = mix(vec3(0.16, 0.040, 0.028), vec3(0.50, 0.31, 0.075), spineRail * 0.34);
  } else if (v_book_face < 3.5) {
    // The folded right edge becomes the closed book's outer spine.
    float spineRail = 0.5 + 0.5 * cos(uv.y * PI * 8.0);
    vec3 pageEdge = mix(vec3(0.49, 0.34, 0.105), vec3(0.82, 0.68, 0.31), spineRail);
    vec3 closedSpine = mix(vec3(0.16, 0.040, 0.028), vec3(0.58, 0.37, 0.09), spineRail * 0.38);
    color = mix(pageEdge, closedSpine, smoothstep(0.26, 0.42, t));
  } else {
    // Right, top and bottom are its recessed solid page block.
    float lineAxis = v_book_face < 3.5 ? uv.y : uv.x;
    float pageLine = 0.5 + 0.5 * sin(lineAxis * PI * 96.0);
    vec3 parchment = mix(vec3(0.49, 0.34, 0.105), vec3(0.82, 0.68, 0.31), 0.34 + pageLine * 0.18);
    float coverLip = 1.0 - smoothstep(0.035, 0.105, min(uv.y, 1.0 - uv.y));
    color = mix(parchment, vec3(0.74, 0.54, 0.12), coverLip);
  }

  if (v_book_face > 0.5) {
    // Solid faces must never be translucent: that made the white UI page
    // bleed into the leather. They are hidden only while depth is exactly flat.
    if (v_solid_reveal < 0.012) discard;
  }

  // End while the object is still a readable solid book. Very low-alpha
  // fragments can be un-premultiplied by WebView capture/composition and flash
  // pale for one frame, while also making the ending feel unnecessarily long.
  if (t > 0.80) discard;
  fragColor = vec4(clamp(color, 0.0, 1.0), faceAlpha);
}
`;

type HtmlCanvasDiagnosticCode =
  | 'surface_invalid'
  | 'webgl2_unavailable'
  | 'api_unavailable'
  | 'shader_compile_failed'
  | 'program_link_failed'
  | 'mesh_failed'
  | 'texture_failed'
  | 'snapshot_failed'
  | 'shader_bindings_missing'
  | 'ready';

function errorDetail(error: unknown) {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

function reportDiagnostic(code: HtmlCanvasDiagnosticCode, detail: string) {
  const normalized = detail.replace(/\s+/g, ' ').trim().slice(0, 400) || 'detail unavailable';
  const key = code;
  if (REPORTED_DIAGNOSTICS.has(key)) return;
  REPORTED_DIAGNOSTICS.add(key);
  const method = code === 'ready' ? console.info : console.warn;
  method(`[HTML-in-Canvas] ${code}: ${normalized}`);
  void reportTrayEffectDiagnostic(code, normalized).catch((error) => {
    console.warn('[HTML-in-Canvas] failed to persist diagnostic', error);
  });
}

function compileShader(
  gl: WebGLRenderingContext,
  type: number,
  source: string,
  label: 'vertex' | 'fragment',
): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) {
    reportDiagnostic('shader_compile_failed', `${label}: createShader returned null`);
    return null;
  }
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    reportDiagnostic(
      'shader_compile_failed',
      `${label}: ${gl.getShaderInfoLog(shader) || 'unknown compiler error'}`,
    );
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

function createProgram(
  gl: WebGLRenderingContext,
  vertexSource = VERTEX_SHADER,
  fragmentSource = FRAGMENT_SHADER,
): WebGLProgram | null {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, vertexSource, 'vertex');
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, fragmentSource, 'fragment');
  if (!vertex || !fragment) {
    if (vertex) gl.deleteShader(vertex);
    if (fragment) gl.deleteShader(fragment);
    return null;
  }

  const program = gl.createProgram();
  if (!program) {
    gl.deleteShader(vertex);
    gl.deleteShader(fragment);
    return null;
  }
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  gl.deleteShader(vertex);
  gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    reportDiagnostic('program_link_failed', gl.getProgramInfoLog(program) || 'unknown linker error');
    gl.deleteProgram(program);
    return null;
  }
  return program;
}

function createMesh(gl: WebGLRenderingContext, columns: number, rows: number) {
  const vertices: number[] = [];
  const indices: number[] = [];
  let vertex = 0;

  for (let row = 0; row < rows; row += 1) {
    const v0 = row / rows;
    const v1 = (row + 1) / rows;
    for (let col = 0; col < columns; col += 1) {
      const u0 = col / columns;
      const u1 = (col + 1) / columns;
      const cellU = (u0 + u1) / 2;
      const cellV = (v0 + v1) / 2;
      const quad = [
        [u0, v0],
        [u0, v1],
        [u1, v0],
        [u1, v1],
      ];
      for (const [u, v] of quad) {
        vertices.push(u * 2 - 1, 1 - v * 2, u, v, cellU, cellV);
      }
      indices.push(vertex, vertex + 1, vertex + 2, vertex + 2, vertex + 1, vertex + 3);
      vertex += 4;
    }
  }

  const vertexBuffer = gl.createBuffer();
  const indexBuffer = gl.createBuffer();
  if (!vertexBuffer || !indexBuffer || vertex > 65_535) return null;

  gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(vertices), gl.STATIC_DRAW);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer);
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(indices), gl.STATIC_DRAW);
  return { vertexBuffer, indexBuffer, indexCount: indices.length };
}

function createBookMesh(gl: WebGLRenderingContext, columns: number, rows: number) {
  const vertices: number[] = [];
  const indices: number[] = [];
  // The front starts at the exact WebView clip-space bounds. The shader turns
  // this same geometry into the smaller book form after the first frame.
  const halfWidth = 1.0;
  const halfHeight = 1.0;
  const halfDepth = 0.135;

  // The textured front is subdivided so the right half can hinge around the
  // center spine while remaining part of this same indexed solid-book draw.
  let frontVertex = 0;
  for (let row = 0; row < rows; row += 1) {
    const v0 = row / rows;
    const v1 = (row + 1) / rows;
    for (let col = 0; col < columns; col += 1) {
      const u0 = col / columns;
      const u1 = (col + 1) / columns;
      const quad: ReadonlyArray<readonly [number, number]> = [
        [u0, v0],
        [u0, v1],
        [u1, v0],
        [u1, v1],
      ];
      for (const [u, v] of quad) {
        vertices.push(
          (u * 2 - 1) * halfWidth,
          (1 - v * 2) * halfHeight,
          -halfDepth,
          u,
          v,
          0,
        );
      }
      indices.push(
        frontVertex,
        frontVertex + 1,
        frontVertex + 2,
        frontVertex + 2,
        frontVertex + 1,
        frontVertex + 3,
      );
      frontVertex += 4;
    }
  }

  const addFace = (face: number, corners: ReadonlyArray<readonly [number, number, number]>) => {
    const first = vertices.length / 6;
    const uvs: ReadonlyArray<readonly [number, number]> = [
      [0, 0],
      [0, 1],
      [1, 0],
      [1, 1],
    ];
    corners.forEach(([x, y, z], index) => {
      const [u, v] = uvs[index];
      vertices.push(x, y, z, u, v, face);
    });
    indices.push(first, first + 1, first + 2, first + 2, first + 1, first + 3);
  };

  // The cover underneath the page uses the same center-hinged subdivision.
  // This prevents one large back quad from shearing diagonally while closing.
  let backVertex = vertices.length / 6;
  for (let row = 0; row < rows; row += 1) {
    const v0 = row / rows;
    const v1 = (row + 1) / rows;
    for (let col = 0; col < columns; col += 1) {
      const u0 = col / columns;
      const u1 = (col + 1) / columns;
      const quad: ReadonlyArray<readonly [number, number]> = [
        [u0, v0],
        [u0, v1],
        [u1, v0],
        [u1, v1],
      ];
      for (const [u, v] of quad) {
        vertices.push(
          (u * 2 - 1) * halfWidth,
          (1 - v * 2) * halfHeight,
          halfDepth,
          u,
          v,
          1,
        );
      }
      indices.push(
        backVertex,
        backVertex + 1,
        backVertex + 2,
        backVertex + 2,
        backVertex + 1,
        backVertex + 3,
      );
      backVertex += 4;
    }
  }
  addFace(2, [
    [-halfWidth, halfHeight, halfDepth],
    [-halfWidth, -halfHeight, halfDepth],
    [-halfWidth, halfHeight, -halfDepth],
    [-halfWidth, -halfHeight, -halfDepth],
  ]);
  addFace(3, [
    [halfWidth, halfHeight, -halfDepth],
    [halfWidth, -halfHeight, -halfDepth],
    [halfWidth, halfHeight, halfDepth],
    [halfWidth, -halfHeight, halfDepth],
  ]);
  addFace(4, [
    [-halfWidth, halfHeight, halfDepth],
    [-halfWidth, halfHeight, -halfDepth],
    [0, halfHeight, halfDepth],
    [0, halfHeight, -halfDepth],
  ]);
  addFace(6, [
    [0, halfHeight, halfDepth],
    [0, halfHeight, -halfDepth],
    [halfWidth, halfHeight, halfDepth],
    [halfWidth, halfHeight, -halfDepth],
  ]);
  addFace(5, [
    [-halfWidth, -halfHeight, -halfDepth],
    [-halfWidth, -halfHeight, halfDepth],
    [0, -halfHeight, -halfDepth],
    [0, -halfHeight, halfDepth],
  ]);
  addFace(7, [
    [0, -halfHeight, -halfDepth],
    [0, -halfHeight, halfDepth],
    [halfWidth, -halfHeight, -halfDepth],
    [halfWidth, -halfHeight, halfDepth],
  ]);

  const vertexBuffer = gl.createBuffer();
  const indexBuffer = gl.createBuffer();
  if (!vertexBuffer || !indexBuffer || vertices.length / 6 > 65_535) return null;
  gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(vertices), gl.STATIC_DRAW);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer);
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(indices), gl.STATIC_DRAW);
  return { vertexBuffer, indexBuffer, indexCount: indices.length };
}

function nextFrame() {
  return new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
}

function syncSnapshotState(source: HTMLElement, clone: HTMLElement) {
  const sourceElements = [source, ...source.querySelectorAll<HTMLElement>('*')];
  const cloneElements = [clone, ...clone.querySelectorAll<HTMLElement>('*')];
  const count = Math.min(sourceElements.length, cloneElements.length);

  for (let index = 0; index < count; index += 1) {
    const live = sourceElements[index];
    const copy = cloneElements[index];
    copy.scrollTop = live.scrollTop;
    copy.scrollLeft = live.scrollLeft;

    if (live instanceof HTMLInputElement && copy instanceof HTMLInputElement) {
      copy.value = live.value;
      copy.checked = live.checked;
      copy.indeterminate = live.indeterminate;
    } else if (live instanceof HTMLTextAreaElement && copy instanceof HTMLTextAreaElement) {
      copy.value = live.value;
      copy.textContent = live.value;
    } else if (live instanceof HTMLSelectElement && copy instanceof HTMLSelectElement) {
      copy.value = live.value;
      Array.from(copy.options).forEach((option, optionIndex) => {
        option.selected = live.options[optionIndex]?.selected ?? false;
      });
    } else if (live instanceof HTMLDetailsElement && copy instanceof HTMLDetailsElement) {
      copy.open = live.open;
    } else if (live instanceof HTMLImageElement && copy instanceof HTMLImageElement) {
      // A cloned responsive/lazy image can otherwise miss the first snapshot,
      // making the handoff visibly different from the window the user closed.
      if (live.currentSrc) copy.src = live.currentSrc;
      copy.loading = 'eager';
      copy.decoding = 'sync';
    }
  }
}

async function waitForSnapshotAssets(clone: HTMLElement) {
  const images = Array.from(clone.querySelectorAll('img'));
  if (images.length === 0) return;
  const loaded = Promise.all(images.map(async (image) => {
    if (!image.complete) {
      await new Promise<void>((resolve) => {
        image.addEventListener('load', () => resolve(), { once: true });
        image.addEventListener('error', () => resolve(), { once: true });
      });
    }
    try {
      await image.decode();
    } catch {
      // A broken or cross-origin image should not block the transition.
    }
  }));
  await Promise.race([
    loaded,
    new Promise<void>((resolve) => window.setTimeout(resolve, SNAPSHOT_TIMEOUT_MS)),
  ]);
}

async function waitForElementSnapshot(
  canvas: HTMLCanvasElement,
  gl: HtmlCanvasWebGlContext,
  source: HTMLElement,
  texture: WebGLTexture,
): Promise<boolean> {
  let lastFailure = '';
  const upload = () => {
    const texElementImage2D = gl.texElementImage2D as ((...args: unknown[]) => void) | undefined;
    if (typeof texElementImage2D !== 'function') return false;

    gl.bindTexture(gl.TEXTURE_2D, texture);
    // Clear stale GL errors so a previous unrelated WebGL call cannot make a
    // successful HTML snapshot look like a failure.
    while (gl.getError() !== gl.NO_ERROR) {
      // Drain the error queue.
    }

    // Chromium changed texElementImage2D during the developer trial. Newer
    // WebView2 runtimes use (target, internalFormat, element). Try that first.
    try {
      texElementImage2D.call(gl, gl.TEXTURE_2D, GL_RGBA8, source);
      const glError = gl.getError();
      if (glError === gl.NO_ERROR) return true;
      lastFailure = `3-argument API returned WebGL error 0x${glError.toString(16)}`;
    } catch (error) {
      lastFailure = `3-argument API threw ${errorDetail(error)}`;
      // Fall through to the legacy signature below.
    }

    // Older WebView2 runtimes still expose the original WebGL-style signature.
    try {
      while (gl.getError() !== gl.NO_ERROR) {
        // Drain any error raised by the unsupported new signature.
      }
      texElementImage2D.call(
        gl,
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        source,
      );
      const glError = gl.getError();
      if (glError === gl.NO_ERROR) return true;
      lastFailure = `legacy API returned WebGL error 0x${glError.toString(16)}`;
      return false;
    } catch (error) {
      lastFailure = `legacy API threw ${errorDetail(error)}`;
      return false;
    }
  };

  let painted = false;
  const onPaint = () => {
    painted = upload() || painted;
  };
  canvas.addEventListener('paint', onPaint);
  const requestPaint = (canvas as HTMLCanvasElement & { requestPaint?: () => void }).requestPaint;
  if (typeof requestPaint === 'function') {
    try {
      requestPaint.call(canvas);
    } catch {
      // Early implementations can expose requestPaint before it is fully wired.
    }
  }

  await nextFrame();
  await nextFrame();
  if (!painted) painted = upload();

  if (!painted) {
    const deadline = performance.now() + SNAPSHOT_TIMEOUT_MS;
    while (!painted && performance.now() < deadline) {
      await nextFrame();
      painted = upload();
    }
  }

  canvas.removeEventListener('paint', onPaint);
  if (!painted) {
    reportDiagnostic('snapshot_failed', lastFailure || 'no paint event or usable element snapshot');
  }
  return painted;
}

function prepareCanvas(surface: HTMLElement) {
  const rect = surface.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) return null;

  const canvas = document.createElement('canvas');
  canvas.setAttribute('layoutsubtree', '');
  canvas.setAttribute('aria-hidden', 'true');
  const dpr = Math.max(1, window.devicePixelRatio || 1);
  canvas.width = Math.max(1, Math.round(rect.width * dpr));
  canvas.height = Math.max(1, Math.round(rect.height * dpr));
  Object.assign(canvas.style, {
    position: 'fixed',
    left: `${rect.left}px`,
    top: `${rect.top}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
    zIndex: '100100',
    pointerEvents: 'none',
    margin: '0',
    background: 'transparent',
    transformOrigin: '100% 100%',
  });

  const clone = surface.cloneNode(true) as HTMLElement;
  clone.removeAttribute('data-yummi-app-surface');
  clone.setAttribute('aria-hidden', 'true');
  Object.assign(clone.style, {
    width: `${rect.width}px`,
    height: `${rect.height}px`,
    margin: '0',
    transform: 'none',
    opacity: '1',
    filter: 'none',
  });
  syncSnapshotState(surface, clone);
  canvas.appendChild(clone);
  return { canvas, clone };
}

export async function playHtmlCanvasTrayEffect(
  surface: HTMLElement,
  effect: HtmlCanvasTrayEffect,
  cleanup: HTMLElement[],
  playbackRate = 1,
): Promise<boolean> {
  const spec = EFFECTS[effect];
  const prepared = prepareCanvas(surface);
  if (!prepared) {
    reportDiagnostic('surface_invalid', 'app surface has no drawable dimensions');
    return false;
  }
  const { canvas, clone } = prepared;

  const gl = canvas.getContext('webgl2', {
    alpha: true,
    antialias: true,
    premultipliedAlpha: true,
    preserveDrawingBuffer: false,
  }) as HtmlCanvasWebGlContext | null;
  if (!gl) {
    reportDiagnostic('webgl2_unavailable', 'canvas.getContext(webgl2) returned null');
    return false;
  }
  if (typeof gl.texElementImage2D !== 'function') {
    reportDiagnostic('api_unavailable', 'WebGL2RenderingContext.texElementImage2D is not exposed');
    return false;
  }

  const program = createProgram(gl);
  const mesh = createMesh(gl, spec.grid[0], spec.grid[1]);
  const bookProgram = effect === 'book-return-v2'
    ? createProgram(gl, BOOK_VERTEX_SHADER, BOOK_FRAGMENT_SHADER)
    : null;
  const bookMesh = effect === 'book-return-v2'
    ? createBookMesh(gl, spec.grid[0], spec.grid[1])
    : null;
  const texture = gl.createTexture();
  if (!program) return false;
  if (!mesh) {
    reportDiagnostic('mesh_failed', `could not allocate ${spec.grid[0]}x${spec.grid[1]} mesh`);
    return false;
  }
  if (effect === 'book-return-v2' && (!bookProgram || !bookMesh)) {
    reportDiagnostic('mesh_failed', 'could not allocate the dedicated 24-vertex solid book mesh');
    return false;
  }
  if (!texture) {
    reportDiagnostic('texture_failed', 'createTexture returned null');
    return false;
  }

  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);

  document.body.appendChild(canvas);
  await waitForSnapshotAssets(clone);
  syncSnapshotState(surface, clone);
  const snapshotReady = await waitForElementSnapshot(canvas, gl, clone, texture);
  if (!snapshotReady) {
    canvas.remove();
    return false;
  }

  const position = gl.getAttribLocation(program, 'a_position');
  const uv = gl.getAttribLocation(program, 'a_uv');
  const cell = gl.getAttribLocation(program, 'a_cell');
  const progress = gl.getUniformLocation(program, 'u_progress');
  const modeUniform = gl.getUniformLocation(program, 'u_mode');
  const textureUniform = gl.getUniformLocation(program, 'u_texture');
  if (position < 0 || uv < 0 || cell < 0 || !progress || !modeUniform || !textureUniform) {
    reportDiagnostic('shader_bindings_missing', 'required attribute or uniform was optimized out');
    canvas.remove();
    return false;
  }

  const bookPosition = bookProgram ? gl.getAttribLocation(bookProgram, 'a_book_position') : -1;
  const bookUv = bookProgram ? gl.getAttribLocation(bookProgram, 'a_book_uv') : -1;
  const bookFace = bookProgram ? gl.getAttribLocation(bookProgram, 'a_book_face') : -1;
  const bookProgress = bookProgram ? gl.getUniformLocation(bookProgram, 'u_progress') : null;
  const bookTextureUniform = bookProgram ? gl.getUniformLocation(bookProgram, 'u_texture') : null;
  if (
    effect === 'book-return-v2'
    && (bookPosition < 0 || bookUv < 0 || bookFace < 0 || !bookProgress || !bookTextureUniform)
  ) {
    reportDiagnostic('shader_bindings_missing', 'solid book shader bindings are missing');
    canvas.remove();
    return false;
  }

  cleanup.push(canvas);
  surface.style.opacity = '0';

  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, mesh.vertexBuffer);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, mesh.indexBuffer);
  gl.enableVertexAttribArray(position);
  gl.enableVertexAttribArray(uv);
  gl.enableVertexAttribArray(cell);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 24, 0);
  gl.vertexAttribPointer(uv, 2, gl.FLOAT, false, 24, 8);
  gl.vertexAttribPointer(cell, 2, gl.FLOAT, false, 24, 16);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.uniform1i(textureUniform, 0);
  gl.uniform1f(modeUniform, spec.mode);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
  if (effect === 'book-return-v2') {
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
  }

  const startedAt = performance.now();
  reportDiagnostic(
    'ready',
    `effect=${effect}; rate=${playbackRate.toFixed(2)}; WebGL2; texElementImage2D.length=${gl.texElementImage2D.length}${effect === 'book-return-v2' ? '; solid-book-one-draw' : ''}`,
  );
  await new Promise<void>((resolve) => {
    const render = (now: number) => {
      const duration = spec.duration / Math.min(4, Math.max(0.1, playbackRate));
      const raw = Math.min(1, Math.max(0, (now - startedAt) / duration));
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

      if (
        effect === 'book-return-v2'
        && bookProgram
        && bookMesh
        && bookProgress
        && bookTextureUniform
      ) {
        // V2 is one solid, indexed mesh from its first frame to its last. The
        // captured window is the front face of that same mesh; the hinge bend,
        // cover material, thickness, rotation and fall never create a second
        // page or book object.
        gl.useProgram(bookProgram);
        gl.bindBuffer(gl.ARRAY_BUFFER, bookMesh.vertexBuffer);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, bookMesh.indexBuffer);
        gl.enableVertexAttribArray(bookPosition);
        gl.enableVertexAttribArray(bookUv);
        gl.enableVertexAttribArray(bookFace);
        gl.vertexAttribPointer(bookPosition, 3, gl.FLOAT, false, 24, 0);
        gl.vertexAttribPointer(bookUv, 2, gl.FLOAT, false, 24, 12);
        gl.vertexAttribPointer(bookFace, 1, gl.FLOAT, false, 24, 20);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.uniform1i(bookTextureUniform, 0);
        gl.uniform1f(bookProgress, raw);
        gl.drawElements(gl.TRIANGLES, bookMesh.indexCount, gl.UNSIGNED_SHORT, 0);
      } else {
        gl.useProgram(program);
        gl.bindBuffer(gl.ARRAY_BUFFER, mesh.vertexBuffer);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, mesh.indexBuffer);
        gl.enableVertexAttribArray(position);
        gl.enableVertexAttribArray(uv);
        gl.enableVertexAttribArray(cell);
        gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 24, 0);
        gl.vertexAttribPointer(uv, 2, gl.FLOAT, false, 24, 8);
        gl.vertexAttribPointer(cell, 2, gl.FLOAT, false, 24, 16);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.uniform1i(textureUniform, 0);
        gl.uniform1f(modeUniform, spec.mode);
        gl.uniform1f(progress, raw);
        gl.drawElements(gl.TRIANGLES, mesh.indexCount, gl.UNSIGNED_SHORT, 0);
      }
      if (raw < 1) requestAnimationFrame(render);
      else resolve();
    };
    requestAnimationFrame(render);
  });

  return true;
}
