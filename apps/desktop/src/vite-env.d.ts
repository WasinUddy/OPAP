/// <reference types="vite/client" />

declare module 'virtual:opap-copying' {
  const copying: string;
  export default copying;
}

interface ImportMetaEnv {
  readonly VITE_OPAP_SOURCE_REVISION?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
