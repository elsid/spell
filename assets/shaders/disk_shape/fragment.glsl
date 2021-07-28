#version 100

precision highp float;

varying vec2 uv;

uniform sampler2D Texture;
uniform float scale;
uniform vec4 color;

const float MAX_DIST = 0.25;
const vec2 CENTER = vec2(0.5, 0.5);
const float STEP = 0.01;

void main() {
    float factor = smoothstep(MAX_DIST - 0.5 * STEP / scale, MAX_DIST + 0.5 * STEP / scale, distance(uv, CENTER));
    gl_FragColor = mix(color, vec4(0.0, 0.0, 0.0, 0.0), factor) * texture2D(Texture, uv);
}
