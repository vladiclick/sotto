// Menu-bar drop-down panel (window "panel", ui/panel.html), drawn over the
// native popover vibrancy. Two views: home, and the language page (picker +
// a short guide to how languages work).

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Loader2,
  Power,
  Settings2,
  ShieldAlert,
  Square,
  TriangleAlert,
} from "lucide-react";
import {
  fetchDashboard,
  fetchLastError,
  isTauri,
  openPrivacyPane,
  panelCommand,
  requestMicrophone,
  showSettings,
} from "../bridge";
import {
  HOTKEY_LABEL,
  useBackendEvent,
  useDictationState,
  useLanguages,
  useModels,
  useOnFocus,
  usePermissions,
  useSettings,
} from "../hooks";
import type { DictationState } from "../types";
import { LanguageGuide, LanguageList, Mark, Switch } from "./ui";

const STATUS: Record<DictationState, { text: string; dot: string }> = {
  loading: { text: "Loading", dot: "bg-ink-3" },
  idle: { text: "Ready", dot: "bg-ok" },
  recording: { text: "Listening", dot: "bg-signal animate-pulse" },
  processing: { text: "Writing", dot: "bg-signal" },
  failed: { text: "Off", dot: "bg-ink-3" },
};

const PANEL_WIDTH = 320;
const LANGUAGE_PAGE_HEIGHT = 468;

function localISO(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function Row({
  label,
  value,
  onClick,
}: {
  label: string;
  value: React.ReactNode;
  onClick?: () => void;
}) {
  const body = (
    <>
      <span className="text-ink-2">{label}</span>
      <span className="flex min-w-0 items-center gap-1 truncate text-right">
        {value}
        {onClick && <ChevronRight size={13} className="shrink-0 text-ink-3" />}
      </span>
    </>
  );
  return onClick ? (
    <button className="panel-row" onClick={onClick}>
      {body}
    </button>
  ) : (
    <div className="panel-row hover:!bg-transparent">{body}</div>
  );
}

export default function Panel() {
  const state = useDictationState();
  const [settings, update] = useSettings();
  const languages = useLanguages();
  const { models, progress } = useModels();
  const [perms, refreshPerms] = usePermissions();
  const [view, setView] = useState<"home" | "languages">(() =>
    // browser preview: ?view=languages opens the language page directly
    !isTauri && new URLSearchParams(location.search).get("view") === "languages" ? "languages" : "home",
  );
  const [todayWords, setTodayWords] = useState(0);
  const [streak, setStreak] = useState(0);
  const [last, setLast] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // the window hugs its content (no dead space, nothing clipped); the
  // language page has a fixed height because its list scrolls
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!isTauri || !el) return;
    const apply = () => {
      const height = view === "languages" ? LANGUAGE_PAGE_HEIGHT : Math.ceil(el.scrollHeight);
      import("@tauri-apps/api/window")
        .then(({ getCurrentWindow, LogicalSize }) =>
          getCurrentWindow().setSize(new LogicalSize(PANEL_WIDTH, height)),
        )
        .catch(console.error);
    };
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(el);
    return () => ro.disconnect();
  }, [view]);

  const refreshStats = useCallback(() => {
    fetchDashboard()
      .then((s) => {
        const today = localISO(new Date());
        setTodayWords(s.days.find((d) => d.date === today)?.words ?? 0);
        setStreak(s.currentStreak);
      })
      .catch(console.error);
  }, []);

  useOnFocus(() => {
    refreshStats();
    fetchLastError().then(setError).catch(console.error);
  });
  useBackendEvent<unknown>("stats-updated", refreshStats);
  useBackendEvent<{ text: string }>("transcribed", (e) => {
    setLast(e.text);
    setError(null);
  });
  useBackendEvent<string>("dictation-error", setError);
  // a hidden panel always reopens on home
  useBackendEvent<unknown>("tauri://blur", () => setView("home"));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (view !== "home") setView("home");
        else panelCommand("hide_panel");
      } else if (e.metaKey && e.key === ",") {
        e.preventDefault();
        showSettings();
      } else if (e.metaKey && e.key.toLowerCase() === "q") {
        e.preventDefault();
        panelCommand("quit_app");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view]);

  const selected = models.find((m) => m.selected);
  const active = models.find((m) => m.active);
  const key = settings ? HOTKEY_LABEL[settings.hotkey] : "right ⌘";
  const toggleMode = settings?.mode === "toggle";
  const activeDownload = selected ? progress[selected.id] : undefined;
  const languageName =
    settings?.language === "auto"
      ? "Auto-detect"
      : (languages.find((l) => l.code === settings?.language)?.name ?? settings?.language);

  if (view === "languages" && settings) {
    const canTranslate = active ? active.translates : true;
    return (
      <div ref={rootRef} className="flex h-full flex-col px-3 pb-3 pt-3">
        <div className="mb-3 flex items-center gap-1">
          <button
            onClick={() => setView("home")}
            className="-ml-1 flex h-7 w-7 items-center justify-center rounded-md hover:bg-fill"
            aria-label="Back"
          >
            <ChevronLeft size={16} />
          </button>
          <h2 className="wordmark text-[22px]">Languages</h2>
        </div>
        <div className="mb-3 rounded-xl bg-fill px-3 py-2.5">
          <LanguageGuide compact />
        </div>
        <div className="mb-2 flex items-center justify-between px-1">
          <span>
            Translate to English
            {!canTranslate && <span className="block text-[11px] text-ink-3">{active?.name} can’t translate</span>}
          </span>
          <Switch
            label="Translate to English"
            checked={settings.translate && canTranslate}
            disabled={!canTranslate}
            onChange={(v) => update({ translate: v })}
          />
        </div>
        <LanguageList
          className="flex-1"
          languages={languages}
          value={settings.language}
          autoFocus
          onChange={(code) => update({ language: code }).then(() => setView("home"))}
        />
      </div>
    );
  }

  const canToggle = state === "idle" || state === "recording";
  const hint =
    state === "recording" ? (
      toggleMode ? (
        <>Listening — tap <kbd className="keycap">{key}</kbd> to finish</>
      ) : (
        <>Listening — release to type</>
      )
    ) : state === "processing" ? (
      "Writing it down on this Mac…"
    ) : state === "loading" ? (
      activeDownload ? (
        `Downloading ${selected?.name} · ${Math.round(activeDownload.pct)}%`
      ) : (
        "Loading the voice model…"
      )
    ) : state === "failed" ? (
      "Dictation is off — see below"
    ) : (
      <>
        {toggleMode ? "Tap" : "Hold"} <kbd className="keycap">{key}</kbd> and speak
      </>
    );

  const needsPerms = perms && (perms.microphone !== "granted" || !perms.accessibility);

  return (
    <div ref={rootRef} className="flex flex-col px-3 pb-2 pt-3">
      {/* header */}
      <div className="flex items-center justify-between px-1">
        <span className="flex items-center gap-2 text-ink">
          <Mark width={26} />
          <span className="wordmark text-[24px]">sotto</span>
        </span>
        <span className="flex items-center gap-1.5 rounded-full bg-fill px-2 py-[3px]">
          <span className={`h-1.5 w-1.5 rounded-full ${STATUS[state].dot}`} />
          <span className="label !text-ink-2">{STATUS[state].text}</span>
        </span>
      </div>

      {/* record */}
      <div className="flex flex-col items-center pb-3.5 pt-4">
        <button
          className={`record ${state === "recording" ? "is-live" : ""}`}
          disabled={!canToggle}
          onClick={() => panelCommand("panel_dictate")}
          aria-label={state === "recording" ? "Stop and type" : "Start dictating"}
        >
          {state === "recording" ? (
            <Square size={20} fill="currentColor" strokeWidth={0} />
          ) : state === "processing" || state === "loading" ? (
            <Loader2 size={24} className="animate-spin text-paper" />
          ) : (
            <span className="block h-[26px] w-[4px] rounded-full bg-signal" />
          )}
        </button>
        <span className="mt-3 flex items-center gap-1.5 text-[12px] text-ink-2">{hint}</span>
        {activeDownload && (
          <div className="mt-2 h-[3px] w-44 overflow-hidden rounded-full bg-fill-2">
            <div className="h-full rounded-full bg-signal transition-all" style={{ width: `${activeDownload.pct}%` }} />
          </div>
        )}
      </div>

      {/* permissions */}
      {needsPerms && (
        <div className="mx-0.5 mb-2.5 rounded-xl bg-warn-soft px-3 py-2.5 text-[12px]">
          <div className="mb-1 flex items-center gap-1.5 font-medium">
            <ShieldAlert size={14} className="text-signal-text" />
            Two switches and you’re set
          </div>
          <p className="mb-1.5 text-ink-2">Turn on Sotto in System Settings, then come back — this updates by itself.</p>
          {perms.microphone !== "granted" && (
            <button
              className="panel-row !min-h-[24px] !px-0 !py-0.5"
              onClick={() =>
                perms.microphone === "undetermined"
                  ? requestMicrophone().then(refreshPerms)
                  : openPrivacyPane("microphone")
              }
            >
              <span>Microphone</span>
              <span className="text-signal-text">Allow →</span>
            </button>
          )}
          {!perms.accessibility && (
            <button className="panel-row !min-h-[24px] !px-0 !py-0.5" onClick={() => openPrivacyPane("accessibility")}>
              <span>Accessibility · for the key & typing</span>
              <span className="text-signal-text">Allow →</span>
            </button>
          )}
        </div>
      )}

      {/* error */}
      {error && (
        <div className="mx-0.5 mb-2.5 flex items-start gap-2 rounded-xl bg-danger-soft px-3 py-2 text-[12px] leading-snug">
          <TriangleAlert size={14} className="mt-px shrink-0 text-signal-text" />
          <span className="line-clamp-3">{error}</span>
        </div>
      )}

      {/* last dictation */}
      <div className="mx-0.5 mb-2 rounded-xl bg-fill px-3 py-2">
        <div className="label mb-0.5">Last</div>
        <p className="line-clamp-2 min-h-[34px] text-[12.5px] leading-snug">
          {last ?? <span className="text-ink-3">What you say shows up here and wherever your cursor is.</span>}
        </p>
      </div>

      {/* rows */}
      <Row label="Language" value={languageName ?? "…"} onClick={() => setView("languages")} />
      <Row
        label="Model"
        value={
          selected && selected.id !== active?.id && progress[selected.id]
            ? `${selected.name} · ${Math.round(progress[selected.id].pct)}%`
            : (active?.name ?? selected?.name ?? "…")
        }
        onClick={() => showSettings("model")}
      />
      <Row
        label="Today"
        value={
          <span className="tabular-nums">
            {todayWords.toLocaleString()} words
            <span className="text-ink-3"> · {streak}-day streak</span>
          </span>
        }
      />

      <div className="mx-2 my-1.5 h-px bg-line" />
      <button className="panel-row" onClick={() => showSettings()}>
        <span className="flex items-center gap-2.5">
          <Settings2 size={14} strokeWidth={1.8} />
          Settings…
        </span>
        <span className="font-mono text-[11px] text-ink-3">⌘,</span>
      </button>
      <button className="panel-row" onClick={() => panelCommand("quit_app")}>
        <span className="flex items-center gap-2.5">
          <Power size={14} strokeWidth={1.8} />
          Quit Sotto
        </span>
        <span className="font-mono text-[11px] text-ink-3">⌘Q</span>
      </button>
    </div>
  );
}
