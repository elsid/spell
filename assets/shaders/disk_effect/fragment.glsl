#version 100

precision highp float;

varying vec2 uv;

uniform sampler2D Texture;
uniform float time;
uniform float scale;
uniform vec4 color;

const float MAX_DIST = 0.25;
const vec2 CENTER = vec2(0.5, 0.5);
const float STEP = 0.01;

void main() {
    float scaled_time = 2.0 * time / log(max(scale, 3.0));
    vec2 local_uv = vec2(
        uv.x * 0.95 + sin(0.5 * uv.x + 0.2 * uv.y + 0.5 * scaled_time) * 0.05,
        uv.y * 0.95 + sin(0.3 * uv.x + 0.4 * uv.y + 0.5 * scaled_time) * 0.05
    );
    float dist = distance(uv, CENTER);
    float dist_factor = dist / MAX_DIST;
    float intensity = mix(sin(2.0 * scaled_time + (10.0 * local_uv.x + local_uv.y) * scale) * 0.05, 0.05, dist_factor) + 0.9;
    float factor = mix(intensity, 0.0, smoothstep(MAX_DIST - 0.5 * STEP / scale, MAX_DIST + 0.5 * STEP / scale, dist));
    gl_FragColor = mix(vec4(0.0, 0.0, 0.0, 0.0), color, factor) * texture2D(Texture, local_uv);
}
