import { useEffect, useRef, useState } from "react";
import type { PatchGeometry } from "@hot-trimmer/ipc-contracts";
import { quadProjection } from "./patch-authoring";

interface LiveRectifiedCanvasProps {
  geometry: PatchGeometry;
  imageUrl: string;
  label: string;
  aspectRatio?: number;
}

interface SurfaceSize { width: number; height: number }

function automaticAspect(geometry: PatchGeometry, image: HTMLImageElement): number {
  const distance = (first: { x: number; y: number }, second: { x: number; y: number }): number => Math.hypot(
    (second.x - first.x) * image.naturalWidth,
    (second.y - first.y) * image.naturalHeight,
  );
  const [topLeft, topRight, bottomRight, bottomLeft] = geometry.corners;
  if (!topLeft || !topRight || !bottomRight || !bottomLeft) return 1;
  const width = (distance(topLeft, topRight) + distance(bottomLeft, bottomRight)) / 2;
  const height = (distance(topLeft, bottomLeft) + distance(topRight, bottomRight)) / 2;
  return Math.max(0.01, width / Math.max(0.01, height));
}

function fittedSize(container: HTMLDivElement, aspectRatio: number): SurfaceSize {
  const availableWidth = Math.max(1, container.clientWidth);
  const availableHeight = Math.max(1, container.clientHeight);
  if (availableWidth / availableHeight > aspectRatio) {
    return { width: availableHeight * aspectRatio, height: availableHeight };
  }
  return { width: availableWidth, height: availableWidth / aspectRatio };
}

function shader(context: WebGLRenderingContext, kind: number, source: string): WebGLShader {
  const compiled = context.createShader(kind);
  if (!compiled) throw new Error("WebGL shader allocation failed");
  context.shaderSource(compiled, source);
  context.compileShader(compiled);
  if (!context.getShaderParameter(compiled, context.COMPILE_STATUS)) {
    throw new Error(context.getShaderInfoLog(compiled) ?? "WebGL shader compilation failed");
  }
  return compiled;
}

function draw(canvas: HTMLCanvasElement, image: HTMLImageElement, geometry: PatchGeometry): void {
  const context = canvas.getContext("webgl", { alpha: true, antialias: false });
  if (!context) return;
  const ratio = Math.min(2, window.devicePixelRatio || 1);
  const width = Math.max(1, Math.round(canvas.clientWidth * ratio));
  const height = Math.max(1, Math.round(canvas.clientHeight * ratio));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const vertex = shader(context, context.VERTEX_SHADER, `
    attribute vec2 position;
    varying vec2 outputUv;
    void main() {
      outputUv = (position + 1.0) * 0.5;
      gl_Position = vec4(position, 0.0, 1.0);
    }
  `);
  const fragment = shader(context, context.FRAGMENT_SHADER, `
    precision highp float;
    varying vec2 outputUv;
    uniform sampler2D sourceImage;
    uniform vec4 first;
    uniform vec4 second;
    void main() {
      float u = outputUv.x;
      float v = 1.0 - outputUv.y;
      float divisor = first.w * u + second.w * v + 1.0;
      vec2 sourceUv = vec2(
        (first.x * u + first.y * v + first.z) / divisor,
        (second.x * u + second.y * v + second.z) / divisor
      );
      gl_FragColor = texture2D(sourceImage, vec2(sourceUv.x, 1.0 - sourceUv.y));
    }
  `);
  const program = context.createProgram();
  if (!program) return;
  context.attachShader(program, vertex);
  context.attachShader(program, fragment);
  context.linkProgram(program);
  context.useProgram(program);
  const positions = context.createBuffer();
  context.bindBuffer(context.ARRAY_BUFFER, positions);
  context.bufferData(context.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), context.STATIC_DRAW);
  const position = context.getAttribLocation(program, "position");
  context.enableVertexAttribArray(position);
  context.vertexAttribPointer(position, 2, context.FLOAT, false, 0, 0);
  const texture = context.createTexture();
  context.bindTexture(context.TEXTURE_2D, texture);
  context.pixelStorei(context.UNPACK_FLIP_Y_WEBGL, 1);
  context.texParameteri(context.TEXTURE_2D, context.TEXTURE_WRAP_S, context.CLAMP_TO_EDGE);
  context.texParameteri(context.TEXTURE_2D, context.TEXTURE_WRAP_T, context.CLAMP_TO_EDGE);
  context.texParameteri(context.TEXTURE_2D, context.TEXTURE_MIN_FILTER, context.LINEAR);
  context.texParameteri(context.TEXTURE_2D, context.TEXTURE_MAG_FILTER, context.LINEAR);
  context.texImage2D(context.TEXTURE_2D, 0, context.RGBA, context.RGBA, context.UNSIGNED_BYTE, image);
  const projection = quadProjection(geometry);
  context.uniform4f(context.getUniformLocation(program, "first"), projection.a, projection.b, projection.c, projection.g);
  context.uniform4f(context.getUniformLocation(program, "second"), projection.d, projection.e, projection.f, projection.h);
  context.viewport(0, 0, width, height);
  context.clearColor(0, 0, 0, 0);
  context.clear(context.COLOR_BUFFER_BIT);
  context.drawArrays(context.TRIANGLES, 0, 6);
  context.deleteTexture(texture);
  context.deleteBuffer(positions);
  context.deleteProgram(program);
  context.deleteShader(vertex);
  context.deleteShader(fragment);
}

function drawSafely(canvas: HTMLCanvasElement, image: HTMLImageElement, geometry: PatchGeometry): void {
  try {
    draw(canvas, image, geometry);
  } catch {
    // The authoritative native preview remains available when WebGL compilation or context allocation fails.
  }
}

export function LiveRectifiedCanvas({ geometry, imageUrl, label, aspectRatio }: LiveRectifiedCanvasProps): React.JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const surfaceRef = useRef<HTMLDivElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const [surfaceSize, setSurfaceSize] = useState<SurfaceSize>({ width: 1, height: 1 });

  function resizeSurface(image = imageRef.current): void {
    const surface = surfaceRef.current;
    if (!surface || !image) return;
    const ratio = aspectRatio && Number.isFinite(aspectRatio) ? aspectRatio : automaticAspect(geometry, image);
    setSurfaceSize(fittedSize(surface, ratio));
  }

  useEffect(() => {
    const image = new Image();
    image.onload = () => {
      imageRef.current = image;
      resizeSurface(image);
      if (canvasRef.current) drawSafely(canvasRef.current, image, geometry);
    };
    image.src = imageUrl;
    return () => { image.onload = null; };
  }, [imageUrl, aspectRatio]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const image = imageRef.current;
    if (canvas && image) {
      resizeSurface(image);
      drawSafely(canvas, image, geometry);
    }
  }, [geometry, aspectRatio]);

  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    const resize = new ResizeObserver(() => {
      resizeSurface();
    });
    resize.observe(surface);
    return () => resize.disconnect();
  }, [geometry, aspectRatio]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const image = imageRef.current;
    if (canvas && image) drawSafely(canvas, image, geometry);
  }, [geometry, surfaceSize]);

  return <div ref={surfaceRef} className="rectified-preview-surface"><canvas ref={canvasRef} className="live-rectified-canvas" role="img" aria-label={label} style={surfaceSize} /></div>;
}
