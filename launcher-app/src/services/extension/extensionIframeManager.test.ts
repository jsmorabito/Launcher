import { describe, it, expect, vi, beforeEach } from 'vitest';

// Set up browser globals for Node environment
let mockPostMessage: ReturnType<typeof vi.fn>;
let mockIframe: any;

if (typeof document === 'undefined') {
  mockPostMessage = vi.fn();
  mockIframe = {
    contentWindow: { postMessage: mockPostMessage },
    dataset: { extensionId: 'org.launcher.tauri-docs' },
  };
  (global as any).document = {
    querySelector: vi.fn((selector: string) => {
      if (selector.includes('org.launcher.tauri-docs')) return mockIframe;
      return null;
    }),
    querySelectorAll: vi.fn().mockReturnValue([]),
  };
}

vi.mock('../../lib/ipc/extensionOrigin', () => ({
  getExtensionFrameOrigin: vi.fn((id: string) => `launcher-extension://${id}`),
}));
vi.mock('../log/logService', () => ({
  logService: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}));

vi.mock('./workerHost.svelte', () => ({
  workerHost: { getWorker: vi.fn(), hasWorker: vi.fn(() => false), post: vi.fn() },
}));
import { workerHost } from './workerHost.svelte';

import { ExtensionIframeManager } from './extensionIframeManager.svelte';

describe('ExtensionIframeManager', () => {
  let manager: ExtensionIframeManager;

  beforeEach(() => {
    vi.clearAllMocks();
    manager = new ExtensionIframeManager();

    // Reset the mock iframe's postMessage
    mockPostMessage = vi.fn();
    mockIframe.contentWindow = { postMessage: mockPostMessage };

    // Reset querySelector mock
    vi.mocked(document.querySelector).mockImplementation((selector: string) => {
      if (selector.includes('org.launcher.tauri-docs')) return mockIframe;
      return null;
    });
  });

  describe('sendViewSearchToExtension', () => {
    it('sends launcher:view:search message to the correct iframe', () => {
      manager.sendViewSearchToExtension('org.launcher.tauri-docs', 'prerequisites');

      // Selector carries [data-role="view"] first; falls back to worker, then
      // to unscoped, only if view is missing.
      expect(document.querySelector).toHaveBeenCalledWith(
        'iframe[data-extension-id="org.launcher.tauri-docs"][data-role="view"]',
      );
      expect(mockPostMessage).toHaveBeenCalledWith(
        { type: 'launcher:view:search', payload: { query: 'prerequisites' } },
        'launcher-extension://org.launcher.tauri-docs',
      );
    });

    it('does nothing when iframe is not found', () => {
      manager.sendViewSearchToExtension('nonexistent-ext', 'query');

      expect(mockPostMessage).not.toHaveBeenCalled();
    });

    it('forwards empty string query (clears search)', () => {
      manager.sendViewSearchToExtension('org.launcher.tauri-docs', '');

      expect(mockPostMessage).toHaveBeenCalledWith(
        { type: 'launcher:view:search', payload: { query: '' } },
        'launcher-extension://org.launcher.tauri-docs',
      );
    });
  });

  describe('sendFilterChangeToView', () => {
    it('posts launcher:event:searchBar:filterChange to the view-role iframe', () => {
      manager.sendFilterChangeToView('org.launcher.tauri-docs', {
        commandId: 'cmd',
        value: 'images',
      });

      // Selector carries [data-role="view"] first; mirrors sendViewSearchToExtension.
      expect(document.querySelector).toHaveBeenCalledWith(
        'iframe[data-extension-id="org.launcher.tauri-docs"][data-role="view"]',
      );
      expect(mockPostMessage).toHaveBeenCalledWith(
        {
          type: 'launcher:event:searchBar:filterChange',
          payload: { commandId: 'cmd', value: 'images' },
        },
        'launcher-extension://org.launcher.tauri-docs',
      );
    });

    it('no-ops when no view iframe is mounted for the extension', () => {
      expect(() =>
        manager.sendFilterChangeToView('missing-ext', {
          commandId: 'cmd',
          value: 'x',
        }),
      ).not.toThrow();
      expect(mockPostMessage).not.toHaveBeenCalled();
    });

    it('prefers view role over worker role', () => {
      const viewPost = vi.fn();
      const workerPost = vi.fn();
      const viewIframe = {
        contentWindow: { postMessage: viewPost },
        dataset: { extensionId: 'ext-multi', role: 'view' },
      };
      const workerIframe = {
        contentWindow: { postMessage: workerPost },
        dataset: { extensionId: 'ext-multi', role: 'worker' },
      };

      vi.mocked(document.querySelector).mockImplementation((selector: string) => {
        if (selector.includes('ext-multi') && selector.includes('[data-role="view"]'))
          return viewIframe as any;
        if (selector.includes('ext-multi') && selector.includes('[data-role="worker"]'))
          return workerIframe as any;
        return null;
      });

      manager.sendFilterChangeToView('ext-multi', {
        commandId: 'cmd',
        value: 'x',
      });

      expect(viewPost).toHaveBeenCalledTimes(1);
      expect(workerPost).not.toHaveBeenCalled();
    });
  });

  describe('handleExtensionSubmit', () => {
    it('sends launcher:view:submit message to the correct iframe', () => {
      manager.handleExtensionSubmit('org.launcher.tauri-docs', 'submit value');

      expect(mockPostMessage).toHaveBeenCalledWith(
        { type: 'launcher:view:submit', payload: { query: 'submit value' } },
        'launcher-extension://org.launcher.tauri-docs',
      );
    });

    it('does nothing when iframe is not found', () => {
      manager.handleExtensionSubmit('nonexistent-ext', 'query');

      expect(mockPostMessage).not.toHaveBeenCalled();
    });
  });

  it('routes root search to the real worker even when a view is mounted', async () => {
    const postMessage = vi.fn();
    vi.mocked(workerHost.getWorker).mockReturnValueOnce({ postMessage } as any);
    const response = manager.sendSearchRequestToExtension('org.launcher.tauri-docs', 'query');
    expect(postMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'launcher:search:request',
        payload: { query: 'query' },
      }),
    );
    const messageId = postMessage.mock.calls[0][0].messageId;
    manager.handleSearchResponse({
      data: { type: 'launcher:search:response', messageId, result: [{ id: 'hit' }] },
    } as MessageEvent);
    expect(await response).toEqual([{ id: 'hit' }]);
    expect(mockPostMessage).not.toHaveBeenCalled();
  });

  describe('sendSearchRequestToExtension', () => {
    it('settles and drops pending requests when a query is superseded', async () => {
      vi.useFakeTimers();
      const first = manager.sendSearchRequestToExtension('org.launcher.tauri-docs', 'old');
      const firstMessageId = mockPostMessage.mock.calls[0][0].messageId;

      manager.cancelPendingSearchRequests();

      await expect(first).resolves.toEqual([]);
      manager.handleSearchResponse({
        data: {
          type: 'launcher:search:response',
          messageId: firstMessageId,
          result: [{ id: 'late' }],
        },
      } as MessageEvent);
      expect(vi.getTimerCount()).toBe(0);
      vi.useRealTimers();
    });

    it('sends launcher:search:request and resolves empty when no iframe', async () => {
      const result = await manager.sendSearchRequestToExtension('nonexistent-ext', 'query');

      expect(result).toEqual([]);
    });

    it('sends launcher:search:request with unique messageId to iframe', async () => {
      // Don't await — the promise won't resolve until a response comes
      const promise = manager.sendSearchRequestToExtension('org.launcher.tauri-docs', 'tauri');

      expect(mockPostMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'launcher:search:request',
          messageId: expect.stringContaining('search_'),
          payload: { query: 'tauri' },
        }),
        'launcher-extension://org.launcher.tauri-docs',
      );

      // Simulate response to clean up the pending request
      const sentMessageId = mockPostMessage.mock.calls[0][0].messageId;
      manager.handleSearchResponse({
        data: {
          type: 'launcher:search:response',
          messageId: sentMessageId,
          result: [{ title: 'Commands' }],
        },
      } as MessageEvent);

      const result = await promise;
      expect(result).toEqual([{ title: 'Commands' }]);
    });
  });

  describe('sendActionExecuteToExtension (role routing)', () => {
    // Set up separate view + worker iframes so we can observe which one got the post.
    let viewPostMessage: ReturnType<typeof vi.fn>;
    let workerPostMessage: ReturnType<typeof vi.fn>;
    let unscopedPostMessage: ReturnType<typeof vi.fn>;

    beforeEach(() => {
      viewPostMessage = vi.fn();
      workerPostMessage = vi.fn();
      unscopedPostMessage = vi.fn();
      const viewIframe = {
        contentWindow: { postMessage: viewPostMessage },
        dataset: { extensionId: 'ext-1', role: 'view' },
      };
      const workerIframe = {
        contentWindow: { postMessage: workerPostMessage },
        dataset: { extensionId: 'ext-1', role: 'worker' },
      };
      const unscopedIframe = {
        contentWindow: { postMessage: unscopedPostMessage },
        dataset: { extensionId: 'legacy-ext' },
      };

      vi.mocked(document.querySelector).mockImplementation((selector: string) => {
        if (selector.includes('ext-1') && selector.includes('[data-role="view"]'))
          return viewIframe as any;
        if (selector.includes('ext-1') && selector.includes('[data-role="worker"]'))
          return workerIframe as any;
        if (selector === 'iframe[data-extension-id="ext-1"]') return viewIframe as any;
        if (selector === 'iframe[data-extension-id="legacy-ext"]') return unscopedIframe as any;
        return null;
      });
    });

    it('role="worker" routes launcher:action:execute to the worker iframe', () => {
      manager.sendActionExecuteToExtension('ext-1', 'send-notification', 'worker');
      expect(workerPostMessage).toHaveBeenCalledWith(
        { type: 'launcher:action:execute', payload: { actionId: 'send-notification' } },
        'launcher-extension://ext-1',
      );
      expect(viewPostMessage).not.toHaveBeenCalled();
    });

    it('role="view" routes launcher:action:execute to the view iframe', () => {
      manager.sendActionExecuteToExtension('ext-1', 'show-modal', 'view');
      expect(viewPostMessage).toHaveBeenCalledWith(
        { type: 'launcher:action:execute', payload: { actionId: 'show-modal' } },
        'launcher-extension://ext-1',
      );
      expect(workerPostMessage).not.toHaveBeenCalled();
    });

    it('no role: prefers view, falls back to worker (matches createPushBridge pattern)', () => {
      manager.sendActionExecuteToExtension('ext-1', 'some-action');
      expect(viewPostMessage).toHaveBeenCalledOnce();
      expect(workerPostMessage).not.toHaveBeenCalled();
    });

    it('legacy single-iframe extension (no role attr): falls back to unscoped selector', () => {
      manager.sendActionExecuteToExtension('legacy-ext', 'legacy-action');
      expect(unscopedPostMessage).toHaveBeenCalledWith(
        { type: 'launcher:action:execute', payload: { actionId: 'legacy-action' } },
        'launcher-extension://legacy-ext',
      );
    });

    it('includes actionPayload in the envelope when a payload is supplied', () => {
      manager.sendActionExecuteToExtension('ext-1', 'open-item', 'worker', {
        url: 'https://x.test',
      });
      expect(workerPostMessage).toHaveBeenCalledWith(
        {
          type: 'launcher:action:execute',
          payload: { actionId: 'open-item', actionPayload: { url: 'https://x.test' } },
        },
        'launcher-extension://ext-1',
      );
    });
  });

  describe('handleSearchResponse', () => {
    it('ignores messages that are not launcher:search:response', () => {
      // Should not throw
      manager.handleSearchResponse({ data: { type: 'other:message' } } as MessageEvent);
      manager.handleSearchResponse({ data: null } as MessageEvent);
    });

    it('resolves with empty array on error response', async () => {
      const promise = manager.sendSearchRequestToExtension('org.launcher.tauri-docs', 'test');
      const sentMessageId = mockPostMessage.mock.calls[0][0].messageId;

      manager.handleSearchResponse({
        data: { type: 'launcher:search:response', messageId: sentMessageId, error: 'Search failed' },
      } as MessageEvent);

      const result = await promise;
      expect(result).toEqual([]);
    });
  });
});

it('preferences reach a mounted Web Worker', () => {
  vi.mocked(workerHost.hasWorker).mockReturnValue(true);
  new ExtensionIframeManager().sendPreferencesToExtension('org.worker', {
    extension: { x: 1 },
    commands: {},
  });
  expect(workerHost.post).toHaveBeenCalledWith('org.worker', {
    type: 'launcher:event:preferences:set-all',
    payload: { extension: { x: 1 }, commands: {} },
  });
});
