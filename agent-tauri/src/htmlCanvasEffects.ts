export type HtmlCanvasTrayEffect =
  | 'fold'
  | 'glass'
  | 'swirl'
  | 'suction'
  | 'page-curl'
  | 'curtain'
  | 'shards'
  | 'book-return';

type HtmlCanvasWebGlContext = WebGLRenderingContext & {
  texElementImage2D?: (
    target: number,
    level: number,
    internalFormat: number,
    format: number,
    type: number,
    source: Element,
  ) => void;
};

type EffectSpec = {
  mode: number;
  duration: number;
  grid: [number, number];
};

const SNAPSHOT_TIMEOUT_MS = 220;
const EFFECTS: Record<HtmlCanvasTrayEffect, EffectSpec> = {
  fold: { mode: 0, duration: 620, grid: [24, 24] },
  glass: { mode: 1, duration: 760, grid: [24, 24] },
  swirl: { mode: 2, duration: 850, grid: [28, 28] },
  suction: { mode: 3, duration: 780, grid: [28, 28] },
  'page-curl': { mode: 4, duration: 830, grid: [30, 24] },
  curtain: { mode: 5, duration: 790, grid: [28, 28] },
  shards: { mode: 6, duration: 800, grid: [6, 4] },
  'book-return': { mode: 7, duration: 1040, grid: [32, 26] },
};

const VERTEX_SHADER = `
attribute vec2 a_position;
attribute vec2 a_uv;
attribute vec2 a_cell;
uniform float u_progress;
uniform float u_mode;
varying vec2 v_uv;
varying float v_shade;

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

    // Phase 2: the right page curls across the spine like a real page turn.
    float rightPage = smoothstep(0.48, 0.54, a_uv.x);
    float curlFront = 1.10 - turn * 1.32;
    float curl = rightPage * smoothstep(curlFront - 0.22, curlFront + 0.05, a_uv.x);
    float theta = curl * PI * 1.32;
    p.x -= curl * (0.20 + 0.42 * turn);
    p.y += sin(theta) * 0.105 * (0.30 + 0.70 * (1.0 - a_uv.y));
    p.x += (1.0 - cos(theta)) * 0.082;
    v_shade += curl * sin(theta) * 0.48;

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

    // Phase 4: hold the recognizable cover briefly, then return it to the tray on a soft arc.
    vec2 returnAnchor = vec2(1.03, -1.03);
    vec2 closed = p;
    float shrink = 1.0 - 0.94 * pow(returnHome, 1.18);
    p = mix(closed, returnAnchor + (closed - returnAnchor) * shrink, returnHome);
    float trail = sin(PI * returnHome);
    p.x += 0.070 * trail * (1.0 - returnHome);
    p.y += 0.038 * sin(returnHome * PI * 2.0) * trail;
  }

  gl_Position = vec4(p, 0.0, 1.0);
  v_uv = a_uv;
}
`;

const FRAGMENT_SHADER = `
precision mediump float;
uniform sampler2D u_texture;
uniform float u_progress;
uniform float u_mode;
varying vec2 v_uv;
varying float v_shade;

const float PI = 3.141592653589793;

void main() {
  float t = smoothstep(0.0, 1.0, u_progress);
  vec2 uv = vec2(v_uv.x, 1.0 - v_uv.y);
  vec4 color;

  if (u_mode < 0.5) {
    float ripple = sin((uv.y * 5.0 + t * 1.6) * PI) * 0.0045 * sin(PI * t);
    color = texture2D(u_texture, vec2(clamp(uv.x + ripple, 0.0, 1.0), uv.y));
  } else if (u_mode < 1.5) {
    float split = 0.0045 * sin(PI * t);
    vec4 center = texture2D(u_texture, uv);
    float red = texture2D(u_texture, vec2(clamp(uv.x + split, 0.0, 1.0), uv.y)).r;
    float blue = texture2D(u_texture, vec2(clamp(uv.x - split, 0.0, 1.0), uv.y)).b;
    float sheen = sin((uv.y * 8.0 + t * 2.0) * PI) * 0.018 * sin(PI * t);
    color = vec4(red + sheen, center.g + sheen, blue + sheen, center.a);
  } else if (u_mode < 2.5) {
    float split = 0.0032 * sin(PI * t);
    vec4 center = texture2D(u_texture, uv);
    color = vec4(
      texture2D(u_texture, vec2(clamp(uv.x + split, 0.0, 1.0), uv.y)).r,
      center.g,
      texture2D(u_texture, vec2(clamp(uv.x - split, 0.0, 1.0), uv.y)).b,
      center.a
    );
  } else if (u_mode < 6.5) {
    color = texture2D(u_texture, uv);
  } else {
    // Book return: keep the live GUI while the page turns, then reveal a stylized
    // Yuumi book cover: burgundy leather, thick gold trim and a blue center gem.
    vec4 page = texture2D(u_texture, uv);
    float wake = smoothstep(0.00, 0.18, t);
    float closeBook = smoothstep(0.46, 0.72, t);
    float coverMix = smoothstep(0.56, 0.74, t);
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

    color = vec4(mix(pageColor, cover, coverMix), page.a);
    color.rgb += vec3(0.10, 0.07, 0.025) * outerFrame * coverMix * 0.12;
    color.rgb += vec3(0.12, 0.20, 0.55) * gemMask * gemPulse * 0.18;
  }

  color.rgb = clamp(color.rgb + v_shade, 0.0, 1.0);
  float alpha = 1.0 - smoothstep(0.76, 1.0, t);
  if (u_mode > 5.5 && u_mode < 6.5) alpha = 1.0 - smoothstep(0.70, 1.0, t);
  if (u_mode > 6.5) alpha = 1.0 - smoothstep(0.90, 1.0, t);
  gl_FragColor = vec4(color.rgb, color.a * alpha);
}
`;

function compileShader(gl: WebGLRenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

function createProgram(gl: WebGLRenderingContext): WebGLProgram | null {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER);
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER);
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

function nextFrame() {
  return new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
}

async function waitForElementSnapshot(
  canvas: HTMLCanvasElement,
  gl: HtmlCanvasWebGlContext,
  source: HTMLElement,
  texture: WebGLTexture,
): Promise<boolean> {
  const upload = () => {
    if (typeof gl.texElementImage2D !== 'function') return false;
    try {
      gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.texElementImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
      return gl.getError() === gl.NO_ERROR;
    } catch {
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
  canvas.appendChild(clone);
  return { canvas, clone };
}

export async function playHtmlCanvasTrayEffect(
  surface: HTMLElement,
  effect: HtmlCanvasTrayEffect,
  cleanup: HTMLElement[],
): Promise<boolean> {
  const spec = EFFECTS[effect];
  const prepared = prepareCanvas(surface);
  if (!prepared) return false;
  const { canvas, clone } = prepared;

  const gl = canvas.getContext('webgl', {
    alpha: true,
    antialias: true,
    premultipliedAlpha: true,
    preserveDrawingBuffer: false,
  }) as HtmlCanvasWebGlContext | null;
  if (!gl || typeof gl.texElementImage2D !== 'function') return false;

  const program = createProgram(gl);
  const mesh = createMesh(gl, spec.grid[0], spec.grid[1]);
  const texture = gl.createTexture();
  if (!program || !mesh || !texture) return false;

  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

  document.body.appendChild(canvas);
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

  const startedAt = performance.now();
  await new Promise<void>((resolve) => {
    const render = (now: number) => {
      const raw = Math.min(1, Math.max(0, (now - startedAt) / spec.duration));
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.uniform1f(progress, raw);
      gl.drawElements(gl.TRIANGLES, mesh.indexCount, gl.UNSIGNED_SHORT, 0);
      if (raw < 1) requestAnimationFrame(render);
      else resolve();
    };
    requestAnimationFrame(render);
  });

  return true;
}
