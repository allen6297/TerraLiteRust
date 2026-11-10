# Terralite

Terralite is Kalob's game. This Rust workspace builds it. Minecraft 1.8 is only the base for world data: Anvil region files (`.mca`) and numeric block ids plus metadata, from before the 1.13 flattening. This is not a vanilla client, and it does not aim at protocol 47.

The server and the client are two processes. The server loads the world and listens on `127.0.0.1`. The client connects, draws the blocks the server sends, and asks the server to break or place. The frames are private to these two programs: version 1, not protocol 47.

## Crates

- `rebuild-world` — shared chunk type and a finite cluster: small, medium, large, or mega. The synthetic world is small, 31×31 chunks and 496 blocks on a side. Reads 1.8 Anvil chunks (block id byte, metadata nibble, optional Add nibble). No block-state palette, no biomes, and no terrain generation. The private localhost frames live in this crate.
- `rebuild-server` — loads that cluster, listens on localhost, and applies break and place.
- `rebuild-client` — connects to that server, meshes the blocks it receives, and opens a window so you can walk from one chunk into the next.

## Build and test

Rust 1.87 or newer. This repo pins 1.99.1 in `rust-toolchain.toml`.

```bash
cargo test
cargo build --workspace
```

`cargo test` uses a synthetic chunk generated in code. It does not need a saved world.

## Server

Start the server, wait until it prints `listening on 127.0.0.1:17610`, then start the client.

```bash
cargo run -p rebuild-server
cargo run -p rebuild-client
```

```bash
cargo run -p rebuild-server -- --region path/to/r.0.0.mca --x 0 --z 0
cargo run -p rebuild-server -- --port 17610
```

With no `--region`, the server loads the built-in small world around chunk `(0, 0)`, lays `C:\CODE\TerraLite\saves\blocks.json` over it when that file exists, and prints a line like:

```text
961 chunks around (0, 0): 2326 non-air blocks [synthetic]
```

A save adds its cells to that count. The server then listens on `127.0.0.1`, default port `17610`. Both programs take `--port`. One client at a time. When that client disconnects, the next one gets the world as it stands. The server is the authority for break and place, and it writes the synthetic save. A region launch does not read or write that file.

That authority path runs mod hooks. `Hooks::register_before_break` adds a hook that sees the cell and may cancel the break. `Hooks::register_after_place` adds a hook that runs only after a place changes the cell. The built-in `TestHook::cancels` records both and cancels breaks of one block id.

Rosegold scripts register those same hooks. On startup the server loads every `*.rg` file in `C:\CODE\TerraLite\mods`, in path order. A missing folder, or an empty one, leaves the list empty. `mods\keep-stone.rg` registers a before-break named `keep-stone` that cancels breaks of `stone`. `mods\log-place.rg` registers an after-place named `log-place` that prints `rosegold: placed <id> at x, y, z` when a place changes a cell. A refused place prints nothing. Delete or rename a file to turn that script off. A bad script stops the server before it listens. The server also reads the data pack at `C:\CODE\TerraLite\datapacks\terralite`. `pack.mcmeta` marks that root, and each `data/terralite/blocks/<id>.json` sets hardness and, for an ore, replaces. Those values win over the same fields in per-block models under `resourcepacks/terralite/assets/terralite/models/block/`. A missing or bad data pack stops the server before it listens.

`import ModAPI` resolves to `C:\CODE\RoseGoldRust\builtin\std\TerraLite\ModAPI.rg`. Block, item, and entity helpers are `import Blocks` / `Items` / `Entities` beside ModAPI. `Blocks.defineBlock` sets texture, per-block model (`assets/<ns>/models/block/<id>.json`), hardness, and light. `Items.defineItem` sets place-as-block and stack size. `Entities.defineEntity` sets Blockbench model, spawn egg item, and hostile AI. ModAPI `Tickable` / `Interactable` (with `TLMod.listenTick` / `listenUseBlock`) wrap `events.tick` and use hooks. `mods\ember.rg` proves that path with a glow brick, ember wand, pulse tick, and a hostile shade on `zombie.bbmodel`.

`--x` and `--z` are the center chunk, not block coordinates. The file is a standard `.mca`: 32×32 chunk slots, zlib or gzip around the chunk NBT. Empty slots are air. The small square around chunk `(0, 0)` crosses into `r.-1.0`, `r.0.-1`, and `r.-1.-1`. When the path is named `r.X.Z.mca`, those files are read from the same folder. A missing neighbor file is air. A file that is not named that way loads only the chunks that share the center's region.

## Client

```bash
cargo run -p rebuild-client
cargo run -p rebuild-client -- --port 17610
```

The client does not load a world of its own. It connects to the server, receives each non-air cell, and builds the mesh from those cells. Start the Terralite server (`rebuild-server`) first. The window needs a display and a Vulkan adapter, so it does not run headless. On Linux the window also needs `libxkbcommon-x11`. `cargo build -p rebuild-client` still typechecks and links without either.

Click the window to capture the mouse. WASD walks, the mouse looks, Space jumps, Ctrl walks faster, Esc releases the mouse. E opens the Items screen (`resourcepacks/terralite/assets/terralite/gui/index.json`). Up and down arrows select an entry. The list and the recipe line come from `datapacks/terralite/data/terralite/recipes`: a result shows what makes it, and an ingredient shows what it is used to make. Esc or E closes it. The hotbar stays on screen. Walking, looking, and breaking pause while it is open. Once the mouse is captured, the block you are looking at is marked with a white wire around that shape only, just off the surface, so it stays visible on grass, the stairs, and the sign. Looking at air draws no outline. Hold left click to break that block. Each block has a hardness in seconds, from the data pack: grass is 0.25, stone and cobblestone are 1.5, coal ore is 2, and iron ore is 2.5. The block darkens until that time elapses, then it breaks. Releasing the button or looking away clears the progress. Breaking a block leaves a small cube, about a quarter block wide, in that block's color. It sits in the cell, or on top when the cell stays solid, such as an ore that leaves its stone. Walking into the cube adds one to a stack of that id, or the first empty slot. If the bar cannot take it, the cube stays. Drops are not saved. Keys 1 through 5 select a slot. The bar starts with a stack of 64 cobblestone in slot 1 and the other four slots empty. White digits in the corner of a slot show its count, with a dark shadow so they read on the block color. The selected slot has a yellow border. Right click places one from the selected slot and the count drops. At 0 the slot is empty and places nothing. The server is the authority for each break and place. On the synthetic world it writes the cell and the block now there to `C:\CODE\TerraLite\saves\blocks.json`. The next server launch lays that file over the demo, so those blocks stay. A missing file leaves the demo unchanged. A region-file launch does not read or write that save. The hotbar is not saved. Drops are not saved. A place that would overlap the player is refused. Gravity pulls the player down onto the top of whatever solid box is under them. Shift does nothing. The mesh and collision update when the server replies.

The synthetic world is the small size, 31×31 chunks and 496 blocks on a side. Medium is 63×63 (1008 blocks), large is 127×127 (2032 blocks), and mega is 255×255 (4080 blocks). The center 3×3 of the small square holds the scene. This demo does not generate terrain for the rest of that square, or for the larger sizes. Walking into the edge of the loaded world stops the player. The player starts in the air above the grass, feet at y = 3.5, eye at about (8.5, 5.1, 14.4), looking north at the oak stairs. They drop onto the grass (the top of the cubes at y = 1) just south of those stairs, at block (8, 1, 12). Walking into a rise of one block or less, including those stairs, steps the body up when there is room above it. A taller wall stops the walk. Space still jumps. A ceiling the raised body would enter blocks the step. The body is a box, so it cannot walk through blocks.

Block shapes come from `resourcepacks/terralite/assets/terralite/pack.json`. The client and the server read that file at startup. It registers shapes, models, and blocks by string id, and the world and the mesher look those ids up in the registries. A block names a registered shape, or a registered model. A shape is boxes inside the cell. The synthetic full cubes (`grass`, `cobblestone`, `oak_log`, `wool`, and `marker`) are `cube` in that file. `marker` is the block the region file stores above 255, so the Add nibble still gets used. Oak stairs are id `oak_stairs` and shape `stair_north`: the bottom half of the cell, then the top half on the north side. They sit on the grass at block `(8, 1, 12)`, south of the log, on the line the start camera looks along, so the step faces you. Coal ore (`coal_ore`) is one block east of those stairs, at `(9, 1, 13)`. Iron ore (`iron_ore`) is one block further east, at `(10, 1, 13)`. Hardness and `"replaces"` live in the data pack at `datapacks/terralite/data/terralite/blocks`. Coal ore leaves `stone`, and iron ore leaves `deepslate`. Breaking one leaves that stone, not the other. A block with no `"replaces"` field breaks to air. A sign (`sign`) stands on the grass just west of that ore, at `(7, 1, 13)`, where a walk west from the stairs reaches it. Its pack entry points at `resourcepacks/terralite/assets/terralite/models/sign.json` instead of a named shape. That file is a Blockbench model: a cube post, and a board whose `from` and `to` meet on Z, so it is one thin quad. Collision uses those element boxes. The board is thickened by half a sixteenth on each side so the player stops on it. The post is an ordinary box. An orange lantern (`lantern`) sits on the grass just southwest of the sign, at `(6, 1, 14)`. Its pack entry is a light: color `[1.0, 0.25, 0.0]` and level `12`. That color spreads through open cells and drops one level per block, so nearby faces pick up the orange and faces with no light stay dim. Full cubes block it. The sun sits north of the world and a little to the east, so tops stay bright and faces turned away go dark. A block darkens the grass it stands on, when that grass still shows, and the grass one cell to the south, where this sun's shadow lands. The lit side of the block stays bright. A colored block light, such as the lantern, replaces that shadow on every face it reaches, so the orange shows there. Faces with no block light keep the sun shadow. There is no path trace. After that picture is drawn, and before the hotbar, the client can run the frame shader at `resourcepacks/terralite/assets/terralite/shaders/frame.wgsl`. `frame.json` is the hook: `"enabled": true` loads it, and `"enabled": false` or a missing `frame.json` skips the pass. The shader multiplies the lit color by a tint and a brightness. The shipped tint is white and the brightness is 1, so the light on the blocks stays as it is. Edit those two values to tint or brighten. The world shader is not replaced. The other eight chunks are wool floors, each with its own metadata color, plus a few cobblestone markers. Full cubes still drop a face when the neighbor covers that whole side, including across a chunk border. A smaller shape only hides the part of a face it actually covers. An id that was not registered does not mesh. Adding a block object is enough. Each block except `marker` and `plate` names a PNG in the resource pack at `resourcepacks/terralite/assets/terralite/textures` and samples it on its faces: `grass.png`, `stone.png`, `cobblestone.png`, `coal_ore.png`, `iron_ore.png`, `deepslate.png`, `dirt.png`, `oak_log.png`, `wool.png`, `oak_stairs.png`, `sign.png`, and `lantern.png`. `marker` and `plate` stay flat color. A missing PNG does not crash.

## Format this slice actually reads

- The small square: 31×31 chunks around the center chunk, 496 blocks on a side.
- Section `Blocks` (4096 bytes), `Data` (2048 metadata nibbles), and `Add` when an id needs the high nibble. Index order is `local_y << 8 | z << 4 | x`.
- Root NBT compound → `Level`, with `xPos`, `zPos`, and `Sections`.
- Region compression type 1 (gzip) or 2 (zlib).
- `resourcepacks/terralite/assets/terralite/pack.json` registers each string block id, each shape, and each Blockbench model. The client and the server read it at startup. The mesher looks those ids up in the registries. Hardness and replaces live in the data pack at `datapacks/terralite` (`data/terralite/blocks/<id>.json`). Region files still store 1.8 numeric ids.

More gameplay rules come later. The private network is TCP on localhost. Each message is a big-endian `u32` length, then a body whose first byte is the tag. Tag 1 is hello (version 1). Tag 2 is welcome: the center chunk, the chunk radius, and each non-air cell. Tag 3 is a break request. Tag 4 is a place request, with the block id and the player box. Tag 5 is the cell after the server applies the request, including whether it changed. Protocol 47 is not a goal.
