import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  IPC_PROTOCOL_VERSION,
  type CommandFailure,
  type FoundationStatus,
  type FoundationStatusRequest,
} from "@hot-trimmer/ipc-contracts";
import "../styles.css";

const workflow = [
  "Open Image",
  "Mark Patches",
  "Layout",
  "Generate Maps",
  "Polish",
  "Preview",
  "Export",
] as const;

function isNativeRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function App(): React.JSX.Element {
  const [status, setStatus] = useState<FoundationStatus | null>(null);
  const [failure, setFailure] = useState<CommandFailure | null>(null);

  useEffect(() => {
    if (!isNativeRuntime()) return;

    const request: FoundationStatusRequest = { protocolVersion: IPC_PROTOCOL_VERSION };
    void invoke<FoundationStatus>("foundation_status", { request })
      .then(setStatus)
      .catch((reason: unknown) => {
        setFailure({
          code: "native_command_failed",
          message: "The native foundation did not respond.",
          recovery: "Restart Hot Trimmer and open Diagnostics if the problem continues.",
          detail: reason instanceof Error ? reason.message : String(reason),
        });
      });
  }, []);

  async function verifyNativeDialog(): Promise<void> {
    setFailure(null);
    try {
      await open({ directory: true, multiple: false, title: "Verify native folder dialog" });
    } catch (reason) {
      setFailure({
        code: "native_dialog_failed",
        message: "The operating system folder dialog could not be opened.",
        recovery: "Retry after restarting Hot Trimmer.",
        detail: reason instanceof Error ? reason.message : String(reason),
      });
    }
  }

  return (
    <main className="foundation-shell" aria-label="Hot Trimmer native desktop foundation">
      <header className="topbar">
        <strong className="brand">Hot Trimmer</strong>
        <nav className="workflow" aria-label="MVP workflow">
          {workflow.map((step, index) => (
            <button key={step} className={`step ${index === 0 ? "active" : ""}`} disabled={index > 0}>
              {step}
            </button>
          ))}
        </nav>
      </header>

      <section className="foundation-content" aria-labelledby="foundation-title">
        <div>
          <p className="eyebrow">Phase 0 · Engineering Foundation</p>
          <h1 id="foundation-title">Native shell online</h1>
          <p>
            The production Rust boundary, versioned IPC contract, native paths, and diagnostics are ready.
            Image import begins in Phase 1.
          </p>
          <div className="foundation-actions">
            <button className="primary" onClick={() => void verifyNativeDialog()} disabled={!isNativeRuntime()}>
              Verify native dialog
            </button>
            <span>{isNativeRuntime() ? "Native runtime" : "Browser preview"}</span>
          </div>
        </div>

        <dl className="foundation-status" aria-label="Foundation status">
          <div><dt>IPC protocol</dt><dd>{status?.protocolVersion ?? IPC_PROTOCOL_VERSION}</dd></div>
          <div><dt>Application</dt><dd>{status?.appVersion ?? "0.1.0"}</dd></div>
          <div><dt>Platform</dt><dd>{status?.platform ?? "web preview"}</dd></div>
          <div><dt>Network</dt><dd>Disabled by product policy</dd></div>
          <div><dt>Project writes</dt><dd>Deferred to Phase 1</dd></div>
        </dl>

        {failure ? (
          <section className="foundation-error" role="alert">
            <strong>{failure.message}</strong>
            <span>{failure.recovery}</span>
            {failure.detail ? <code>{failure.detail}</code> : null}
          </section>
        ) : null}
      </section>

      <footer className="status">
        <span>Production architecture</span>
        <span>Mode: Foundation</span>
        <span>Persistent formats versioned</span>
        <span>Offline by default</span>
      </footer>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

