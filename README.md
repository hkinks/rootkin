# Rootkin

A free, cross-platform family tree editor written in Rust. It opens, edits and
saves `.familyx` files (the format used by Chronoplex *My Family Tree*), so a
tree can be maintained on Linux and still be opened by the Windows program.

Not affiliated with or endorsed by Chronoplex Software. The file format was
reverse-engineered solely for interoperability; see [docs/design.md](docs/design.md).

## Features

- Read and write `.familyx` (v3 `family8` part) losslessly: anything the editor
  does not understand is preserved byte-for-byte
- People list with search, zoomable ancestor/descendant tree, photos
- Edit names, gender, living status, events (birth, death, marriage, …)
- Add and link parents, spouses and children; delete people; undo/redo
- Atomic saves with a `.bak` copy of the previous version

## Build and run

With Nix:

```sh
nix-shell
cargo run --release -p rootkin -- path/to/tree.familyx
```

Elsewhere you need a Rust toolchain plus the usual eframe/wgpu runtime libraries
(Vulkan or OpenGL, Wayland or X11).

Command-line tool:

```sh
cargo run -p rootkin-cli -- summary tree.familyx       # list people
cargo run -p rootkin-cli -- dump tree.familyx out.xml  # decoded XML
cargo run -p rootkin-cli -- example new.familyx        # fictional sample tree
```

## Layout

- `crates/familyx` — format library (codec, lossless XML, model, edits)
- `crates/rootkin` — the egui desktop app
- `crates/rootkin-cli` — command-line tool
- `docs/` — design notes, TODO, UX review

## Privacy

Family trees contain personal data. `.gitignore` excludes `*.familyx`, decoded
XML and backups; the only committed tree is the fictional
`crates/familyx/tests/data/example.familyx`.

## License

Released into the public domain under the [Unlicense](UNLICENSE), or, at your
option, the [MIT license](LICENSE-MIT).
