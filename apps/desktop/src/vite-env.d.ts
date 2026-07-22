/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_OPAP_SOURCE_REVISION?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
