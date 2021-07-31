#version 100

precision highp float;

varying vec2 uv;

uniform sampler2D Texture;
uniform float scale;
uniform vec4 base_color;
uniform vec4 border_color;

const float INNER_BORDER = 0.1;
const float OUTER_BORDER = 0.24;
const float BORDER_SIZE = 0.01;
const vec2 CENTER = vec2(0.5, 0.5);

void main() {
    float dist = distance(uv, CENTER);
    float border_size = BORDER_SIZE / scale;
    float step_size = border_size * 0.1;
    if (dist < INNER_BORDER + 0.5 * border_size) {
        gl_FragColor = mix(base_color, border_color, smoothstep(INNER_BORDER, INNER_BORDER + step_size, dist)) * texture2D(Texture, uv);
    } else if (dist < INNER_BORDER + 1.5 * border_size) {
        gl_FragColor = mix(border_color, base_color, smoothstep(INNER_BORDER + border_size, INNER_BORDER + border_size + step_size, dist)) * texture2D(Texture, uv);
    } else if (dist < OUTER_BORDER + 0.5 * border_size) {
        gl_FragColor = mix(base_color, border_color, smoothstep(OUTER_BORDER, OUTER_BORDER + step_size, dist)) * texture2D(Texture, uv);
    } else {
        gl_FragColor = mix(border_color, vec4(0.0, 0.0, 0.0, 0.0), smoothstep(OUTER_BORDER + border_size - step_size, OUTER_BORDER + border_size, dist)) * texture2D(Texture, uv);
    }
}
