import type { GpuTiledPreviewPublication } from "@hot-trimmer/ipc-contracts";

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

export interface GpuTiledPreviewPayloadClient {
  getPayload: (request: { protocolVersion: number; generation: number; opaqueHandle: string }) => Promise<Uint8Array | ArrayBuffer>;
  releasePayload: (request: { protocolVersion: number; generation: number; opaqueHandle: string }) => Promise<void>;
}

export interface GpuTiledPreviewPaintSummary {
  generation: number;
  payloadBytes: number;
  payloadNonTransparent: number;
  payloadNonZeroRgb: number;
  validPayload: boolean;
  painted: boolean;
}

export function isValidGpuTiledPreviewPayload(
  publication: GpuTiledPreviewPublication,
  payloadByteLength: number,
): boolean {
  const { manifest } = publication;
  const rowBytes = manifest.width * 4;
  const expectedBytes = manifest.rowStride * manifest.height;
  const sourceX = manifest.validRect.x - manifest.outputRect.x;
  const sourceY = manifest.validRect.y - manifest.outputRect.y;
  return manifest.pixelFormat === "rgba8UnormSrgb"
    && Number.isSafeInteger(expectedBytes)
    && payloadByteLength === expectedBytes
    && rowBytes <= manifest.rowStride
    && sourceX >= 0
    && sourceY >= 0
    && sourceX + manifest.validRect.width <= manifest.width
    && sourceY + manifest.validRect.height <= manifest.height;
}

export function shouldDisplayGpuTiledPreview(
  manifestMap: string | undefined,
  mapView: string,
  hasTransientSourceFallback: boolean,
): boolean {
  return !hasTransientSourceFallback && gpuTiledPreviewMapMatches(manifestMap, mapView);
}

export function gpuTiledPreviewMapMatches(
  manifestMap: string | undefined,
  mapView: string,
): boolean {
  return normalizePreviewMapKey(manifestMap) === normalizePreviewMapKey(mapView);
}

function normalizePreviewMapKey(value: string | undefined): string | undefined {
  switch (value) {
    case "BaseColor":
    case "base_color":
      return "baseColor";
    case "AmbientOcclusion":
    case "ambient_occlusion":
      return "ambientOcclusion";
    case "MaterialId":
    case "material_id":
      return "materialId";
    case "EdgeMask":
    case "edge_mask":
      return "edgeMask";
    default:
      return value;
  }
}

/**
 * Owns only frontend display resources. Tile placement remains entirely native
 * metadata: no atlas, crop, or region calculations are reconstructed here.
 */
export class GpuTiledPreviewPainter {
  private latestGeneration = 0;
  private stagingCanvas: HTMLCanvasElement | null = null;
  private summary: GpuTiledPreviewPaintSummary | null = null;

  beginGeneration(generation: number): void {
    this.latestGeneration = Math.max(this.latestGeneration, generation);
  }

  lastSummary(): GpuTiledPreviewPaintSummary | null {
    return this.summary;
  }

  async paint(
    surface: HTMLCanvasElement,
    publication: GpuTiledPreviewPublication,
    client: GpuTiledPreviewPayloadClient,
    protocolVersion: number,
  ): Promise<boolean> {
    const { manifest } = publication;
    try {
      if (manifest.generation !== this.latestGeneration || manifest.pixelFormat !== "rgba8UnormSrgb") return false;
      const payload = gpuTiledPreviewPayloadBytes(await client.getPayload({ protocolVersion, generation: manifest.generation, opaqueHandle: manifest.opaqueHandle }));
      this.summary = summarizePayload(manifest.generation, payload, false, false);
      if (manifest.generation !== this.latestGeneration) return false;
      const validPayload = isValidGpuTiledPreviewPayload(publication, payload.byteLength);
      this.summary = summarizePayload(manifest.generation, payload, validPayload, false);
      if (!validPayload) return false;

      const staging = this.ensureStagingCanvas(surface);
      const context = staging.getContext("2d");
      if (!context) return false;
      const rowBytes = manifest.width * 4;
      const pixels = manifest.rowStride === rowBytes
        ? copyRgbaBytes(payload)
        : copyTightRgbaRows(payload, manifest.rowStride, rowBytes, manifest.height);
      const image = new ImageData(pixels, manifest.width, manifest.height, { colorSpace: "srgb" });
      const sourceX = manifest.validRect.x - manifest.outputRect.x;
      const sourceY = manifest.validRect.y - manifest.outputRect.y;
      context.putImageData(
        image,
        manifest.outputRect.x,
        manifest.outputRect.y,
        sourceX,
        sourceY,
        manifest.validRect.width,
        manifest.validRect.height,
      );

      if (manifest.generation !== this.latestGeneration) return false;
      const display = surface.getContext("2d");
      if (!display) return false;
      display.clearRect(0, 0, surface.width, surface.height);
      display.drawImage(staging, 0, 0);
      this.summary = summarizePayload(manifest.generation, payload, true, true);
      return true;
    } finally {
      void client.releasePayload({ protocolVersion, generation: manifest.generation, opaqueHandle: manifest.opaqueHandle });
    }
  }

  dispose(): void {
    this.latestGeneration = Number.MAX_SAFE_INTEGER;
    this.stagingCanvas = null;
    this.summary = null;
  }

  private ensureStagingCanvas(surface: HTMLCanvasElement): HTMLCanvasElement {
    if (this.stagingCanvas === null) this.stagingCanvas = document.createElement("canvas");
    if (this.stagingCanvas.width !== surface.width || this.stagingCanvas.height !== surface.height) {
      this.stagingCanvas.width = surface.width;
      this.stagingCanvas.height = surface.height;
      const stagingContext = this.stagingCanvas.getContext("2d");
      if (stagingContext) stagingContext.drawImage(surface, 0, 0);
    }
    return this.stagingCanvas;
  }
}

export function gpuTiledPreviewPayloadBytes(payload: Uint8Array | ArrayBuffer): Uint8Array {
  return payload instanceof Uint8Array ? payload : new Uint8Array(payload);
}

function summarizePayload(
  generation: number,
  payload: Uint8Array,
  validPayload: boolean,
  painted: boolean,
): GpuTiledPreviewPaintSummary {
  let payloadNonTransparent = 0;
  let payloadNonZeroRgb = 0;
  for (let index = 0; index < payload.length; index += 4) {
    if (payload[index + 3] !== 0) payloadNonTransparent += 1;
    if (payload[index] !== 0 || payload[index + 1] !== 0 || payload[index + 2] !== 0) payloadNonZeroRgb += 1;
  }
  return {
    generation,
    payloadBytes: payload.byteLength,
    payloadNonTransparent,
    payloadNonZeroRgb,
    validPayload,
    painted,
  };
}

function copyRgbaBytes(payload: Uint8Array): Uint8ClampedArray<ArrayBuffer> {
  const pixels = new Uint8ClampedArray(new ArrayBuffer(payload.byteLength));
  pixels.set(payload);
  return pixels;
}

function copyTightRgbaRows(payload: Uint8Array, rowStride: number, rowBytes: number, height: number): Uint8ClampedArray<ArrayBuffer> {
  const pixels = new Uint8ClampedArray(rowBytes * height);
  for (let row = 0; row < height; row += 1) {
    pixels.set(payload.subarray(row * rowStride, row * rowStride + rowBytes), row * rowBytes);
  }
  return pixels;
}
