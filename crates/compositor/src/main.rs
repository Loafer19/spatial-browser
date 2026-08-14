// Compositor: owns the window, the GPU surface, and the spatial canvas.
// Roadmap: winit window -> wgpu instance -> textured-quad renderer for
// CEF OSR shared textures -> egui overlay for browser chrome (url bar,
// history, bookmarks).

fn main() {
    println!("spatial-browser: compositor bootstrap");
}
