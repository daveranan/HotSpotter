import assert from "node:assert/strict";
import test from "node:test";

import { IPC_PROTOCOL_VERSION, type TrimSheetDocumentCommand } from "./document-contracts.ts";

test("document commands keep the typed protocol boundary", () => {
  const command: TrimSheetDocumentCommand = {
    type: "set_output_resolution",
    outputSize: { width: 2048, height: 2048 },
  };
  assert.equal(IPC_PROTOCOL_VERSION, 1);
  assert.equal(command.type, "set_output_resolution");
});
