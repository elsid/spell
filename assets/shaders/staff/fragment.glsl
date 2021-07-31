#version 100

precision highp float;

varying vec2 uv;

uniform sampler2D Texture;
uniform float scale;

const float SIZE_Y = 0.025;
const float BORDER_SIZE_Y = (1.0 - SIZE_Y) * 0.5;
const float SIZE_X = 1.25;
const float BORDER_SIZE_X = (1.0 - SIZE_X) * 0.5;
const float STEP = SIZE_Y * 0.1;
const vec4 COLOR = vec4(0.3, 0.2, 0.04, 1.0);
const vec4 BACKGROUND = vec4(0.0, 0.0, 0.0, 0.0);

void main() {
    float step = STEP / scale;
    float x = abs(0.5 - uv.x) * SIZE_Y * 0.2;
    float border_size_y = BORDER_SIZE_Y + x;
    vec4 color_y;
    if (uv.y < 0.5) {
        color_y = mix(BACKGROUND, COLOR, smoothstep(border_size_y, border_size_y + step, uv.y));
    } else {
        color_y = mix(COLOR, BACKGROUND, smoothstep(1.0 - border_size_y - step, 1.0 - border_size_y, uv.y));
    }
    vec4 color_x;
    if (uv.x < 0.5) {
        color_x = mix(BACKGROUND, color_y, smoothstep(BORDER_SIZE_X, BORDER_SIZE_X + step, uv.x));
    } else {
        color_x = mix(color_y, BACKGROUND, smoothstep(1.0 - BORDER_SIZE_X - step, 1.0 - BORDER_SIZE_X, uv.x));
    }
    gl_FragColor = color_x * texture2D(Texture, uv);
}
