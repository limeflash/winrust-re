//! The installer's own colours, as the mockup's CSS writes them (`--h1b1c22` and the like in `[data-theme]`): dark value, light value.
//! The shared `--t1`..`--t3` live in `winrust_ui::widgets::look`.
use winrust_ui::gfx::Rgba;

/// The window and the module list: `--h1b1c22`.
pub const WIN: Rgba = Rgba::pair(0x1b1c22, 0xeeeff1);
/// The title bar and the footer: `--h202128`.
pub const BAR: Rgba = Rgba::pair(0x202128, 0xecedef);
/// The trough of the language switch: `--h16171c`.
pub const WELL: Rgba = Rgba::pair(0x16171c, 0xf1f2f4);
/// The chosen language: `--h2e2f38`.
pub const SEG_ON: Rgba = Rgba::pair(0x2e2f38, 0xe5e6e8);
/// The preview pane's gradient, top and bottom: `--h17181d`, `--h141519`.
pub const PANE_TOP: Rgba = Rgba::pair(0x17181d, 0xf0f1f3);
pub const PANE_BOT: Rgba = Rgba::pair(0x141519, 0xf2f3f5);
/// Behind the preview picture: `--h0d0e12`.
pub const CARD: Rgba = Rgba::pair(0x0d0e12, 0xf5f6f8);
/// Text on the white install button: `--h14151a`.
pub const INSTALL_FG: Rgba = Rgba::pair(0x14151a, 0xf1f2f4);
/// `--t1b` and `--t4` of this mockup (the shared `T4` is the bar's, a shade darker).
pub const T1B: Rgba = Rgba::pair(0xc9cdd8, 0x343844);
pub const T4: Rgba = Rgba::pair(0x6f7486, 0x8d93a1);
/// "Done": `#4cc38a`, the same in both modes.
pub const OK: Rgba = Rgba::new(0x4c as f32 / 255.0, 0xc3 as f32 / 255.0, 0x8a as f32 / 255.0, 1.0);
/// "Remove all": the danger text (`--dangerT`): `#ff8a8f` on the dark style, `#c8333a` on the light one.
pub const REMOVE: Rgba = Rgba::pair(0xff8a8f, 0xc8333a);
/// What failed: `#e5484d`.
pub const BAD: Rgba = Rgba::new(0xe5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4d as f32 / 255.0, 1.0);

pub const fn rgb(c: u32) -> Rgba {
    Rgba::new(((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0)
}
