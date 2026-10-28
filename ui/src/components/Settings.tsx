// Settings window (window "settings", ui/settings.html). Sidebar + four
// tabs. Every control saves immediately through the backend, which
// validates and broadcasts "settings-changed" to the other windows.

import { useEffect, useRef, useState } from "react";
import {
  AudioLines,
  Check,
  Download,
  ExternalLink,
  FolderOpen,
  Info,
  Languages as LanguagesIcon,
  Loader2,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import {
  cancelDownload,
  deleteModel,
  isTauri,
  openLogs,
  openPrivacyPane,
  openProjectPage,
  requestMicrophone,
  type SettingsTab,
} from "../bridge";
import {
  formatBytes,
  useBackendEvent,
  useLanguages,
  useModels,
  usePermissions,
  useSettings,
} from "../hooks";
import { VOCABULARY_MAX, type Hotkey, type ModelInfo, type Settings as S } from "../types";
import { LanguageGuide, LanguageList, Mark, Meter, Segmented, Switch } from "./ui";

type Update = (patch: Partial<S>) => Promise<string | null>;

const TABS: { id: SettingsTab; label: string; icon: React.ElementType }[] = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "model", label: "Model", icon: AudioLines },
  { id: "language", label: "Language", icon: LanguagesIcon },
  { id: "about", label: "About", icon: Info },
];

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-7">
      <h3 className="label mb-2.5">{title}</h3>
      {children}
    </section>
  );
}

function Line({
  title,
  detail,
  children,
}: {
  title: string;
  detail?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-line py-3 last:border-0">
      <div className="min-w-0">
        <div className="text-[13px]">{title}</div>
        {detail && <div className="mt-0.5 text-[12px] leading-snug text-ink-3">{detail}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Card({ children }: { children: React.ReactNode }) {
  return <div className="rounded-xl border border-line bg-raised px-4">{children}</div>;
}

// ---- General ----

const KEYS: { value: Hotkey; cap: string; name: string }[] = [
  { value: "rightCommand", cap: "⌘", name: "Right Command" },
  { value: "rightOption", cap: "⌥", name: "Right Option" },
  { value: "rightControl", cap: "⌃", name: "Right Control" },
  { value: "fn", cap: "fn", name: "Fn / Globe" },
];

function General({ s, update }: { s: S; update: Update }) {
  const [perms, refreshPerms] = usePermissions();
  const [loginError, setLoginError] = useState<string | null>(null);

  return (
    <>
      <Section title="Dictation key">
        <div className="mb-3 grid grid-cols-4 gap-2">
          {KEYS.map((k) => (
            <button
              key={k.value}
              onClick={() => update({ hotkey: k.value })}
              aria-pressed={s.hotkey === k.value}
              className={`flex flex-col items-center gap-2 rounded-xl border px-2 py-3 transition-colors ${
                s.hotkey === k.value ? "border-signal bg-signal-soft" : "border-line bg-raised hover:bg-fill"
              }`}
            >
              <span className="keycap !h-8 !min-w-8 !text-[15px]">{k.cap}</span>
              <span className="text-[11.5px] text-ink-2">{k.name}</span>
            </button>
          ))}
        </div>
        {s.hotkey === "fn" && (
          <p className="mb-3 text-[12px] leading-snug text-ink-3">
            Set System Settings → Keyboard → “Press 🌐 key to” → <em>Do Nothing</em>, or macOS will open the emoji
            picker too.
          </p>
        )}
        <Card>
          <Line
            title="How it listens"
            detail={
              s.mode === "hold"
                ? "Hold the key while you talk; let go to type. A quick tap or a shortcut does nothing."
                : "Tap once to start, tap again to type. Hands-free for long thoughts."
            }
          >
            <Segmented
              value={s.mode}
              onChange={(mode) => update({ mode })}
              options={[
                { value: "hold", label: "Hold to talk" },
                { value: "toggle", label: "Tap to toggle" },
              ]}
            />
          </Line>
          <Line title="Cancel a recording" detail="Press Esc while it listens — nothing is typed.">
            <kbd className="keycap">esc</kbd>
          </Line>
        </Card>
      </Section>

      <Section title="Inserting text">
        <Card>
          <Line
            title="Method"
            detail={
              s.insert === "type"
                ? "Types the words like a keyboard. Never touches your clipboard."
                : "Pastes in one go — best for long text and stubborn apps. Your clipboard is put back right after."
            }
          >
            <Segmented
              value={s.insert}
              onChange={(insert) => update({ insert })}
              options={[
                { value: "type", label: "Type it" },
                { value: "paste", label: "Paste it" },
              ]}
            />
          </Line>
        </Card>
      </Section>

      <Section title="Feedback">
        <Card>
          <Line title="Sounds" detail="A soft tick when the mic opens and a pop when it stops.">
            <Switch label="Sounds" checked={s.sounds} onChange={(sounds) => update({ sounds })} />
          </Line>
          <Line title="Recording pill" detail="A small pill above the Dock while you speak.">
            <Switch label="Recording pill" checked={s.overlay} onChange={(overlay) => update({ overlay })} />
          </Line>
        </Card>
      </Section>

      <Section title="System">
        <Card>
          <Line title="Open at login" detail={loginError ?? "Start Sotto quietly in the menu bar when you log in."}>
            <Switch
              label="Open at login"
              checked={s.launchAtLogin}
              onChange={(launchAtLogin) => update({ launchAtLogin }).then(setLoginError)}
            />
          </Line>
        </Card>
      </Section>

      <Section title="Permissions">
        <Card>
          <Line title="Microphone" detail="Only while you dictate. Audio never leaves this Mac.">
            {perms?.microphone === "granted" ? (
              <span className="flex items-center gap-1 text-[12px] text-ok">
                <Check size={14} /> Allowed
              </span>
            ) : (
              <button
                className="btn btn-primary"
                onClick={() =>
                  perms?.microphone === "undetermined"
                    ? requestMicrophone().then(refreshPerms)
                    : openPrivacyPane("microphone")
                }
              >
                Allow
              </button>
            )}
          </Line>
          <Line title="Accessibility" detail="Lets Sotto notice the dictation key and type for you.">
            {perms?.accessibility ? (
              <span className="flex items-center gap-1 text-[12px] text-ok">
                <Check size={14} /> Allowed
              </span>
            ) : (
              <button className="btn btn-primary" onClick={() => openPrivacyPane("accessibility")}>
                Open Settings
              </button>
            )}
          </Line>
          <Line title="Dictation key" detail="Starts within two seconds of Accessibility being allowed.">
            <span className={`text-[12px] ${perms?.hotkey ? "text-ok" : "text-ink-3"}`}>
              {perms?.hotkey ? "Listening" : "Waiting"}
            </span>
          </Line>
        </Card>
      </Section>
    </>
  );
}

// ---- Model ----

function ModelCard({
  m,
  pct,
  onUse,
  onError,
}: {
  m: ModelInfo;
  pct?: number;
  onUse: () => void;
  onError: (e: string) => void;
}) {
  const busy = m.downloading || pct !== undefined;
  return (
    <div
      className={`rounded-xl border px-4 py-3.5 transition-colors ${
        m.selected ? "border-signal bg-signal-soft" : "border-line bg-raised"
      }`}
    >
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[14px] font-medium">{m.name}</span>
            {m.id === "large-v3-turbo" && <span className="label !text-signal-text">Default</span>}
            {m.englishOnly && <span className="label">English only</span>}
            {m.translates && <span className="label">Translates</span>}
          </div>
          <p className="mt-1 text-[12.5px] leading-snug text-ink-2">{m.summary}</p>
          <div className="mt-2 flex items-center gap-4">
            <Meter label="Speed" value={m.speed} />
            <Meter label="Accuracy" value={m.accuracy} />
            <span className="font-mono text-[11px] text-ink-3">{formatBytes(m.size)}</span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5 pt-0.5">
          {m.active ? (
            <span className="flex items-center gap-1 text-[12px] font-medium text-signal-text">
              <Check size={14} strokeWidth={2.4} /> In use
            </span>
          ) : busy ? (
            <button className="btn" onClick={() => cancelDownload(m.id)}>
              <X size={13} /> Cancel
            </button>
          ) : m.selected ? (
            <span className="flex items-center gap-1.5 text-[12px] text-ink-2">
              <Loader2 size={13} className="animate-spin" /> Loading
            </span>
          ) : (
            <>
              {m.downloaded && (
                <button
                  className="btn !px-2"
                  aria-label={`Delete ${m.name}`}
                  title="Delete from this Mac"
                  onClick={() => deleteModel(m.id).catch((e) => onError(String(e)))}
                >
                  <Trash2 size={13} />
                </button>
              )}
              <button className="btn btn-primary" onClick={onUse}>
                {m.downloaded ? (
                  "Use"
                ) : (
                  <>
                    <Download size={13} /> Use
                  </>
                )}
              </button>
            </>
          )}
        </div>
      </div>
      {busy && (
        <div className="mt-3 flex items-center gap-3">
          <div className="h-[3px] flex-1 overflow-hidden rounded-full bg-fill-2">
            <div className="h-full rounded-full bg-signal transition-all" style={{ width: `${pct ?? 0}%` }} />
          </div>
          <span className="w-10 text-right font-mono text-[11px] tabular-nums text-ink-3">
            {Math.round(pct ?? 0)}%
          </span>
        </div>
      )}
    </div>
  );
}

function Model({ update }: { update: Update }) {
  const { models, progress } = useModels();
  const [error, setError] = useState<string | null>(null);
  useBackendEvent<{ id: string; phase: string; message: string }>("model-download", (p) => {
    if (p.phase === "error") setError(p.message);
  });
  return (
    <>
      <p className="mb-4 text-[12.5px] leading-snug text-ink-2">
        Every model runs entirely on this Mac. Bigger models hear better and take longer; Turbo is the sweet spot.
        Switching keeps the current model working until the new one is ready.
      </p>
      {error && (
        <div className="mb-3 rounded-lg bg-danger-soft px-3 py-2 text-[12px]">
          {error}
        </div>
      )}
      <div className="space-y-2.5">
        {models.map((m) => (
          <ModelCard
            key={m.id}
            m={m}
            pct={progress[m.id]?.pct}
            onError={setError}
            onUse={() => {
              setError(null);
              update({ model: m.id }).then((e) => e && setError(e));
            }}
          />
        ))}
      </div>
      <p className="mt-4 text-[11.5px] leading-snug text-ink-3">
        Models come from huggingface.co/ggerganov/whisper.cpp and are checked against a pinned SHA-256 before use.
      </p>
    </>
  );
}

// ---- Language ----

function Language({ s, update }: { s: S; update: Update }) {
  const languages = useLanguages();
  const { models } = useModels();
  const active = models.find((m) => m.active);
  const canTranslate = active ? active.translates : true;
  const [vocab, setVocab] = useState(s.vocabulary);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // follow outside changes unless the user is mid-edit
  useEffect(() => {
    if (!timer.current) setVocab(s.vocabulary);
  }, [s.vocabulary]);

  const editVocab = (v: string) => {
    setVocab(v);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      timer.current = undefined;
      update({ vocabulary: v });
    }, 600);
  };

  return (
    <>
      <div className="mb-6 grid grid-cols-[1fr_230px] gap-6">
        <div className="flex h-[300px] flex-col rounded-xl border border-line bg-raised p-3">
          <LanguageList languages={languages} value={s.language} onChange={(language) => update({ language })} />
        </div>
        <div>
          <h3 className="label mb-2.5">How it works</h3>
          <LanguageGuide />
        </div>
      </div>

      <Section title="Translation">
        <Card>
          <Line
            title="Translate to English"
            detail={
              canTranslate
                ? "Speak any language, get English text."
                : `${active?.name} can’t translate. Pick Medium, Large v3, Small, Base or Tiny under Model.`
            }
          >
            <Switch
              label="Translate to English"
              checked={s.translate && canTranslate}
              disabled={!canTranslate}
              onChange={(translate) => update({ translate })}
            />
          </Line>
        </Card>
      </Section>

      <Section title="Your vocabulary">
        <textarea
          value={vocab}
          maxLength={VOCABULARY_MAX}
          onChange={(e) => editVocab(e.target.value)}
          onBlur={() => {
            if (timer.current) {
              clearTimeout(timer.current);
              timer.current = undefined;
              update({ vocabulary: vocab });
            }
          }}
          placeholder="Names, brands and jargon Sotto should spell your way — e.g. Andrei, Tauri, Kubernetes, MakerWorld."
          className="h-24 w-full resize-none rounded-xl border border-line bg-raised px-3 py-2.5 text-[12.5px] leading-snug placeholder:text-ink-3 focus:border-signal"
          spellCheck={false}
        />
        <div className="mt-1 flex justify-between text-[11px] text-ink-3">
          <span>Used as a hint, not a filter — write it like a normal sentence or a list.</span>
          <span className="font-mono tabular-nums">
            {vocab.length}/{VOCABULARY_MAX}
          </span>
        </div>
      </Section>
    </>
  );
}

// ---- About ----

function About() {
  const [version, setVersion] = useState("");
  useEffect(() => {
    if (!isTauri) return;
    import("@tauri-apps/api/app").then((m) => m.getVersion()).then(setVersion).catch(console.error);
  }, []);
  return (
    <div className="flex flex-col items-start">
      <img src="/sotto-icon.png" alt="" className="mb-4 h-20 w-20" draggable={false} />
      <div className="wordmark text-[44px]">sotto</div>
      <p className="mt-1 font-mono text-[11px] tracking-[0.12em] text-ink-3">
        {version ? `VERSION ${version}` : " "}
      </p>
      <p className="mt-4 max-w-[440px] text-[13px] leading-relaxed text-ink-2">
        <em>Sotto voce</em> — under the breath. Hold a key, speak, and Sotto writes it wherever your cursor is.
        Speech recognition runs on this Mac’s GPU; nothing you say is uploaded, logged or kept.
      </p>
      <div className="mt-5 flex gap-2">
        <button className="btn btn-primary" onClick={() => openProjectPage()}>
          <ExternalLink size={13} /> GitHub
        </button>
        <button className="btn" onClick={() => openLogs()}>
          <FolderOpen size={13} /> Logs
        </button>
      </div>
      <div className="mt-8 w-full border-t border-line pt-4 text-[11.5px] leading-relaxed text-ink-3">
        Built on whisper.cpp (MIT) and OpenAI Whisper models (MIT), with Tauri. Type: Instrument Serif and IBM Plex
        Mono (SIL Open Font License). Sotto is MIT-licensed.
      </div>
    </div>
  );
}

// ---- window ----

const TITLES: Record<SettingsTab, [string, string]> = {
  general: ["General", "How you start, stop and insert."],
  model: ["Model", "The speech engine on your Mac."],
  language: ["Language", "What you speak and how it’s written."],
  about: ["About", ""],
};

export default function Settings() {
  const [settings, update] = useSettings();
  const [tab, setTab] = useState<SettingsTab>(() => {
    // browser preview: ?tab=model opens a tab directly
    const t = isTauri ? null : new URLSearchParams(location.search).get("tab");
    return TABS.some((x) => x.id === t) ? (t as SettingsTab) : "general";
  });
  useBackendEvent<string>("settings-tab", (t) => {
    if (TABS.some((x) => x.id === t)) setTab(t as SettingsTab);
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && e.key.toLowerCase() === "w") {
        e.preventDefault();
        if (isTauri) import("@tauri-apps/api/window").then((m) => m.getCurrentWindow().hide());
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full">
      <aside className="flex w-[200px] shrink-0 flex-col border-r border-line bg-paper-2 px-3 pb-4">
        <div data-tauri-drag-region className="h-[52px] shrink-0" />
        <div className="mb-6 flex items-center gap-2 px-2 text-ink">
          <Mark width={30} />
          <span className="wordmark text-[28px]">sotto</span>
        </div>
        <nav className="space-y-0.5">
          {TABS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => setTab(id)}
              aria-current={tab === id ? "page" : undefined}
              className={`flex w-full items-center gap-2.5 rounded-lg px-2.5 py-[7px] text-left text-[13px] transition-colors ${
                tab === id ? "bg-fill-2 text-ink" : "text-ink-2 hover:bg-fill"
              }`}
            >
              <Icon size={15} strokeWidth={1.8} className={tab === id ? "text-signal-text" : ""} />
              {label}
            </button>
          ))}
        </nav>
        <div className="flex-1" />
        <p className="px-2 font-mono text-[10px] leading-relaxed tracking-[0.08em] text-ink-3">
          ON-DEVICE · PRIVATE
          <br />
          WHISPER ON METAL
        </p>
      </aside>

      <main className="scroll-thin min-w-0 flex-1 overflow-y-auto">
        <div data-tauri-drag-region className="sticky top-0 z-10 h-[38px] bg-paper/90 backdrop-blur" />
        <div className="px-9 pb-10">
          {tab !== "about" && (
            <header className="mb-6">
              <h1 className="wordmark text-[34px]">{TITLES[tab][0]}</h1>
              <p className="mt-1 text-[12.5px] text-ink-3">{TITLES[tab][1]}</p>
            </header>
          )}
          {!settings ? null : tab === "general" ? (
            <General s={settings} update={update} />
          ) : tab === "model" ? (
            <Model update={update} />
          ) : tab === "language" ? (
            <Language s={settings} update={update} />
          ) : (
            <About />
          )}
        </div>
      </main>
    </div>
  );
}
