import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from 'react';

export function Card({
  title,
  action,
  children,
  className = '',
}: {
  title?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section
      className={`rounded-xl border border-white/8 bg-zinc-900/70 p-4 shadow-[0_4px_16px_rgb(0_0_0/0.25)] ${className}`}
    >
      {(title || action) && (
        <div className="mb-3 flex items-center justify-between gap-2">
          {title && (
            <h2 className="text-[13px] font-semibold tracking-wide text-zinc-200">{title}</h2>
          )}
          {action}
        </div>
      )}
      {children}
    </section>
  );
}

export function Dot({ on, className = '' }: { on: boolean; className?: string }) {
  return (
    <span
      className={`inline-block size-2 rounded-full ${
        on ? 'bg-emerald-400 shadow-[0_0_6px_rgb(52_211_153/0.8)]' : 'bg-zinc-600'
      } ${className}`}
    />
  );
}

export function Badge({
  tone = 'neutral',
  children,
}: {
  tone?: 'neutral' | 'ok' | 'warn' | 'accent';
  children: ReactNode;
}) {
  const tones = {
    neutral: 'bg-zinc-800 text-zinc-400 border-white/8',
    ok: 'bg-emerald-500/10 text-emerald-300 border-emerald-400/20',
    warn: 'bg-amber-500/10 text-amber-300 border-amber-400/20',
    accent: 'bg-indigo-500/10 text-indigo-300 border-indigo-400/20',
  } as const;
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] font-medium ${tones[tone]}`}
    >
      {children}
    </span>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  description,
  disabled = false,
}: {
  checked: boolean;
  onChange(next: boolean): void;
  label: string;
  description?: string;
  disabled?: boolean;
}) {
  return (
    <label
      className={`flex items-center justify-between gap-3 py-2 ${
        disabled ? 'opacity-50' : 'cursor-pointer'
      }`}
    >
      <span className="min-w-0">
        <span className="block text-[13px] text-zinc-200">{label}</span>
        {description && (
          <span className="mt-0.5 block text-[11px] leading-snug text-zinc-500">
            {description}
          </span>
        )}
      </span>
      <span className="relative inline-flex shrink-0">
        <input
          type="checkbox"
          className="peer sr-only"
          checked={checked}
          disabled={disabled}
          onChange={(event) => onChange(event.target.checked)}
        />
        <span className="h-5 w-9 rounded-full bg-zinc-700 transition-colors peer-checked:bg-indigo-500" />
        <span className="absolute top-0.5 left-0.5 size-4 rounded-full bg-white transition-transform peer-checked:translate-x-4" />
      </span>
    </label>
  );
}

export function Button({
  variant = 'default',
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'default' | 'primary' | 'danger' | 'ghost';
}) {
  const variants = {
    default:
      'border-white/10 bg-zinc-800 text-zinc-200 hover:bg-zinc-700 active:bg-zinc-600',
    primary:
      'border-indigo-400/30 bg-indigo-500 text-white hover:bg-indigo-400 active:bg-indigo-600',
    danger:
      'border-rose-400/20 bg-rose-500/10 text-rose-300 hover:bg-rose-500/20 active:bg-rose-500/30',
    ghost: 'border-transparent bg-transparent text-zinc-400 hover:text-zinc-200',
  } as const;
  return (
    <button
      type="button"
      className={`shrink-0 rounded-lg border px-3 py-1.5 text-[12px] font-medium whitespace-nowrap transition-colors disabled:pointer-events-none disabled:opacity-40 ${variants[variant]} ${className}`}
      {...props}
    />
  );
}

export function TextInput({
  className = '',
  ...props
}: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={`w-full rounded-lg border border-white/10 bg-zinc-950/60 px-3 py-2 text-[12px] text-zinc-200 placeholder:text-zinc-600 focus:border-indigo-400/50 focus:outline-none disabled:opacity-40 ${className}`}
      {...props}
    />
  );
}
