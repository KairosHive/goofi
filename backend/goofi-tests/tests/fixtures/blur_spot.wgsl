/* goofi
{"doc":"A red spot with hidden blue outside it."}
*/
fn shade(uv: vec2f) -> vec4f {
    let inside = length((uv - vec2f(0.65, 0.5)) * vec2f(2.0, 1.0)) < 0.045;
    return select(vec4f(0.0, 0.0, 1.0, 0.0), vec4f(1.0, 0.0, 0.0, 1.0), inside);
}
