// Shapes served by the Rust backend (src-tauri/src/commands.rs).

export interface DayActivity {
  date: string; // YYYY-MM-DD (local)
  words: number;
}

/** The subset of stats.rs `DashboardStats` the UI reads. */
export interface DashboardStats {
  currentStreak: number;
  days: DayActivity[];
}

export type DictationState =
  | "loading"
  | "idle"
  | "recording"
  | "processing"
  | "failed";

export type Hotkey = "rightCommand" | "rightOption" | "rightControl" | "fn";
export type Mode = "hold" | "toggle";
export type Insert = "type" | "paste";

/** settings.rs `Settings` */
export interface Settings {
  model: string;
  language: string; // "auto" or a Whisper code
  translate: boolean;
  vocabulary: string;
  hotkey: Hotkey;
  mode: Mode;
  insert: Insert;
  sounds: boolean;
  overlay: boolean;
  launchAtLogin: boolean;
}

export interface Language {
  code: string;
  name: string;
}

/** commands.rs `ModelInfo` (models.rs `ModelSpec` + status) */
export interface ModelInfo {
  id: string;
  file: string;
  name: string;
  summary: string;
  size: number;
  speed: number; // 1–5
  accuracy: number; // 1–5
  englishOnly: boolean;
  translates: boolean;
  downloaded: boolean;
  downloading: boolean;
  active: boolean;
  selected: boolean;
}

/** download.rs "model-download" event */
export interface DownloadProgress {
  id: string;
  phase: "downloading" | "verifying" | "done" | "error" | "cancelled";
  received: number;
  total: number;
  pct: number;
  message: string;
}

export interface Permissions {
  microphone: "granted" | "denied" | "undetermined";
  accessibility: boolean;
  /** The dictation-key listener is running. */
  hotkey: boolean;
}

export const VOCABULARY_MAX = 600; // settings.rs MAX_VOCABULARY_CHARS
