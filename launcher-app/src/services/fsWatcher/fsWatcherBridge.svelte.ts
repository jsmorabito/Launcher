import { createPushBridge } from '../eventPushBridge/createPushBridge';

/** Routes Rust subscription events through the shared worker-aware delivery bridge. */
export const fsWatcherBridge = createPushBridge(
  'launcher:fs-watch',
  'launcher:event:fs-watch:push',
  'fsWatcherBridge',
);
