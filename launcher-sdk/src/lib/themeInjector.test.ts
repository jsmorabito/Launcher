import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setupThemeInjection, injectThemeVariables, injectFontFaceCSS } from './themeInjector';

describe('themeInjector', () => {
  beforeEach(() => {
    document.head.textContent = '';
    document.body.textContent = '';
    vi.restoreAllMocks();
  });

  it('injectThemeVariables creates a <style id="launcher-theme-vars"> in document.head', () => {
    injectThemeVariables({ '--color-bg': 'red' });

    const style = document.getElementById('launcher-theme-vars') as HTMLStyleElement;
    expect(style).toBeTruthy();
    expect(style.parentElement).toBe(document.head);
    expect(style.textContent).toContain('--color-bg: red;');
  });

  it('injectThemeVariables populates it with :root declarations', () => {
    injectThemeVariables({
      '--color-bg': 'red',
      '--color-text': 'white',
    });

    const style = document.getElementById('launcher-theme-vars')!;
    expect(style.textContent).toContain(':root:root:root {');
    expect(style.textContent).toContain('--color-bg: red;');
    expect(style.textContent).toContain('--color-text: white;');
  });

  it('calling injectThemeVariables twice updates the existing style element', () => {
    injectThemeVariables({ '--color-bg': 'red' });
    const initialStyle = document.getElementById('launcher-theme-vars');

    injectThemeVariables({ '--color-bg': 'blue' });
    const updatedStyle = document.getElementById('launcher-theme-vars');

    expect(initialStyle).toBe(updatedStyle);
    expect(updatedStyle?.textContent).toContain('--color-bg: blue;');
  });

  it('injectThemeVariables with empty object produces an empty :root {} block', () => {
    injectThemeVariables({});
    const style = document.getElementById('launcher-theme-vars');
    expect(style?.textContent).toContain(':root:root:root {\n\n}');
  });

  it('receiving launcher:theme:variables message triggers injection', () => {
    setupThemeInjection();

    const themeVars = { '--color-bg': 'green' };
    const event = new MessageEvent('message', {
      data: {
        type: 'launcher:theme:variables',
        payload: themeVars,
      },
    });

    window.dispatchEvent(event);

    const style = document.getElementById('launcher-theme-vars');
    expect(style?.textContent).toContain('--color-bg: green;');
  });

  it('injectFontFaceCSS creates a <style id="launcher-theme-fonts"> in document.head', () => {
    const mockCSS = '@font-face { font-family: "Satoshi"; }';
    injectFontFaceCSS(mockCSS);

    const style = document.getElementById('launcher-theme-fonts') as HTMLStyleElement;
    expect(style).toBeTruthy();
    expect(style.parentElement).toBe(document.head);
    expect(style.textContent).toBe(mockCSS);
  });

  it('calling injectFontFaceCSS twice updates the existing style element', () => {
    injectFontFaceCSS('body { font-family: Satoshi; }');
    const initialStyle = document.getElementById('launcher-theme-fonts');

    injectFontFaceCSS('body { font-family: JetBrains; }');
    const updatedStyle = document.getElementById('launcher-theme-fonts');

    expect(initialStyle).toBe(updatedStyle);
    expect(updatedStyle?.textContent).toBe('body { font-family: JetBrains; }');
  });

  it('receiving launcher:theme:fonts message triggers injection', () => {
    setupThemeInjection();

    const mockCSS = '/* merged font faces */';
    const event = new MessageEvent('message', {
      data: {
        type: 'launcher:theme:fonts',
        payload: mockCSS,
      },
    });

    window.dispatchEvent(event);

    const style = document.getElementById('launcher-theme-fonts');
    expect(style?.textContent).toBe(mockCSS);
  });

  it('ignores launcher:theme:fonts with non-string payload', () => {
    setupThemeInjection();

    const event = new MessageEvent('message', {
      data: {
        type: 'launcher:theme:fonts',
        payload: { invalid: 'type' },
      },
    });

    window.dispatchEvent(event);

    const style = document.getElementById('launcher-theme-fonts');
    expect(style).toBeNull();
  });
});
