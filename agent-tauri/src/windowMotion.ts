export type WindowMotionPayload = {
  vx: number;
  vy: number;
  speed: number;
  angle: number;
  angularVelocity: number;
  phase: 'drag' | 'glide' | 'settle' | 'stop';
};

const INFINITY_SLIDER_POSITION = 100;
const MOTION_EFFECT_THRESHOLD_PX_S = 1_400;
const MOTION_EFFECT_FULL_PX_S = 6_400;

export function glideStrengthToSlider(strength: number | null): number {
  if (strength === null) return INFINITY_SLIDER_POSITION;
  if (!Number.isFinite(strength) || strength <= 0) return 0;
  return Math.min(99.9, (100 * strength) / (1 + strength));
}

export function sliderToGlideStrength(position: number): number | null {
  const clamped = Math.min(INFINITY_SLIDER_POSITION, Math.max(0, position));
  if (clamped >= 99.95) return null;
  const ratio = clamped / 100;
  return ratio / (1 - ratio);
}

export function formatGlideStrength(strength: number | null): string {
  if (strength === null) return '∞';
  if (strength === 0) return '0';
  if (strength < 0.1) return `${strength.toFixed(2)}×`;
  if (strength < 10) return `${strength.toFixed(1)}×`;
  if (strength < 100) return `${Math.round(strength)}×`;
  return `${Math.round(strength).toLocaleString()}×`;
}

function rotationDeviation(angle: number): number {
  const normalized = ((angle % 360) + 360) % 360;
  return Math.min(normalized, 360 - normalized);
}

function motionSurface(): HTMLElement {
  return (
    document.querySelector<HTMLElement>('[data-yummi-app-surface]') ?? document.documentElement
  );
}

function setMotionEffectsActive(active: boolean) {
  document.documentElement.classList.toggle('yummi-motion-effects-active', active);
}

function setRotationActive(active: boolean) {
  document.documentElement.classList.toggle('yummi-window-rotating', active);
}

export function updateWindowMotionVisual(payload: WindowMotionPayload) {
  const root = motionSurface();
  root.style.setProperty('--window-rotation-angle', `${payload.angle.toFixed(3)}deg`);

  const rotating =
    rotationDeviation(payload.angle) > 0.1 ||
    Math.abs(payload.angularVelocity) > 0.5 ||
    payload.phase === 'settle';
  setRotationActive(rotating);

  const effectsAllowed =
    !rotating &&
    payload.phase !== 'stop' &&
    payload.speed >= MOTION_EFFECT_THRESHOLD_PX_S &&
    !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

  setMotionEffectsActive(effectsAllowed);
  if (!effectsAllowed) {
    root.style.setProperty('--window-motion-opacity', '0');
    root.style.setProperty('--window-motion-opacity2', '0');
    root.style.setProperty('--window-motion-opacity3', '0');
    root.style.setProperty('--window-motion-streak-opacity', '0');
    return;
  }

  const speed = Math.max(payload.speed, 1);
  const nx = payload.vx / speed;
  const ny = payload.vy / speed;
  const intensity = Math.min(
    1,
    Math.max(
      0,
      (payload.speed - MOTION_EFFECT_THRESHOLD_PX_S) /
        (MOTION_EFFECT_FULL_PX_S - MOTION_EFFECT_THRESHOLD_PX_S),
    ),
  );
  const trailingX = -nx;
  const trailingY = -ny;
  const angle = (Math.atan2(payload.vy, payload.vx) * 180) / Math.PI + 90;
  const primaryOpacity = 0.08 + intensity * 0.34;

  root.style.setProperty('--window-motion-opacity', primaryOpacity.toFixed(3));
  root.style.setProperty('--window-motion-opacity2', (primaryOpacity * 0.62).toFixed(3));
  root.style.setProperty('--window-motion-opacity3', (primaryOpacity * 0.34).toFixed(3));
  root.style.setProperty('--window-motion-streak-opacity', (intensity * 0.18).toFixed(3));
  root.style.setProperty('--window-motion-blur', `${(1.5 + intensity * 4.5).toFixed(2)}px`);
  root.style.setProperty('--window-motion-angle', `${angle.toFixed(2)}deg`);
  root.style.setProperty(
    '--window-motion-x1',
    `${(trailingX * (5 + intensity * 7)).toFixed(2)}px`,
  );
  root.style.setProperty(
    '--window-motion-y1',
    `${(trailingY * (5 + intensity * 7)).toFixed(2)}px`,
  );
  root.style.setProperty(
    '--window-motion-x2',
    `${(trailingX * (12 + intensity * 12)).toFixed(2)}px`,
  );
  root.style.setProperty(
    '--window-motion-y2',
    `${(trailingY * (12 + intensity * 12)).toFixed(2)}px`,
  );
  root.style.setProperty(
    '--window-motion-x3',
    `${(trailingX * (21 + intensity * 19)).toFixed(2)}px`,
  );
  root.style.setProperty(
    '--window-motion-y3',
    `${(trailingY * (21 + intensity * 19)).toFixed(2)}px`,
  );
}

export function resetWindowMotionVisual() {
  const root = motionSurface();
  setMotionEffectsActive(false);
  setRotationActive(false);
  root.style.setProperty('--window-motion-opacity', '0');
  root.style.setProperty('--window-motion-opacity2', '0');
  root.style.setProperty('--window-motion-opacity3', '0');
  root.style.setProperty('--window-motion-streak-opacity', '0');
  root.style.setProperty('--window-rotation-angle', '0deg');
}
