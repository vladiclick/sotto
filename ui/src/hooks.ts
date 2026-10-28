// Shared state hooks. Each window (panel, settings, overlay) is its own
// webview, so state is always read from the backend and kept in sync through
// backend events — never cached across windows.

import { useCallback, useEffect, useRef, useState } from "react";
import {
  checkPermissions,
  fetchDictationState,
  fetchLanguages,
  fetchModels,
  fetchSettings,
  onBackendEvent,
  saveSettings,
} from "./bridge";
import type {
  DictationState,
  DownloadProgress,
  Language,
  ModelInfo,
  Permissions,
  Settings,
} from "./types";

/** Subscribe to a backend event for the component's lifetime. */
export function useBackendEvent<T>(name: string, handler: (payload: T) => void) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    const sub = onBackendEvent<T>(name, (p) => ref.current(p));
    return () => {
      sub.then((un) => un());
    };
  }, [name]);
}

/** Run `fn` whenever this window gains focus (and once on mount). */
export function useOnFocus(fn: () => void) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    ref.current();
    const onFocus = () => ref.current();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);
}

export function useDictationState(): DictationState {
  const [state, setState] = useState<DictationState>("loading");
  useBackendEvent<string>("dictation-state", (s) => setState(s as DictationState));
  useOnFocus(() => {
    fetchDictationState().then(setState).catch(console.error);
  });
  return state;
}

/**
 * Settings with optimistic updates. `update` applies a patch immediately,
 * saves it, and adopts what the backend actually stored (it validates).
 * Returns an error message if the save failed (the change is rolled back).
 */
export function useSettings(): [
  Settings | null,
  (patch: Partial<Settings>) => Promise<string | null>,
] {
  const [settings, setSettings] = useState<Settings | null>(null);
  const latest = useRef<Settings | null>(null);
  latest.current = settings;

  useOnFocus(() => {
    fetchSettings().then(setSettings).catch(console.error);
  });
  useBackendEvent<Settings>("settings-changed", setSettings);

  const update = useCallback(async (patch: Partial<Settings>) => {
    const before = latest.current;
    if (!before) return "settings are still loading";
    const next = { ...before, ...patch };
    setSettings(next);
    try {
      setSettings(await saveSettings(next));
      return null;
    } catch (e) {
      setSettings(before);
      return String(e);
    }
  }, []);

  return [settings, update];
}

export function useLanguages(): Language[] {
  const [langs, setLangs] = useState<Language[]>([]);
  useEffect(() => {
    fetchLanguages().then(setLangs).catch(console.error);
  }, []);
  return langs;
}

/** Model list plus live download progress, keyed by model id. */
export function useModels(): {
  models: ModelInfo[];
  progress: Record<string, DownloadProgress>;
  refresh: () => void;
} {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const refresh = useCallback(() => {
    fetchModels().then(setModels).catch(console.error);
  }, []);

  useOnFocus(refresh);
  useBackendEvent<unknown>("models-changed", refresh);
  useBackendEvent<unknown>("settings-changed", refresh);
  useBackendEvent<DownloadProgress>("model-download", (p) => {
    setProgress((prev) => {
      const next = { ...prev };
      if (p.phase === "downloading" || p.phase === "verifying") next[p.id] = p;
      else delete next[p.id];
      return next;
    });
    if (p.phase !== "downloading") refresh();
  });

  return { models, progress, refresh };
}

/** Permissions, re-checked on focus and every 1.5 s while focused. */
export function usePermissions(): [Permissions | null, () => void] {
  const [perms, setPerms] = useState<Permissions | null>(null);
  const refresh = useCallback(() => {
    checkPermissions().then(setPerms).catch(console.error);
  }, []);
  useOnFocus(refresh);
  useEffect(() => {
    const id = setInterval(() => {
      if (document.hasFocus()) refresh();
    }, 1500);
    return () => clearInterval(id);
  }, [refresh]);
  return [perms, refresh];
}

export function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  return `${Math.round(n / 1e6)} MB`;
}

export const HOTKEY_LABEL: Record<Settings["hotkey"], string> = {
  rightCommand: "right ⌘",
  rightOption: "right ⌥",
  rightControl: "right ⌃",
  fn: "fn",
};
