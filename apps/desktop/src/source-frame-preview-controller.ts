export interface PreviewControllerClock {
  now: () => number;
  setTimeout: (callback: () => void, delayMs: number) => unknown;
  clearTimeout: (handle: unknown) => void;
}

const browserClock: PreviewControllerClock = {
  now: () => performance.now(),
  setTimeout: (callback, delayMs) => window.setTimeout(callback, delayMs),
  clearTimeout: (handle) => window.clearTimeout(handle as number),
};

/** Coalesces transient preview work while guaranteeing the newest request is published. */
export class SourceFramePreviewController<T> {
  private executor: ((request: T) => Promise<void>) | null = null;
  private pending: T | null = null;
  private timer: unknown = null;
  private inFlight = false;
  private lastStartedAt = -Infinity;
  private intervalMs: number;

  constructor(clock: PreviewControllerClock = browserClock, maxFps = 30) {
    this.clock = clock;
    this.intervalMs = 1000 / Math.max(1, maxFps);
  }

  private readonly clock: PreviewControllerClock;

  setExecutor(executor: (request: T) => Promise<void>): void {
    this.executor = executor;
  }

  setMaxFps(maxFps: number): void {
    this.intervalMs = 1000 / Math.max(1, maxFps);
  }

  enqueue(request: T): void {
    this.pending = request;
    this.pump();
  }

  cancel(): void {
    this.pending = null;
    if (this.timer !== null) {
      this.clock.clearTimeout(this.timer);
      this.timer = null;
    }
  }

  private pump(): void {
    if (this.inFlight || this.timer !== null || this.pending === null || this.executor === null) return;
    const delay = Math.max(0, this.intervalMs - (this.clock.now() - this.lastStartedAt));
    this.timer = this.clock.setTimeout(() => {
      this.timer = null;
      const request = this.pending;
      this.pending = null;
      if (request === null || this.executor === null) return;
      this.inFlight = true;
      this.lastStartedAt = this.clock.now();
      void this.executor(request).finally(() => {
        this.inFlight = false;
        this.pump();
      });
    }, delay);
  }
}
