// Small shared UI pieces in Sotto's visual language.

import { useMemo, useState } from "react";
import { Check, Search } from "lucide-react";
import type { Language } from "../types";

/** The mark: a decrescendo hairpin fading into a text cursor (same geometry
 *  as scripts/brand.py). The hairpin takes the current text colour. */
export function Mark({ width = 28 }: { width?: number }) {
  return (
    <svg
      width={width}
      height={(width * 320) / 560}
      viewBox="0 0 560 320"
      aria-hidden="true"
      className="shrink-0"
    >
      <path
        d="M29 92 L444 160 L29 228"
        fill="none"
        stroke="currentColor"
        strokeWidth="30"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <rect x="500" y="10" width="44" height="300" rx="22" fill="var(--signal)" />
    </svg>
  );
}

export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className="switch"
      onClick={() => onChange(!checked)}
    />
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="group">
      {options.map((o) => (
        <button key={o.value} aria-pressed={value === o.value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** 1–5 rating as five small ticks. */
export function Meter({ value, label }: { value: number; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5" title={`${label}: ${value}/5`}>
      <span className="label !text-[9px]">{label}</span>
      <span className="inline-flex gap-[2px]">
        {[1, 2, 3, 4, 5].map((i) => (
          <span
            key={i}
            className={`h-[9px] w-[3px] rounded-full ${i <= value ? "bg-ink" : "bg-ink-4"}`}
          />
        ))}
      </span>
    </span>
  );
}

/** The three things worth knowing about languages, shown wherever the
 *  language is picked. */
export function LanguageGuide({ compact = false }: { compact?: boolean }) {
  const items = [
    ["Auto-detect", "hears which language you speak, every time. Best if you switch between languages."],
    ["Pick yours", "for the fastest, most accurate result — about twice as fast as auto-detect."],
    ["Translate", "turns any language into English text. Needs a model that translates (not Turbo)."],
  ];
  return (
    <ol className={`space-y-1.5 ${compact ? "text-[11.5px]" : "text-[12.5px]"} leading-snug text-ink-2`}>
      {items.map(([title, body], i) => (
        <li key={title} className="flex gap-2">
          <span className="font-mono text-[10px] leading-[1.6] text-signal-text">0{i + 1}</span>
          <span>
            <span className="font-medium text-ink">{title}</span> {body}
          </span>
        </li>
      ))}
    </ol>
  );
}

/** Searchable list of every Whisper language, "Auto-detect" first. */
export function LanguageList({
  languages,
  value,
  onChange,
  className = "",
  autoFocus = false,
}: {
  languages: Language[];
  value: string;
  onChange: (code: string) => void;
  className?: string;
  autoFocus?: boolean;
}) {
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const all = [{ code: "auto", name: "Auto-detect" }, ...languages];
    return q
      ? all.filter((l) => l.name.toLowerCase().includes(q) || l.code.toLowerCase().startsWith(q))
      : all;
  }, [languages, query]);

  return (
    <div className={`flex min-h-0 flex-col ${className}`}>
      <label className="field mb-2 flex h-8 items-center gap-2 rounded-lg bg-fill px-2.5">
        <Search size={13} className="text-ink-3" />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={`Search ${languages.length} languages`}
          className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-ink-3"
          spellCheck={false}
          autoFocus={autoFocus}
        />
      </label>
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto pr-1" role="listbox">
        {shown.map((l) => (
          <button
            key={l.code}
            role="option"
            aria-selected={value === l.code}
            onClick={() => onChange(l.code)}
            className="panel-row !min-h-[28px] !py-1"
          >
            <span className="flex items-center gap-2">
              <span className="w-4">
                {value === l.code && <Check size={13} strokeWidth={2.4} className="text-signal-text" />}
              </span>
              {l.name}
            </span>
            {l.code !== "auto" && <span className="font-mono text-[10.5px] text-ink-3">{l.code}</span>}
          </button>
        ))}
        {shown.length === 0 && (
          <p className="px-2 py-4 text-center text-[12px] text-ink-3">No language matches “{query}”.</p>
        )}
      </div>
    </div>
  );
}
