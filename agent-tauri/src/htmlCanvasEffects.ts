export type HtmlCanvasTrayEffect =
  | 'fold'
  | 'glass'
  | 'swirl'
  | 'suction'
  | 'page-curl'
  | 'curtain'
  | 'shards';

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
  } else {
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
  } else {
    color = texture2D(u_texture, uv);
  }

  color.rgb = clamp(color.rgb + v_shade, 0.0, 1.0);
  float alpha = 1.0 - smoothstep(0.76, 1.0, t);
  if (u_mode > 5.5) alpha = 1.0 - smoothstep(0.70, 1.0, t);
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
