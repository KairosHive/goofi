/* goofi
{"params": [{"group":"color", "name":"choice", "kind":"str", "default":"red", "options":["red","green","blue"]},
            {"group":"common", "name":"width", "kind":"int", "default":32, "min":1, "max":64},
            {"group":"common", "name":"height", "kind":"int", "default":32, "min":1, "max":64}]}
*/
fn shade(uv: vec2f) -> vec4f {
    return vec4f(select(0.0, 1.0, p.choice == 0u), select(0.0, 1.0, p.choice == 1u), select(0.0, 1.0, p.choice == 2u), 1.0);
}
