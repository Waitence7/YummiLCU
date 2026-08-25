import type { TrayHideEffect } from './state/types';

export const TRAY_HIDE_EFFECT_OPTIONS: ReadonlyArray<{
  value: TrayHideEffect;
  label: string;
  description: string;
}> = [
  { value: 'fold', label: '천 접힘', description: '오른쪽 아래로 천처럼 비틀리며 접혀 들어갑니다.' },
  { value: 'jelly', label: '젤리', description: '창이 말랑하게 늘었다가 튕기며 사라집니다.' },
  { value: 'pixels', label: '픽셀 분해', description: '화면이 작은 조각으로 갈라져 트레이 방향으로 흩어집니다.' },
  { value: 'cat', label: '고양이 꼬리', description: '창이 눌리며 Yummi 색상의 꼬리가 휙 지나갑니다.' },
  { value: 'glass', label: '유리 물결', description: '화면이 유리 조각처럼 물결치며 흐려집니다.' },
  { value: 'fade', label: '부드럽게 사라짐', description: '빠르게 축소되며 투명해집니다.' },
  { value: 'none', label: '효과 없음', description: '애니메이션 없이 바로 트레이로 이동합니다.' },
];

const SURFACE_SELECTOR = '[data-yummi-app-surface]';
let running = false;

function wait(ms: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, ms));
}

function animate(
  element: Element,
  frames: Keyframe[],
  options: KeyframeAnimationOptions,
): Promise<void> {
  if (!('animate' in element)) return Promise.resolve();
  const animation = element.animate(frames, options);
  return animation.finished.then(() => undefined).catch(() => undefined);
}

function snapshotInlineStyle(element: HTMLElement) {
  return {
    opacity: element.style.opacity,
    transform: element.style.transform,
    transformOrigin: element.style.transformOrigin,
    filter: element.style.filter,
    clipPath: element.style.clipPath,
    borderRadius: element.style.borderRadius,
    willChange: element.style.willChange,
  };
}

function restoreInlineStyle(element: HTMLElement, previous: ReturnType<typeof snapshotInlineStyle>) {
  Object.assign(element.style, previous);
}

async function fold(surface: HTMLElement) {
  surface.style.transformOrigin = '100% 100%';
  surface.style.willChange = 'transform, opacity, filter, clip-path';
  await animate(
    surface,
    [
      { transform: 'perspective(900px) rotateX(0deg) rotateY(0deg) skew(0deg) scale(1)', opacity: 1, filter: 'blur(0px)' },
      { offset: 0.28, transform: 'perspective(900px) rotateX(-3deg) rotateY(5deg) skewX(-1.5deg) scale(1.01, .985)', opacity: 1 },
      { offset: 0.58, transform: 'perspective(900px) rotateX(8deg) rotateY(-14deg) skewX(5deg) scale(.82, .7)', opacity: .9, filter: 'blur(.3px)' },
      { transform: 'perspective(900px) translate(42%, 36%) rotateX(18deg) rotateY(-28deg) skewX(11deg) scale(.06, .025)', opacity: 0, filter: 'blur(3px)' },
    ],
    { duration: 310, easing: 'cubic-bezier(.3,.8,.2,1)', fill: 'forwards' },
  );
}

async function jelly(surface: HTMLElement) {
  surface.style.transformOrigin = '100% 100%';
  surface.style.willChange = 'transform, opacity, filter';
  await animate(
    surface,
    [
      { transform: 'scale(1,1) translate(0,0)', opacity: 1 },
      { offset: 0.18, transform: 'scale(1.035,.965) translate(-.5%, .6%)' },
      { offset: 0.38, transform: 'scale(.955,1.035) translate(1.5%, -.5%)' },
      { offset: 0.56, transform: 'scale(1.018,.94) translate(4%, 4%)' },
      { offset: 0.72, transform: 'scale(.72,.82) translate(18%, 13%)', opacity: .92 },
      { transform: 'scale(.025,.06) translate(1350%, 760%) rotate(4deg)', opacity: 0, filter: 'blur(2px)' },
    ],
    { duration: 340, easing: 'cubic-bezier(.25,.75,.2,1)', fill: 'forwards' },
  );
}

async function pixels(surface: HTMLElement, cleanup: HTMLElement[]) {
  const rows = 4;
  const cols = 6;
  const promises: Promise<void>[] = [];
  const host = document.body;

  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      const clone = surface.cloneNode(true) as HTMLElement;
      clone.removeAttribute('data-yummi-app-surface');
      clone.setAttribute('aria-hidden', 'true');
      Object.assign(clone.style, {
        position: 'fixed',
        inset: '0',
        zIndex: '99990',
        pointerEvents: 'none',
        margin: '0',
        width: '100vw',
        height: '100vh',
        overflow: 'hidden',
        transformOrigin: '100% 100%',
        willChange: 'transform, opacity, filter',
        clipPath: `inset(${(row * 100) / rows}% ${100 - ((col + 1) * 100) / cols}% ${100 - ((row + 1) * 100) / rows}% ${(col * 100) / cols}%)`,
      });
      host.appendChild(clone);
      cleanup.push(clone);

      const index = row * cols + col;
      const driftX = 12 + col * 6 + ((index * 17) % 13);
      const driftY = 8 + row * 8 + ((index * 11) % 15);
      const rotate = ((index * 19) % 22) - 11;
      promises.push(
        animate(
          clone,
          [
            { transform: 'translate(0,0) scale(1)', opacity: 1, filter: 'blur(0px)' },
            { offset: 0.22, transform: `translate(${col - 2}px, ${row - 1}px) scale(.995)`, opacity: 1 },
            { transform: `translate(${driftX}vw, ${driftY}vh) rotate(${rotate}deg) scale(.12)`, opacity: 0, filter: 'blur(2px)' },
          ],
          {
            duration: 310 + index * 5,
            delay: index * 4,
            easing: 'cubic-bezier(.32,.72,.25,1)',
            fill: 'forwards',
          },
        ),
      );
    }
  }
  surface.style.opacity = '0';
  await Promise.all(promises);
}

function makeTail(cleanup: HTMLElement[]) {
  const tail = document.createElement('div');
  tail.setAttribute('aria-hidden', 'true');
  Object.assign(tail.style, {
    position: 'fixed',
    right: '-8px',
    bottom: '24px',
    zIndex: '100000',
    width: '118px',
    height: '28px',
    borderRadius: '999px 8px 999px 999px',
    background: 'linear-gradient(90deg, rgba(99,102,241,.08), rgba(99,102,241,.9) 45%, rgba(129,140,248,.98))',
    boxShadow: '0 7px 26px rgba(79,70,229,.28)',
    transformOrigin: '100% 50%',
    pointerEvents: 'none',
  });
  document.body.appendChild(tail);
  cleanup.push(tail);
  return tail;
}

async function cat(surface: HTMLElement, cleanup: HTMLElement[]) {
  const tail = makeTail(cleanup);
  surface.style.transformOrigin = '100% 100%';
  surface.style.willChange = 'transform, opacity, filter';
  await Promise.all([
    animate(
      surface,
      [
        { transform: 'translate(0,0) scale(1)', opacity: 1 },
        { offset: 0.2, transform: 'translate(-1%, 1%) scale(1.018,.975) skewX(-1deg)' },
        { offset: 0.48, transform: 'translate(8%, 10%) scale(.88,.7) skewX(5deg)', opacity: .96 },
        { offset: 0.7, transform: 'translate(24%, 25%) scale(.58,.28) skewX(-9deg)', opacity: .72 },
        { transform: 'translate(46%, 43%) scale(.04,.025) skewX(18deg)', opacity: 0, filter: 'blur(2px)' },
      ],
      { duration: 370, easing: 'cubic-bezier(.28,.78,.22,1)', fill: 'forwards' },
    ),
    animate(
      tail,
      [
        { transform: 'translateX(115px) rotate(30deg) scaleX(.2)', opacity: 0 },
        { offset: 0.2, transform: 'translateX(38px) rotate(-16deg) scaleX(.78)', opacity: .9 },
        { offset: 0.55, transform: 'translateX(-18px) rotate(19deg) scaleX(1)', opacity: 1 },
        { transform: 'translateX(72px) rotate(-35deg) scaleX(.22)', opacity: 0 },
      ],
      { duration: 410, easing: 'cubic-bezier(.2,.8,.2,1)', fill: 'forwards' },
    ),
  ]);
}

async function glass(surface: HTMLElement, cleanup: HTMLElement[]) {
  const bands = 8;
  const promises: Promise<void>[] = [];
  for (let index = 0; index < bands; index += 1) {
    const clone = surface.cloneNode(true) as HTMLElement;
    clone.removeAttribute('data-yummi-app-surface');
    clone.setAttribute('aria-hidden', 'true');
    const top = (index * 100) / bands;
    const bottom = 100 - ((index + 1) * 100) / bands;
    Object.assign(clone.style, {
      position: 'fixed',
      inset: '0',
      zIndex: '99990',
      width: '100vw',
      height: '100vh',
      pointerEvents: 'none',
      clipPath: `inset(${top}% 0 ${bottom}% 0)`,
      transformOrigin: '100% 100%',
      willChange: 'transform, opacity, filter',
    });
    document.body.appendChild(clone);
    cleanup.push(clone);
    const direction = index % 2 === 0 ? 1 : -1;
    promises.push(
      animate(
        clone,
        [
          { transform: 'translateX(0) skewX(0deg) scale(1)', opacity: 1, filter: 'blur(0px) saturate(1)' },
          { offset: 0.34, transform: `translateX(${direction * (4 + index)}px) skewX(${direction * 1.5}deg) scale(1.002)`, opacity: .94, filter: 'blur(.35px) saturate(1.12)' },
          { offset: 0.66, transform: `translate(${direction * (11 + index * 2)}px, ${index * 2}px) skewX(${direction * 4}deg) scale(.82)`, opacity: .68, filter: 'blur(1.1px) saturate(1.3)' },
          { transform: `translate(${20 + index * 3}vw, ${18 + index * 2}vh) skewX(${direction * 10}deg) scale(.08)`, opacity: 0, filter: 'blur(5px) saturate(1.45)' },
        ],
        { duration: 360 + index * 8, delay: index * 7, easing: 'cubic-bezier(.3,.72,.18,1)', fill: 'forwards' },
      ),
    );
  }
  surface.style.opacity = '0';
  await Promise.all(promises);
}

async function fade(surface: HTMLElement) {
  surface.style.transformOrigin = '100% 100%';
  await animate(
    surface,
    [
      { transform: 'translate(0,0) scale(1)', opacity: 1 },
      { transform: 'translate(7%, 7%) scale(.88)', opacity: 0 },
    ],
    { duration: 160, easing: 'ease-in', fill: 'forwards' },
  );
}

export async function playTrayHideEffect(
  requested: TrayHideEffect,
  onHidden?: () => Promise<unknown> | unknown,
): Promise<void> {
  if (running) return;
  const surface = document.querySelector<HTMLElement>(SURFACE_SELECTOR);
  if (!surface) {
    await onHidden?.();
    return;
  }

  running = true;
  const previous = snapshotInlineStyle(surface);
  const cleanup: HTMLElement[] = [];
  const reducedMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
  const effect: TrayHideEffect = reducedMotion && requested !== 'none' ? 'fade' : requested;

  try {
    switch (effect) {
      case 'fold':
        await fold(surface);
        break;
      case 'jelly':
        await jelly(surface);
        break;
      case 'pixels':
        await pixels(surface, cleanup);
        break;
      case 'cat':
        await cat(surface, cleanup);
        break;
      case 'glass':
        await glass(surface, cleanup);
        break;
      case 'fade':
        await fade(surface);
        break;
      case 'none':
        surface.style.opacity = '0';
        break;
    }

    if (onHidden) await onHidden();
    else await wait(90);
  } finally {
    cleanup.forEach((element) => element.remove());
    restoreInlineStyle(surface, previous);
    running = false;
  }
}
