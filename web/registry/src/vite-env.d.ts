/// <reference types="vite/client" />

declare module "*.swift?raw" {
  const content: string;
  export default content;
}
