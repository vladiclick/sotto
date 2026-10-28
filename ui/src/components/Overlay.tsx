// Recording pill shown above the Dock while dictating (window "overlay",
// src-tauri/src/overlay.rs). The blinking vermilion bar is the logo's
// cursor; the bars follow the live mic level ("mic-level", ~25 Hz RMS) and
// are animated per frame straight on the DOM — no React re-render at 60 fps.
// While transcribing they turn into a soft travelling wave.

import { useEffect, useRef, useState } from "react";
import { isTauri, onBackendEvent } from "../bridge";
import type { DictationState } from "../types";

type Mode = "hidden" | "recording" | "processing";

const BARS = 13;
const MIN_H = 3; // px: resting dot
const MAX_H = 22; // px: loudest bar
const MID = (BARS - 1) / 2;

/** Center-weighted envelope so the shape reads as a voice, not a meter. */
const ENVELOPE = Array.from({ length: BARS }, (_, i) => {
  const d = Math.abs(i - MID) / MID;
  return 0.3 + 0.7 * Math.pow(1 - d, 1.3);
});
const SPEED = Array.from({ length: BARS }, (_, i) => 5 + ((i * 7) % 5));
const PHASE = Array.from({ length: BARS }, (_, i) => i * 1.7);

/** RMS → 0..1 on a dB scale (-55 dB silent … -15 dB loud). */
function loudness(rms: number): number {
  const db = 20 * Math.log10(Math.max(rms, 1e-5));
  return Math.min(1, Math.max(0, (db + 55) / 40));
}

function toMode(s: DictationState): Mode {
  return s === "recording" ? "recording" : s === "processing" ? "processing" : "hidden";
}

export default function Overlay() {
  const [mode, setMode] = useState<Mode>("hidden");
  const modeRef = useRef<Mode>("hidden");
  const target = useRef(0);
  const bars = useRef<(HTMLSpanElement | null)[]>([]);
  const heights = useRef<number[]>(Array(BARS).fill(MIN_H));

  // browser preview: pretend to record, with a synthetic voice level
  useEffect(() => {
    if (isTauri) return;
    modeRef.current = "recording";
    setMode("recording");
    const id = setInterval(() => {
      const t = performance.now() / 1000;
      target.current = Math.max(0, Math.sin(t * 3.1) * Math.sin(t * 1.3)) * 0.9 + 0.1;
    }, 40);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    const subs = [
      onBackendEvent<string>("dictation-state", (s) => {
        const m = toMode(s as DictationState);
        modeRef.current = m;
        if (m !== "recording") target.current = 0;
        setMode(m);
      }),
      onBackendEvent<number>("mic-level", (rms) => {
        target.current = loudness(rms);
      }),
    ];

    let raf = 0;
    let smooth = 0;
    const tick = (t: number) => {
      const sec = t / 1000;
      const m = modeRef.current;
      // fast attack, slow release: syllables pop, pauses settle gently
      const k = target.current > smooth ? 0.5 : 0.12;
      smooth += (target.current - smooth) * k;

      for (let i = 0; i < BARS; i++) {
        let h: number;
        if (m === "processing") {
          const wave = (Math.sin(sec * 6 - i * 0.55) + 1) / 2;
          h = MIN_H + wave * 5;
        } else {
          const wobble = 0.72 + 0.28 * Math.sin(sec * SPEED[i] + PHASE[i]);
          const breathe = 0.6 + 0.4 * Math.sin(sec * 2.2 + i * 0.4); // idle life
          const v = Math.max(smooth * wobble, 0.04 * breathe) * ENVELOPE[i];
          h = MIN_H + (MAX_H - MIN_H) * v;
        }
        heights.current[i] += (h - heights.current[i]) * 0.35;
        const el = bars.current[i];
        if (el) el.style.height = `${heights.current[i].toFixed(1)}px`;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);

    return () => {
      cancelAnimationFrame(raf);
      subs.forEach((p) => p.then((un) => un()));
    };
  }, []);

  return (
    <div className="flex h-full items-center justify-center">
      <div className={`sotto-pill ${mode === "hidden" ? "is-hidden" : ""}`}>
        <span className={`sotto-cursor ${mode === "recording" ? "" : "is-off"}`} />
        <div className={`flex items-center gap-[3px] ${mode === "processing" ? "opacity-60" : ""}`}>
          {Array.from({ length: BARS }, (_, i) => (
            <span
              key={i}
              ref={(el) => {
                bars.current[i] = el;
              }}
              className="sotto-bar"
            />
          ))}
        </div>
      </div>
    </div>
  );
}
