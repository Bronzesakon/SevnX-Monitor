/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly TAURI_DEV_HOST?: string
  readonly TAURI_ENV_DEBUG?: string
  readonly TAURI_ENV_PLATFORM?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
