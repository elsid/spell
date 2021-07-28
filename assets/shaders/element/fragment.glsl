#version 100

precision highp float;

varying vec2 uv;

uniform sampler2D Texture;
uniform vec4 color;

const float STEP = 0.04;
const float BORDER = 0.45;
const float MAX_DIST = 0.5;
const vec2 CENTER = vec2(0.5, 0.5);

void main() {
    float dist = distance(uv, CENTER);
    vec4 inner_color = mix(color, vec4(0.0, 0.0, 0.0, 1.0), smoothstep(BORDER - STEP, BORDER, dist));
    vec4 result_color = mix(inner_color, vec4(0.0, 0.0, 0.0, 0.0), smoothstep(MAX_DIST - STEP, MAX_DIST, dist));
    gl_FragColor = result_color * texture2D(Texture, uv);
}
