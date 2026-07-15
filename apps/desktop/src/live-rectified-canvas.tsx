import { useEffect, useRef } from "react";
import type { PatchGeometry } from "@hot-trimmer/ipc-contracts";
import { quadProjection } from "./patch-authoring";

interface LiveRectifiedCanvasProps {
  geometry: PatchGeometry;
  imageUrl: string;
  label: string;
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

export function LiveRectifiedCanvas({ geometry, imageUrl, label }: LiveRectifiedCanvasProps): React.JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);

  useEffect(() => {
    const image = new Image();
    image.onload = () => {
      imageRef.current = image;
      if (canvasRef.current) draw(canvasRef.current, image, geometry);
    };
    image.src = imageUrl;
    return () => { image.onload = null; };
  }, [imageUrl]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const image = imageRef.current;
    if (canvas && image) draw(canvas, image, geometry);
  }, [geometry]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const resize = new ResizeObserver(() => {
      if (imageRef.current) draw(canvas, imageRef.current, geometry);
    });
    resize.observe(canvas);
    return () => resize.disconnect();
  }, [geometry]);

  return <canvas ref={canvasRef} className="live-rectified-canvas" role="img" aria-label={label} />;
}
