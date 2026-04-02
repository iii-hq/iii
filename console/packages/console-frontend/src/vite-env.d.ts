/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_III_ENGINE_HOST: string
  readonly VITE_III_ENGINE_PORT: string
  readonly VITE_III_WS_PORT: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
