# Changelog

## 0.1.0 — first public release

- Hold a key (right ⌘, ⌥, ⌃ or fn) to dictate into any app; or tap to
  toggle. Shortcuts and quick taps never start a dictation; Esc cancels.
- whisper.cpp on Metal, fully on-device. Eight models from Tiny (32 MB) to
  Large v3 (1.08 GB), downloaded with resume and SHA-256 verification and
  switched without interrupting dictation.
- 100 languages: auto-detect, pinned, or translate to English; custom
  vocabulary.
- Type or paste insertion; paste restores your clipboard and is marked
  transient for clipboard managers.
- Menu-bar panel, Settings window, recording pill above the Dock, start/stop
  sounds, open at login.
- Filters Whisper's silence hallucinations ("Thank you.", "Thanks for
  watching!") and non-speech tags.
