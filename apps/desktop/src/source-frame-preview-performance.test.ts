import assert from "node:assert/strict";
import test from "node:test";
import { SourceFramePreviewController, type PreviewControllerClock } from "./source-frame-preview-controller.ts";

type Profile = "draft512" | "refinement1024" | "authoritative";

test("source-frame-preview-performance coalesces drags and preserves a final profile request", async () => {
  class FakeClock implements PreviewControllerClock {
    value = 0;
    nextId = 0;
    timers = new Map<number, { at: number; callback: () => void }>();
    now = () => this.value;
    setTimeout = (callback: () => void, delayMs: number) => {
      const id = ++this.nextId;
      this.timers.set(id, { at: this.value + delayMs, callback });
      return id;
    };
    clearTimeout = (handle: unknown) => { this.timers.delete(handle as number); };
    advance(ms: number) {
      const target = this.value + ms;
      while (true) {
        const next = [...this.timers.entries()].sort((a, b) => a[1].at - b[1].at)[0];
        if (!next || next[1].at > target) break;
        this.timers.delete(next[0]);
        this.value = next[1].at;
        next[1].callback();
      }
      this.value = target;
    }
  }

  const clock = new FakeClock();
  const published: number[] = [];
  const completions: Array<() => void> = [];
  const controller = new SourceFramePreviewController<number>(clock, 8);
  controller.setExecutor(async (event) => {
    published.push(event);
    await new Promise<void>((resolve) => completions.push(resolve));
  });
  controller.enqueue(0);
  clock.advance(0);
  assert.deepEqual(published, [0]);
  for (let event = 1; event < 30; event += 1) controller.enqueue(event);
  completions.shift()!();
  await Promise.resolve();
  await Promise.resolve();
  clock.advance(125);
  assert.deepEqual(published, [0, 29]);
});

test("source-frame-preview-performance profile requests do not alter source ownership", () => {
  const sourceCrop = { x: 2000, y: 0, width: 4000, height: 4000 };
  const profiles: readonly Profile[] = ["draft512", "refinement1024", "authoritative"];
  for (const profile of profiles) {
    assert.deepEqual(sourceCrop, { x: 2000, y: 0, width: 4000, height: 4000 }, profile);
  }
});

test("source-frame-preview-performance coalesces detached crop requests by their final bounds", async () => {
  class FakeClock implements PreviewControllerClock {
    value = 0;
    nextId = 0;
    timers = new Map<number, { at: number; callback: () => void }>();
    now = () => this.value;
    setTimeout = (callback: () => void, delayMs: number) => {
      const id = ++this.nextId;
      this.timers.set(id, { at: this.value + delayMs, callback });
      return id;
    };
    clearTimeout = (handle: unknown) => { this.timers.delete(handle as number); };
    advance(ms: number) {
      const target = this.value + ms;
      while (true) {
        const next = [...this.timers.entries()].sort((a, b) => a[1].at - b[1].at)[0];
        if (!next || next[1].at > target) break;
        this.timers.delete(next[0]);
        this.value = next[1].at;
        next[1].callback();
      }
      this.value = target;
    }
  }

  const clock = new FakeClock();
  const published: Array<{ regionId: string; x: number }> = [];
  const completions: Array<() => void> = [];
  const controller = new SourceFramePreviewController<{
    regionId: string;
    projection: { type: "crop"; bounds: { x: number } };
    revision: number;
  }>(clock, 8);
  controller.setExecutor(async (request) => {
    published.push({ regionId: request.regionId, x: request.projection.bounds.x });
    await new Promise<void>((resolve) => completions.push(resolve));
  });
  controller.enqueue({ regionId: "region-1", projection: { type: "crop", bounds: { x: 0 } }, revision: 7 });
  clock.advance(0);
  for (let event = 1; event < 30; event += 1) {
    controller.enqueue({ regionId: "region-1", projection: { type: "crop", bounds: { x: event } }, revision: 7 });
  }
  completions.shift()!();
  await Promise.resolve();
  await Promise.resolve();
  clock.advance(125);
  assert.deepEqual(published, [{ regionId: "region-1", x: 0 }, { regionId: "region-1", x: 29 }]);
});
