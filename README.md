# mythterm

A GPU-accelerated terminal emulator powered by [Myth](https://github.com/panxinmiao/myth) rendering engine and [egui](https://github.com/emilk/egui).

## Architecture

```
mythterm-bin          # Binary entry point
mythterm-ui           # egui-based UI (tabs, splits, overlays, settings)
mythterm-render       # Myth engine integration + text rendering pipelines
mythterm-font         # Font discovery, shaping (rustybuzz), rasterization (ab_glyph)
mythterm-mux          # Session multiplexer (tabs, panes, PTY management)
mythterm-core         # Terminal emulator core (VT parser, screen model, cells)
mythterm-config       # Configuration (TOML, live reload)
```

## Building

```bash
cargo build --release
```

## License

MIT OR Apache-2.0
