import React, { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IPC_PROTOCOL_VERSION,
  type CompiledMapView,
  type GpuTiledPreviewPublication,
  type IntermediateAtlasProjection,
} from "@hot-trimmer/ipc-contracts";
import {
  gpuTiledPreviewMapMatches,
  gpuTiledPreviewPayloadBytes,
  isValidGpuTiledPreviewPayload,
} from "./source-frame-preview-controller";

export const materialPreviewMapViews = [
  "baseColor",
  "normal",
  "height",
  "roughness",
  "metallic",
  "ambientOcclusion",
] as const satisfies readonly CompiledMapView[];

function publicationForView(
  artifact: IntermediateAtlasProjection,
  view: CompiledMapView,
): GpuTiledPreviewPublication | undefined {
  return artifact.tileManifests?.[view]
    ?? (artifact.tileManifest && gpuTiledPreviewMapMatches(artifact.tileManifest.manifest.map, view)
      ? artifact.tileManifest
      : undefined);
}

export function materialPreviewReady(
  artifact: IntermediateAtlasProjection | null | undefined,
  revision?: number,
): boolean {
  return !!artifact
    && (revision === undefined || artifact.documentRevision === revision)
    && materialPreviewMapViews.every((view) => !!publicationForView(artifact, view));
}

export const materialPreviewFragmentShader = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 out_color;
uniform sampler2D u_base_color;
uniform sampler2D u_normal;
uniform sampler2D u_height;
uniform sampler2D u_roughness;
uniform sampler2D u_metallic;
uniform sampler2D u_ao;
uniform vec3 u_light;
uniform vec2 u_texel;

vec3 srgb_to_linear(vec3 value) {
  vec3 low = value / 12.92;
  vec3 high = pow((value + 0.055) / 1.055, vec3(2.4));
  return mix(low, high, step(vec3(0.04045), value));
}

vec3 linear_to_srgb(vec3 value) {
  vec3 low = value * 12.92;
  vec3 high = 1.055 * pow(max(value, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
  return mix(low, high, step(vec3(0.0031308), value));
}

void main() {
  vec2 uv = vec2(v_uv.x, 1.0 - v_uv.y);
  vec4 encoded_base = texture(u_base_color, uv);
  vec3 base = srgb_to_linear(encoded_base.rgb);
  vec3 normal = normalize(texture(u_normal, uv).rgb * 2.0 - 1.0);
  float roughness = clamp(texture(u_roughness, uv).r, 0.04, 1.0);
  float metallic = clamp(texture(u_metallic, uv).r, 0.0, 1.0);
  float ao = clamp(texture(u_ao, uv).r, 0.0, 1.0);
  vec3 light = normalize(u_light);
  vec3 view = vec3(0.0, 0.0, 1.0);
  vec3 halfway = normalize(light + view);
  float n_dot_l = max(dot(normal, light), 0.0);
  float wrapped_light = clamp((dot(normal, light) + 0.18) / 1.18, 0.0, 1.0);
  float specular_power = mix(160.0, 4.0, roughness * roughness);
  vec3 f0 = mix(vec3(0.04), base, metallic);
  vec3 diffuse = base * (1.0 - metallic) * wrapped_light;
  vec3 specular = f0 * pow(max(dot(normal, halfway), 0.0), specular_power)
    * mix(1.0, 0.16, roughness) * n_dot_l;

  // A short height look-ahead adds restrained cavity shadowing at grazing
  // angles. Normal remains the primary surface response; this only makes a
  // recessed bevel readable under the preview light.
  float height_here = texture(u_height, uv).r;
  float height_toward_light = texture(u_height, uv + light.xy * u_texel * 3.0).r;
  float cavity = clamp(1.0 + (height_here - height_toward_light) * 2.5, 0.48, 1.0);
  vec3 ambient = base * (0.075 + 0.16 * ao);
  vec3 color = ambient + (diffuse + specular) * ao * cavity * 1.15;
  out_color = vec4(linear_to_srgb(color), encoded_base.a);
}`;

const vertexShader = `#version 300 es
out vec2 v_uv;
void main() {
  vec2 point = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  v_uv = point;
  gl_Position = vec4(point * 2.0 - 1.0, 0.0, 1.0);
}`;

function compileShader(gl: WebGL2RenderingContext, kind: number, source: string): WebGLShader {
  const shader = gl.createShader(kind);
  if (!shader) throw new Error("The material preview could not allocate a GPU shader.");
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const diagnostic = gl.getShaderInfoLog(shader) ?? "unknown shader error";
    gl.deleteShader(shader);
    throw new Error(`The material preview shader failed: ${diagnostic}`);
  }
  return shader;
}

function createProgram(gl: WebGL2RenderingContext): WebGLProgram {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, vertexShader);
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, materialPreviewFragmentShader);
  const program = gl.createProgram();
  if (!program) throw new Error("The material preview could not allocate a GPU program.");
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  gl.deleteShader(vertex);
  gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const diagnostic = gl.getProgramInfoLog(program) ?? "unknown link error";
    gl.deleteProgram(program);
    throw new Error(`The material preview program failed: ${diagnostic}`);
  }
  return program;
}

function tightRgbaBytes(payload: Uint8Array, publication: GpuTiledPreviewPublication): Uint8Array {
  const { width, height, rowStride } = publication.manifest;
  const rowBytes = width * 4;
  if (rowStride === rowBytes) return payload;
  const tight = new Uint8Array(rowBytes * height);
  for (let row = 0; row < height; row += 1) {
    tight.set(payload.subarray(row * rowStride, row * rowStride + rowBytes), row * rowBytes);
  }
  return tight;
}

async function loadTextureBytes(publication: GpuTiledPreviewPublication): Promise<Uint8Array> {
  const { generation, opaqueHandle } = publication.manifest;
  const payload = gpuTiledPreviewPayloadBytes(await invoke<Uint8Array>(
    "get_gpu_tiled_preview_payload",
    { request: { protocolVersion: IPC_PROTOCOL_VERSION, generation, opaqueHandle } },
  ));
  if (!isValidGpuTiledPreviewPayload(publication, payload.byteLength)) {
    throw new Error(`The ${publication.manifest.map} preview payload is incomplete.`);
  }
  // Active-generation payloads stay cache-owned so switching from Material to
  // an individual map does not force another render. The next native generation
  // retires them as one bounded set.
  return tightRgbaBytes(payload, publication);
}

export function MaterialPreviewCanvas(props: {
  artifact: IntermediateAtlasProjection;
  onPaint: (dimensions: { width: number; height: number; generation?: number }) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const drawRef = useRef<((light: readonly [number, number, number]) => void) | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "failed">("loading");
  const [problem, setProblem] = useState<string | null>(null);
  const publications = useMemo(() => materialPreviewMapViews.map((view) => {
    const publication = publicationForView(props.artifact, view);
    if (!publication) throw new Error(`${view} has not been rendered for the current material preview.`);
    return publication;
  }), [props.artifact]);
  const generationKey = publications.map(({ manifest }) => `${manifest.generation}:${manifest.opaqueHandle}`).join("|");

  useEffect(() => {
    let disposed = false;
    let resources: {
      gl: WebGL2RenderingContext;
      program: WebGLProgram;
      textures: WebGLTexture[];
      vertexArray: WebGLVertexArrayObject;
    } | null = null;
    const canvas = canvasRef.current;
    if (!canvas) return;
    setStatus("loading");
    setProblem(null);
    void (async () => {
      const gl = canvas.getContext("webgl2", { alpha: true, antialias: true });
      if (!gl) throw new Error("WebGL 2 is unavailable, so the lit material preview cannot run.");
      const maximum = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
      if (props.artifact.width > maximum || props.artifact.height > maximum) {
        throw new Error(`This GPU supports material-preview textures up to ${maximum}px.`);
      }
      const program = createProgram(gl);
      const vertexArray = gl.createVertexArray();
      if (!vertexArray) throw new Error("The material preview could not allocate a vertex array.");
      gl.bindVertexArray(vertexArray);
      const payloads = await Promise.all(publications.map(loadTextureBytes));
      if (disposed) { gl.deleteVertexArray(vertexArray); gl.deleteProgram(program); return; }
      gl.useProgram(program);
      const textures = payloads.map((pixels, index) => {
        const texture = gl.createTexture();
        if (!texture) throw new Error("The material preview could not allocate a map texture.");
        const manifest = publications[index]!.manifest;
        gl.activeTexture(gl.TEXTURE0 + index);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, manifest.width, manifest.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        const uniform = gl.getUniformLocation(program, [
          "u_base_color", "u_normal", "u_height", "u_roughness", "u_metallic", "u_ao",
        ][index]!);
        gl.uniform1i(uniform, index);
        return texture;
      });
      resources = { gl, program, textures, vertexArray };
      const lightUniform = gl.getUniformLocation(program, "u_light");
      const texelUniform = gl.getUniformLocation(program, "u_texel");
      gl.uniform2f(texelUniform, 1 / props.artifact.width, 1 / props.artifact.height);
      gl.viewport(0, 0, canvas.width, canvas.height);
      drawRef.current = (light) => {
        gl.uniform3f(lightUniform, light[0], light[1], light[2]);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
      };
      drawRef.current([-0.42, 0.48, 0.76]);
      setStatus("ready");
      props.onPaint({
        width: props.artifact.width,
        height: props.artifact.height,
        generation: publications[0]!.manifest.generation,
      });
    })().catch((reason) => {
      if (!disposed) {
        setStatus("failed");
        setProblem(reason instanceof Error ? reason.message : String(reason));
      }
    });
    return () => {
      disposed = true;
      drawRef.current = null;
      if (resources) {
        resources.textures.forEach((texture) => resources?.gl.deleteTexture(texture));
        resources.gl.deleteVertexArray(resources.vertexArray);
        resources.gl.deleteProgram(resources.program);
      }
    };
  }, [generationKey, props.artifact.width, props.artifact.height]);

  return <div className="material-preview-surface">
    <canvas
      ref={canvasRef}
      className="material-preview-canvas"
      width={props.artifact.width}
      height={props.artifact.height}
      aria-label="Lit material preview"
      onPointerMove={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        const x = ((event.clientX - rect.left) / Math.max(rect.width, 1) - 0.5) * 1.7;
        const y = (0.5 - (event.clientY - rect.top) / Math.max(rect.height, 1)) * 1.7;
        drawRef.current?.([x, y, 0.72]);
      }}
      onPointerLeave={() => drawRef.current?.([-0.42, 0.48, 0.76])}
    />
    <div className={`material-preview-state ${status}`}>
      {status === "loading" ? "Loading the complete material map set…" : status === "failed" ? problem : "Real-time material · move the pointer to relight"}
    </div>
  </div>;
}
