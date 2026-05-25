/// <reference types="vite/client" />

declare module "*.swift?raw" {
  const content: string;
  export default content;
}

declare module "*.kt?raw" {
  const content: string;
  export default content;
}

declare module "*.rs?raw" {
  const content: string;
  export default content;
}
