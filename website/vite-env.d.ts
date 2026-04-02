/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_DOCS_URL: string;
  readonly VITE_MAILMODO_FORM_URL: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
