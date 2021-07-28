#version 100

precision highp float;

varying vec2 uv;

uniform sampler2D Texture;
uniform float scale;
uniform vec4 color;

const float MAX_DIST = 0.25;
const vec2 CENTER = vec2(0.5, 0.5);
const float FADE_FACTOR = 0.75;
const float STEP = 0.01;

void main() {
    float dist = distance(uv, CENTER);
    float dist_factor = dist / MAX_DIST;
    float intensity = (dist_factor - FADE_FACTOR) / (1.0 - FADE_FACTOR);
    float factor = mix(intensity, 0.0, smoothstep(MAX_DIST - 0.5 * STEP / scale, MAX_DIST + 0.5 * STEP / scale, dist));
    gl_FragColor = mix(vec4(0.0, 0.0, 0.0, 0.0), color, factor) * texture2D(Texture, uv);
}
