goofi is a live signal-processing patch: a graph of nodes running now, in a window a human has
open beside you. Your edits reach their screen at once; theirs reach your next read. Work in small
steps, `goofi nodes inspect` between them. Every write answers with what it did: read the reply
instead of re-reading. Never guess a name: `goofi library list` indexes the node types,
`goofi library get <type>` reads one in full; panel types and viewer kinds are closed sets, and a
guess is refused with the whole set.

## Driving

`goofi` is on PATH, pointed at THIS server over local TCP: a sandbox that blocks the network blocks
every op. One op is one line, `goofi <op> [--arg value …]`. `goofi op list` answers every op with
its arguments and its kind; `goofi help <group>` lists a group; `<op> --help` explains one (read
that, not `op list --doc`, the whole manual); `--json` answers raw JSON for `jq`. Text is compact
on a pipe and laid out for a person at a terminal; `GOOFI_STYLE=compact|human` overrides. `goofi guide`
names the guides and `goofi guide get <name>` reads one: read the skill matching your task before
building. Your ops land on the one undo history the human shares: `undo` takes back the last step,
whoever made it. Several lines on stdin are ONE batch and ONE undo step, landing whole or not at
all:

    goofi - <<'EOF'
    node add LFO --name osc
    node add Buffer --name buf
    link add osc/out buf/data
    EOF

A NAME addresses a node in every op, so a later line wires what an earlier one built. Over MCP
(`/mcp`, served only under `--mcp`) the one tool `goofi_exec` runs the same lines, a list as one
batch; the NPY pipe and `--json` are the shell's alone.

## Seeing

`goofi nodes inspect` draws one scope as mermaid: no argument is the root, a sub-patch narrows.
`goofi session status` lists every standing error with its age (0.2 s may clear itself, 30 s will
not) and says where the patch is saved. `goofi node state <node>` is the cheap peek: params,
whether each output emits, the error; `--no-params`/`--no-error` drop a section, `--slot` narrows
to one output. `nothing emitted yet` in place of a rate is the first thing to look for.
`goofi node snapshot <node>/<slot>` answers an ARRAY's latest frame as shape and range (silence vs
signal), STRING and TABLE as their value; a facade or boundary port resolves to the stream behind
it; `--raw` answers NPY on stdout:

    goofi node snapshot buf/out --raw \
      | python3 -c "import numpy,sys; print(numpy.load(sys.stdin.buffer).mean())"

The first ask on a never-watched slot opens its feed and answers null; ask again after the next
emit. A monitor is a loop over it. `goofi layout inspect` names the tabs and panel ids the layout
ops take; `goofi variable list` says what an expression can read. Design in the open tab: a new
tab hides your work behind a click, so add one only when asked. Your terminal is a panel on that
layout: mind the one the human watches you through.

## Building

    goofi node add LFO
    → {"name": "lfo0", "uid": "000000000001", "input_slots": {},
       "output_slots": {"out": "ARRAY"}, "params": {…}}

    goofi link add lfo0/out buffer0/data   → {"from": …, "to": …, "dtype": "ARRAY"}

    goofi node param edit lfo0 lfo/frequency --value 7.5
    → {"value": 7.5, "error": null}

Give no `--pos`: each node lands on the grid right of the last, and `nodes arrange` (`machine
arrange` for a state machine) lays a scope out by dataflow once the wires are in. A NAME is unique
across the patch, minted if you give none, yours with `--name`; the uid beside it keys records of
your own. `node param edit` answers the param AS STORED, coerced to its declared type; min/max are
the editor's range, not a clamp. `--expression "nd('other').out.sfreq"` (or `vars.x`, or `t`)
binds instead of a literal; a bound param reads back `expr: <source> → <value> (on)` and
`--expression` takes the source half. Bare `nd('x')` is the single output, `.out.<slot>` one of
several, `nd('x').params.<group>.<param>` a param re-read on edit, `me` this node. `node edit` is
the node's own record: rename, move, viewer write, any mix in one call and one undo. `link add`
refuses a dtype mismatch naming both ends, a wrong slot naming the slots that exist, and an end
naming no node: a reply means the wire is there. Take names from a read, never from memory.

## Custom nodes

The patch's own library is `nodes_signal/` in the workspace: one file is one type, named by
CamelCasing its stem (`scale.py` → `Scale`), overriding a shipped type of the same name.

    # nodes_signal/scale.py
    import goofi

    class Scale(goofi.Node):
        """Multiply every value by a factor."""

        INPUTS = {"input": goofi.DataType.ARRAY}
        OUTPUTS = {"out": goofi.DataType.ARRAY}
        PARAMS = {"scale": {"factor": goofi.NumParam(2.0, 0.0, 10.0)}}

        def process(self, input):
            if input is None:
                return None
            return {"out": (input.data * self.params.scale.factor, input.meta)}

    goofi library refresh → {"added": ["Scale"], "changed": [], "removed": []}

The docstring is the doc; its FIRST LINE is the catalog nutshell, under 80 characters. `INPUTS`,
`OUTPUTS`, `PARAMS` and `PRODUCER = True` (a source pacing itself) are each optional. An input
`process()` reads unconditionally is `goofi.InputSlot(goofi.DataType.ARRAY, required=True)`: the
engine then refuses the tick instead of calling you with `None`. Edit and refresh: the type comes
back under `changed` and every live instance restarts onto the new code (`setup()` runs again: a
buffer empties, a device reopens). Missing imports register the node unavailable and name the
module; a raise in `process()` is that node's error, not a crash. `goofi library get <type>
--source` is a shipped node to copy, in any of three languages: a `.rs` beside it is a Rust node,
built on refresh where `cargo` exists and run in a child of goofi's own binary; a `.wgsl` in
`nodes_graphics/` is a GPU shader (read `graphics:Life` for state kept between ticks). Graphics also
take Rust (`goofi_graphics_sdk`) and Python: a file starting `# goofi: graphics`, one
`goofi.DataType.TEXTURE` output named `out`, returning `goofi.Texture(pixels)` for RGB/RGBA uint8
or float32 (`graphics:Camera` is one). `signal:GraphicsIn` reads GPU pixels back for the CPU.

## Workspace

Your cwd is the patch workspace: it rides inside the `.gfi` on save, minus what `.goofiignore`
lists, and that list also decides whether the workspace counts as changed. `AGENTS.md` there is
yours to extend as you learn what the patch is for. `goofi session status` says where it is.

## Handle with care

`session load` and `session new` replace the patch you work inside and clear the undo history.
`session save` and `dir list` are the human's file browser; `layout viewpoint edit` is the camera
of a client with a screen; the agent ops can stop the shell you run in. All answer to you, all act
on what the human is looking at: ask before using one.
