import assert from "node:assert/strict";
import test from "node:test";
import fixture from "../../../fixtures/contracts/foundation-status.json" with { type: "json" };
import { IPC_PROTOCOL_VERSION } from "./index.ts";

const requiredCapabilities = [
  "native_paths",
  "typed_ipc",
  "structured_diagnostics",
  "native_dialog",
];

test("foundation fixture uses the current protocol and complete response shape", () => {
  assert.equal(fixture.request.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(fixture.response.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(typeof fixture.response.appVersion, "string");
  assert.equal(typeof fixture.response.platform, "string");
  assert.deepEqual(fixture.response.capabilities, requiredCapabilities);
  assert.deepEqual(Object.keys(fixture.response.directories).sort(), ["appData", "cache", "logs", "recovery"]);
});

