// Typed bridge to the Tauri backend (src-tauri/src/commands.rs). Outside the
// app (plain `npm run dev`) every call returns neutral preview data so the
// UI can be designed in a browser.

import type {
  DashboardStats,
  DictationState,
  Language,
  ModelInfo,
  Permissions,
  Settings,
} from "./types";

export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Subscribe to a backend event; no-op outside Tauri. Returns unlisten. */
export async function onBackendEvent<T>(
  name: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(name, (e) => handler(e.payload));
}

// ---- preview data (browser only) ----

const PREVIEW_SETTINGS: Settings = {
  model: "large-v3-turbo",
  language: "auto",
  translate: false,
  vocabulary: "",
  hotkey: "rightCommand",
  mode: "hold",
  insert: "type",
  sounds: true,
  overlay: true,
  launchAtLogin: false,
};

// ---- dictation ----

const todayISO = (() => {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
})();

export const fetchDashboard = (): Promise<DashboardStats> =>
  isTauri
    ? call("get_dashboard")
    : Promise.resolve({ currentStreak: 12, days: [{ date: todayISO, words: 1284 }] });

export const fetchDictationState = (): Promise<DictationState> =>
  isTauri ? call("get_dictation_state") : Promise.resolve("idle");

export const fetchLastError = (): Promise<string | null> =>
  isTauri ? call("get_last_error") : Promise.resolve(null);

// ---- settings ----

export const fetchSettings = (): Promise<Settings> =>
  isTauri ? call("get_settings") : Promise.resolve(PREVIEW_SETTINGS);

/** Save a full settings object; resolves to what was actually stored. */
export const saveSettings = (settings: Settings): Promise<Settings> =>
  isTauri ? call("set_settings", { settings }) : Promise.resolve(settings);

// whisper.cpp's language table (the real list comes from the engine)
const PREVIEW_LANGUAGES = "en english,zh chinese,de german,es spanish,ru russian,ko korean,fr french,ja japanese,pt portuguese,tr turkish,pl polish,ca catalan,nl dutch,ar arabic,sv swedish,it italian,id indonesian,hi hindi,fi finnish,vi vietnamese,he hebrew,uk ukrainian,el greek,ms malay,cs czech,ro romanian,da danish,hu hungarian,ta tamil,no norwegian,th thai,ur urdu,hr croatian,bg bulgarian,lt lithuanian,la latin,mi maori,ml malayalam,cy welsh,sk slovak,te telugu,fa persian,lv latvian,bn bengali,sr serbian,az azerbaijani,sl slovenian,kn kannada,et estonian,mk macedonian,br breton,eu basque,is icelandic,hy armenian,ne nepali,mn mongolian,bs bosnian,kk kazakh,sq albanian,sw swahili,gl galician,mr marathi,pa punjabi,si sinhala,km khmer,sn shona,yo yoruba,so somali,af afrikaans,oc occitan,ka georgian,be belarusian,tg tajik,sd sindhi,gu gujarati,am amharic,yi yiddish,lo lao,uz uzbek,fo faroese,ht haitian creole,ps pashto,tk turkmen,nn nynorsk,mt maltese,sa sanskrit,lb luxembourgish,my myanmar,bo tibetan,tl tagalog,mg malagasy,as assamese,tt tatar,haw hawaiian,ln lingala,ha hausa,ba bashkir,jw javanese,su sundanese,yue cantonese"
  .split(",")
  .map((pair) => {
    const [code, ...name] = pair.split(" ");
    const full = name.join(" ");
    return { code, name: full.replace(/\b\w/g, (c) => c.toUpperCase()) };
  })
  .sort((a, b) => a.name.localeCompare(b.name));

export const fetchLanguages = (): Promise<Language[]> =>
  isTauri ? call("get_languages") : Promise.resolve(PREVIEW_LANGUAGES);

// ---- models ----

const preview = (
  id: string,
  name: string,
  summary: string,
  size: number,
  speed: number,
  accuracy: number,
  extra: Partial<ModelInfo> = {},
): ModelInfo => ({
  id,
  file: `ggml-${id}.bin`,
  name,
  summary,
  size,
  speed,
  accuracy,
  englishOnly: false,
  translates: true,
  downloaded: false,
  downloading: false,
  active: false,
  selected: false,
  ...extra,
});

const PREVIEW_MODELS: ModelInfo[] = [
  preview("tiny", "Tiny", "Instant and tiny. Fine for short notes in clear English.", 32_152_673, 5, 1),
  preview("small", "Small", "Good everyday accuracy on older Macs.", 190_085_487, 4, 3, { downloaded: true }),
  preview("medium", "Medium", "Strong accuracy with translation; slower than Turbo.", 539_212_467, 2, 4),
  preview(
    "large-v3-turbo",
    "Large v3 Turbo",
    "The default: near-Large accuracy at a fraction of the time.",
    574_041_195,
    3,
    5,
    { translates: false, downloaded: true, active: true, selected: true },
  ),
  preview("large-v3", "Large v3", "The most accurate model, with translation. Slowest.", 1_081_140_203, 1, 5),
];

export const fetchModels = (): Promise<ModelInfo[]> =>
  isTauri ? call("list_models") : Promise.resolve(PREVIEW_MODELS);

export const downloadModel = (id: string): Promise<void> =>
  isTauri ? call("download_model", { id }) : Promise.resolve();

export const cancelDownload = (id: string): Promise<void> =>
  isTauri ? call("cancel_download", { id }) : Promise.resolve();

export const deleteModel = (id: string): Promise<void> =>
  isTauri ? call("delete_model", { id }) : Promise.resolve();

// ---- permissions ----

export const checkPermissions = (): Promise<Permissions> =>
  isTauri
    ? call("check_permissions")
    : Promise.resolve({ microphone: "granted", accessibility: true, hotkey: true });

export const requestMicrophone = (): Promise<void> =>
  isTauri ? call("request_microphone") : Promise.resolve();

export const openPrivacyPane = (pane: "microphone" | "accessibility"): Promise<void> =>
  isTauri ? call("open_privacy_pane", { pane }) : Promise.resolve();

// ---- windows & misc ----

export type SettingsTab = "general" | "model" | "language" | "about";

export const showSettings = (tab?: SettingsTab): Promise<void> =>
  isTauri ? call("show_settings", { tab: tab ?? null }) : Promise.resolve();

export const panelCommand = (cmd: "hide_panel" | "panel_dictate" | "quit_app"): Promise<void> =>
  isTauri ? call(cmd) : Promise.resolve();

export const openLogs = (): Promise<void> =>
  isTauri ? call("open_logs") : Promise.resolve();

export const openProjectPage = (): Promise<void> =>
  isTauri ? call("open_project_page") : Promise.resolve();
