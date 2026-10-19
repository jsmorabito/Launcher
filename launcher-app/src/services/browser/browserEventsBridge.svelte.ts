import { createPushBridge } from '../eventPushBridge/createPushBridge';

/** Routes Rust subscription events through the shared worker-aware delivery bridge. */
export const browserEventsBridge = createPushBridge(
  'launcher:browser-event',
  'launcher:event:browser-event:push',
  'browserEventsBridge',
);
