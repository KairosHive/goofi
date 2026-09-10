/* goofi
{"inputs":[{"name":"geometry","kind":"ARRAY"}],"params":[
{"group":"common","name":"width","kind":"int","default":64,"min":0,"max":4096},
{"group":"common","name":"height","kind":"int","default":64,"min":0,"max":4096}]}
*/
fn shade(uv: vec2f) -> vec4f { return goofi_geo_field(geometry, uv); }
