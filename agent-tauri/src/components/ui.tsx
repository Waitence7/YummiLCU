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
      className={`rounded-xl border border-slate-200 bg-white p-4 shadow-[0_4px_16px_rgb(15_23_42/0.06)] ${className}`}
    >
      {(title || action) && (
        <div className="mb-3 flex items-center justify-between gap-2">
          {title && (
            <h2 className="text-[13px] font-semibold tracking-wide text-slate-800">{title}</h2>
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
        on ? 'bg-emerald-500 shadow-[0_0_6px_rgb(16_185_129/0.45)]' : 'bg-slate-300'
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
    neutral: 'bg-slate-100 text-slate-600 border-slate-200',
    ok: 'bg-emerald-50 text-emerald-700 border-emerald-200',
    warn: 'bg-amber-50 text-amber-700 border-amber-200',
    accent: 'bg-indigo-50 text-indigo-700 border-indigo-200',
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
        <span className="block text-[13px] text-slate-800">{label}</span>
        {description && (
          <span className="mt-0.5 block text-[11px] leading-snug text-slate-500">
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
        <span className="h-5 w-9 rounded-full bg-slate-300 transition-colors peer-checked:bg-indigo-500" />
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
      'border-slate-200 bg-white text-slate-700 hover:bg-slate-50 active:bg-slate-100',
    primary:
      'border-indigo-600/30 bg-indigo-600 text-white hover:bg-indigo-500 active:bg-indigo-700',
    danger:
      'border-rose-200 bg-rose-50 text-rose-700 hover:bg-rose-100 active:bg-rose-100',
    ghost: 'border-transparent bg-transparent text-slate-500 hover:text-slate-800',
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
      className={`w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-[12px] text-slate-800 placeholder:text-slate-400 focus:border-indigo-400/70 focus:outline-none disabled:opacity-40 ${className}`}
      {...props}
    />
  );
}
