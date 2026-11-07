// The vault door: Stahl's icon, lit as spun steel. The icon's five rings and its seam down the
// middle are all here, at the same radii and in the same colours, so the door can split along it.
"use strict";

const VERTEX = `#version 300 es
in vec2 pos;
void main() { gl_Position = vec4(pos, 0.0, 1.0); }`;

const FRAGMENT = `#version 300 es
precision highp float;
uniform vec2 res;      // canvas size, in pixels
uniform vec2 center;   // door centre, in pixels from the top left
uniform float radius;  // door radius, in pixels
uniform float split;   // how far each half has slid, in door radii
uniform float spin;    // tumbler rotation, in radians
uniform vec2 light;    // light position, in door radii
uniform float gain;    // highlight strength, lowered as the door fills the view
out vec4 color;

const int RINGS = 5;
const float EDGE[RINGS + 1] = float[](0.0, 0.1875, 0.3906, 0.6094, 0.7969, 0.9922);
// Lit (left) and shaded (right) halves of assets/icons/icon.svg, inner ring first.
const vec3 LIT[RINGS] = vec3[](
  vec3(0.824, 0.847, 0.863), vec3(0.647, 0.698, 0.729), vec3(0.294, 0.392, 0.459),
  vec3(0.235, 0.322, 0.376), vec3(0.149, 0.200, 0.231));
const vec3 DIM[RINGS] = vec3[](
  vec3(0.780, 0.816, 0.831), vec3(0.565, 0.631, 0.667), vec3(0.125, 0.255, 0.329),
  vec3(0.110, 0.208, 0.278), vec3(0.067, 0.129, 0.173));
const float DIRECTION[RINGS] = float[](0.0, -1.0, 1.0, -0.6, 0.25);

float hash(float x) { return fract(sin(x * 127.1) * 43758.5453); }
float hash2(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453); }
float noise(float x) {
  float i = floor(x), f = fract(x);
  return mix(hash(i), hash(i + 1.0), f * f * (3.0 - 2.0 * f));
}

// One half of the door, with premultiplied alpha.
vec4 half_door(vec2 world, float side) {
  vec2 p = world - vec2(side * split, 0.0);
  float px = 1.0 / radius;
  float inside = smoothstep(-px, px, p.x * side - px * 0.75);
  float r = length(p);
  float outer = 1.0 - smoothstep(EDGE[RINGS] - px, EDGE[RINGS] + px, r);
  float alpha = inside * outer;
  if (alpha <= 0.0) return vec4(0.0);

  int ring = 0;
  for (int i = 0; i < RINGS; i++) if (r >= EDGE[i + 1]) ring = i + 1;
  ring = min(ring, RINGS - 1);
  float a = EDGE[ring], b = EDGE[ring + 1];
  float seed = float(ring) * 17.3;
  float angle = atan(p.y, p.x);
  float turned = angle + spin * DIRECTION[ring];

  vec2 radial = r > 0.0001 ? p / r : vec2(1.0, 0.0);
  vec2 tangent = vec2(-radial.y, radial.x);

  // Lathe marks: finer than a pixel is averaged away so zooming in never shimmers.
  float detail = clamp(radius * 0.9, 120.0, 1400.0);
  float lines = noise(r * detail + seed) * 0.6 + noise(r * detail * 0.31 + seed) * 0.4;
  float blotch = noise(turned * 3.0 + seed) * 0.5 + noise(turned * 11.0 + seed * 2.0) * 0.5;

  // The rim of each ring catches light like a chamfer.
  float width = 0.016;
  float inDist = r - a, outDist = b - r;
  float chamfer = smoothstep(width, 0.0, outDist) - smoothstep(width, 0.0, inDist);
  vec3 normal = normalize(vec3(radial * chamfer * 0.9, 1.0));

  vec3 toLight = normalize(vec3(light - p, 1.3));
  vec3 halfway = normalize(toLight + vec3(0.0, 0.0, 1.0));

  // Anisotropic highlight: brushed circles flare along the radius, in opposing fans.
  vec2 brushed = normalize(tangent + radial * (lines - 0.5) * 0.09);
  float along = dot(brushed, halfway.xy);
  float fan = exp(-along * along * 150.0) * 0.9 + exp(-along * along * 14.0) * 0.28;
  float diffuse = max(dot(normal, toLight), 0.0);

  vec3 lit = LIT[ring], dim = DIM[ring];
  vec3 base = side < 0.0 ? lit : dim;
  base *= 0.92 + (lines - 0.5) * 0.22 + (blotch - 0.5) * 0.06;

  float strength = mix(0.72, 0.5, float(ring) / 4.0) * gain;
  vec3 steel = vec3(0.84, 0.91, 0.97);
  vec3 c = base * (0.5 + 0.55 * diffuse) + steel * fan * strength * diffuse * (side < 0.0 ? 1.0 : 0.72);
  c += steel * max(chamfer * dot(radial, normalize(light - p)), 0.0) * 0.5 * gain;

  // The groove between rings.
  float groove = smoothstep(0.0, 0.007, min(inDist, outDist));
  c *= mix(0.16, 1.0, groove);

  // The dial: graduations cut into the fourth ring, turning with it.
  if (ring == 3) {
    float count = 72.0;
    float cell = turned / 6.2831853 * count;
    float tick = abs(fract(cell) - 0.5) * 2.0;
    bool major = mod(floor(cell), 6.0) < 0.5;
    float reach = major ? 0.05 : 0.026;
    float cut = smoothstep(0.9, 0.96, tick) * step(b - 0.018 - reach, r) * step(r, b - 0.018);
    c = mix(c, c * 0.34, cut);
  }

  // Bolts around the third ring, each a small lit dome.
  if (ring == 2) {
    float count = 12.0;
    float step_ = 6.2831853 / count;
    float cellAngle = mod(turned + step_ * 0.5, step_) - step_ * 0.5;
    float mid = (a + b) * 0.5;
    vec2 local = vec2(r * cos(cellAngle) - mid, r * sin(cellAngle));
    float d = length(local) / 0.024;
    if (d < 1.0) {
      float dome = sqrt(1.0 - d * d);
      vec3 boltNormal = normalize(vec3(local / 0.024, dome + 0.2));
      float boltLight = pow(max(dot(boltNormal, halfway), 0.0), 26.0);
      c = base * 0.5 + base * 0.7 * max(dot(boltNormal, toLight), 0.0) + steel * boltLight * 0.9;
    }
    c *= mix(0.55, 1.0, smoothstep(0.92, 1.12, d));
  }

  // The cut faces of the two halves.
  float faceDist = abs(p.x);
  c += steel * smoothstep(0.012, 0.0, faceDist) * 0.14 * smoothstep(0.0, 0.02, split);
  c *= mix(0.25, 1.0, smoothstep(0.0, 0.0035, faceDist));

  return vec4(c * alpha, alpha);
}

void main() {
  vec2 frag = vec2(gl_FragCoord.x, res.y - gl_FragCoord.y);
  vec2 world = (frag - center) / radius;
  vec4 left = half_door(world, -1.0);
  vec4 right = half_door(world, 1.0);
  vec4 door = left + right * (1.0 - left.a);

  // A shadow on the wall behind the closed door.
  float r = length(world);
  float closed = 1.0 - smoothstep(0.0, 0.12, split);
  float shadow = (1.0 - smoothstep(0.95, 1.5, r)) * 0.55 * closed;

  float a = door.a + shadow * (1.0 - door.a);
  vec3 rgb = door.rgb;
  rgb += (hash2(gl_FragCoord.xy) - 0.5) / 180.0 * door.a;
  color = vec4(rgb, a);
}`;

// Returns { draw(state) } or null, so the page can fall back on the flat icon.
window.createDoor = function createDoor(canvas) {
  const gl = canvas.getContext("webgl2", { alpha: true, premultipliedAlpha: true, antialias: false, powerPreference: "low-power" });
  if (!gl) return null;

  function compile(type, source) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      console.warn(gl.getShaderInfoLog(shader));
      return null;
    }
    return shader;
  }

  const vs = compile(gl.VERTEX_SHADER, VERTEX);
  const fs = compile(gl.FRAGMENT_SHADER, FRAGMENT);
  if (!vs || !fs) return null;
  const program = gl.createProgram();
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
  gl.useProgram(program);

  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const at = gl.getAttribLocation(program, "pos");
  gl.enableVertexAttribArray(at);
  gl.vertexAttribPointer(at, 2, gl.FLOAT, false, 0, 0);

  const uniform = (name) => gl.getUniformLocation(program, name);
  const u = {
    res: uniform("res"),
    center: uniform("center"),
    radius: uniform("radius"),
    split: uniform("split"),
    spin: uniform("spin"),
    light: uniform("light"),
    gain: uniform("gain"),
  };
  gl.clearColor(0, 0, 0, 0);

  return {
    draw({ width, height, scale, x, y, radius, split, spin, lightX, lightY, gain }) {
      const w = Math.round(width * scale);
      const h = Math.round(height * scale);
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
        gl.viewport(0, 0, w, h);
      }
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.uniform2f(u.res, w, h);
      gl.uniform2f(u.center, x * scale, y * scale);
      gl.uniform1f(u.radius, radius * scale);
      gl.uniform1f(u.split, split);
      gl.uniform1f(u.spin, spin);
      gl.uniform2f(u.light, lightX, lightY);
      gl.uniform1f(u.gain, gain);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    },
  };
};
