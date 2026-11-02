// src/services/envService.ts

class EnvService {
  /**
   * Returns the current application mode (development or production).
   */
  get mode(): string {
    return import.meta.env.MODE;
  }

  /**
   * Detects if the application is running in development mode.
   */
  get isDev(): boolean {
    return import.meta.env.MODE === 'development';
  }

  /**
   * Base URL of the extension store API. Empty (the default) means no store is
   * configured: the Store lists nothing and no extension update checks run.
   * Set VITE_LAUNCHER_STORE_URL at build time to point at your own store.
   */
  get storeApiBaseUrl(): string {
    return (import.meta.env.VITE_LAUNCHER_STORE_URL as string | undefined) ?? '';
  }

  get hasStore(): boolean {
    return this.storeApiBaseUrl !== '';
  }

  /**
   * The semver version of the Launcher SDK bundled and supported by this host.
   */
  get supportedSdkVersion(): string {
    return SUPPORTED_SDK_VERSION;
  }
}

export const SUPPORTED_SDK_VERSION = '4.15.0';
export const envService = new EnvService();
