import { describe, it, expect } from 'vitest';
import { isExtensionInstalled, isEmojiInstalled } from './installedExtensions';

describe('isExtensionInstalled', () => {
  it('returns false for null/empty extension or paths', () => {
    expect(isExtensionInstalled(null, ['/path/to/org.launcher.browser'])).toBe(false);
    expect(isExtensionInstalled(undefined, ['/path/to/org.launcher.browser'])).toBe(false);
    expect(isExtensionInstalled({ id: 'org.launcher.browser' }, null)).toBe(false);
    expect(isExtensionInstalled({ id: 'org.launcher.browser' }, [])).toBe(false);
    expect(isExtensionInstalled({}, ['/path/to/org.launcher.browser'])).toBe(false);
  });

  it('matches exact manifest ID in flat array (e.g. test mock)', () => {
    const ext = {
      id: 101,
      slug: 'github-assistant',
      manifest: { id: 'org.example.github' },
    };
    expect(isExtensionInstalled(ext, ['org.example.github'])).toBe(true);
    expect(isExtensionInstalled(ext, ['org.other.ext'])).toBe(false);
  });

  it('matches full macOS path by manifest ID', () => {
    const ext = {
      id: 'org.launcher.browser',
      slug: 'browser',
      manifest: { id: 'org.launcher.browser' },
    };
    const paths = [
      '/Users/khoshbin/Library/Application Support/org.launcher/extensions/org.launcher.browser',
    ];
    expect(isExtensionInstalled(ext, paths)).toBe(true);
  });

  it('matches full Windows path with backslashes', () => {
    const ext = {
      id: 'org.launcher.browser',
      slug: 'browser',
      manifest: { id: 'org.launcher.browser' },
    };
    const paths = ['C:\\Users\\User\\AppData\\Roaming\\org.launcher\\extensions\\org.launcher.browser'];
    expect(isExtensionInstalled(ext, paths)).toBe(true);
  });

  it('matches path with trailing slash', () => {
    const ext = {
      id: 'org.launcher.coffee',
      slug: 'coffee',
      manifest: { id: 'org.launcher.coffee' },
    };
    const paths = [
      '/Users/khoshbin/Library/Application Support/org.launcher/extensions/org.launcher.coffee/',
    ];
    expect(isExtensionInstalled(ext, paths)).toBe(true);
  });

  it('matches numeric store ID folder', () => {
    const ext = {
      id: 42,
      slug: 'my-extension',
      manifest: { id: 'com.example.myextension' },
    };
    const paths = ['/path/to/extensions/42'];
    expect(isExtensionInstalled(ext, paths)).toBe(true);
  });

  it('matches slug folder name', () => {
    const ext = {
      id: 'ext_999',
      slug: 'speed-test',
      manifest: { id: 'org.launcher.speedtest' },
    };
    const paths = ['/path/to/extensions/speed-test'];
    expect(isExtensionInstalled(ext, paths)).toBe(true);
  });

  it('supports Set input', () => {
    const ext = {
      id: 'org.launcher.browser',
      manifest: { id: 'org.launcher.browser' },
    };
    const set = new Set(['/path/to/extensions/org.launcher.browser']);
    expect(isExtensionInstalled(ext, set)).toBe(true);
  });
});

describe('isEmojiInstalled', () => {
  it('returns true when emoji manifest ID is in paths', () => {
    expect(isEmojiInstalled(['org.launcher.emoji'])).toBe(true);
    expect(isEmojiInstalled(['/path/to/org.launcher.emoji'])).toBe(true);
    expect(isEmojiInstalled(['/path/to/emoji'])).toBe(true);
  });

  it('returns false when emoji is not installed', () => {
    expect(isEmojiInstalled([])).toBe(false);
    expect(isEmojiInstalled(['/path/to/org.launcher.browser'])).toBe(false);
    expect(isEmojiInstalled(null)).toBe(false);
  });
});
