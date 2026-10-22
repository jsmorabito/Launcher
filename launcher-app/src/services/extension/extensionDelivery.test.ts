/** @vitest-environment jsdom */
import { describe, it, expect, vi } from 'vitest';

vi.mock('../../lib/ipc/extensionOrigin', () => ({
  getExtensionFrameOrigin: (id: string) => `launcher-extension://${id}`,
}));

import { post } from './extensionDelivery';

function makeIframe(id: string) {
  const iframe = document.createElement('iframe');
  iframe.setAttribute('data-extension-id', id);
  const postMessage = vi.fn();
  Object.defineProperty(iframe, 'contentWindow', {
    value: { postMessage } as any,
    configurable: true,
  });
  return { iframe, postMessage };
}

describe('extensionDelivery.post', () => {
  it('posts command kind as launcher:command:execute with {commandId,args}', () => {
    const { iframe, postMessage } = makeIframe('ext.a');
    post(iframe, {
      kind: 'command',
      source: 'search',
      payload: { commandId: 'run', args: { foo: 1 } },
    });
    expect(postMessage).toHaveBeenCalledWith(
      { type: 'launcher:command:execute', payload: { commandId: 'run', args: { foo: 1 } } },
      'launcher-extension://ext.a',
    );
  });

  it('posts action kind as launcher:action:execute with {actionId}', () => {
    const { iframe, postMessage } = makeIframe('ext.a');
    post(iframe, { kind: 'action', source: 'search', payload: { actionId: 'open' } });
    expect(postMessage).toHaveBeenCalledWith(
      { type: 'launcher:action:execute', payload: { actionId: 'open' } },
      'launcher-extension://ext.a',
    );
  });

  it('posts viewSubmit as launcher:view:submit', () => {
    const { iframe, postMessage } = makeIframe('ext.a');
    post(iframe, { kind: 'viewSubmit', source: 'search', payload: { query: 'hi' } });
    expect(postMessage).toHaveBeenCalledWith(
      { type: 'launcher:view:submit', payload: { query: 'hi' } },
      'launcher-extension://ext.a',
    );
  });

  it('does not post for predictiveWarm kind', () => {
    const { iframe, postMessage } = makeIframe('ext.a');
    post(iframe, { kind: 'predictiveWarm', source: 'userHighlight', payload: {} });
    expect(postMessage).not.toHaveBeenCalled();
  });
});
