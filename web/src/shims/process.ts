declare global {
  interface Window {
    process?: {
      env?: Record<string, string>;
    };
  }

  interface GlobalThis {
    process?: {
      env?: Record<string, string>;
    };
  }
}

type BrowserProcess = {
  env?: Record<string, string>;
};

const globalWithProcess = globalThis as typeof globalThis & {
  process?: BrowserProcess;
};

const existingProcess =
  typeof globalWithProcess.process === 'object' && globalWithProcess.process !== null
    ? globalWithProcess.process
    : {};

const existingEnv =
  typeof existingProcess.env === 'object' && existingProcess.env !== null
    ? existingProcess.env
    : {};

globalWithProcess.process = {
  ...existingProcess,
  env: {
    ...existingEnv,
    NODE_ENV: import.meta.env.PROD ? 'production' : 'development',
  },
};

export {};
