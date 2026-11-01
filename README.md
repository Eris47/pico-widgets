# pico-widgets

A tiny, low-memory desktop widgets app for Windows, written in Rust.

**Goals:** very low RAM use, small single `.exe`, many widget types (clock, photos, ...), and a widget maker so anyone can design their own.

## Status

Milestone 1: a draggable, see-through clock widget.

- Drag it anywhere with the left mouse button (the position is remembered)
- Right-click it, then choose **Quit**
- Redraws once per minute, so it uses almost no CPU

## Roadmap

1. Clock widget (done)
2. Widget framework: several widgets, a shared grid, tray icon, start with Windows
3. Photo widgets (single photo, slideshow) with memory-friendly image loading
4. More widgets: calendar, weather, system monitor, notes
5. Custom widget maker: describe a widget in a small text file, then add a visual editor

## Build it yourself

You need [Rust](https://rustup.rs) and the Visual Studio C++ Build Tools. Then:

```
cargo run --release
```

New to all this? Read **[GUIDE.md](GUIDE.md)**: a step-by-step beginner guide.

## License

MIT, see [LICENSE](LICENSE).
