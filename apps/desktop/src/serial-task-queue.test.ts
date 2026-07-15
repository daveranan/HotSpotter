import assert from "node:assert/strict";
import test from "node:test";
import { SerialTaskQueue } from "./serial-task-queue.ts";

test("native mutations run in acceptance order even when later work is faster", async () => {
  const queue = new SerialTaskQueue();
  const order: string[] = [];
  let releaseFirst!: () => void;
  const firstGate = new Promise<void>((resolve) => { releaseFirst = resolve; });

  const first = queue.run(async () => {
    order.push("first:start");
    await firstGate;
    order.push("first:end");
  });
  const second = queue.run(async () => { order.push("second"); });

  await Promise.resolve();
  assert.deepEqual(order, ["first:start"]);
  releaseFirst();
  await Promise.all([first, second]);
  assert.deepEqual(order, ["first:start", "first:end", "second"]);
});

test("a rejected mutation does not strand later edits", async () => {
  const queue = new SerialTaskQueue();
  await assert.rejects(queue.run(async () => { throw new Error("rejected"); }), /rejected/);
  assert.equal(await queue.run(async () => "recovered"), "recovered");
});
