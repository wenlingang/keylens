//! The hold animation: while the trigger key is held, an anime-style ⌘ keycap fills up with the
//! app's colour in a small click-through window over where the panel will appear. Letting go
//! early springs it away; holding on bursts it as the panel shows.
//!
//! The timeline is a pure function of time (tested below); the view only draws a frame of it and
//! asks for the next frame while something moves, so it costs nothing when idle.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// Accent when the app icon has no usable colour (same as the panel's).
pub const DEFAULT_ACCENT: u32 = 0x3b82f6;
/// Side of the (transparent) window the animation is drawn in: room for the burst star at its
/// largest, so the window edge never cuts it off.
pub const WINDOW_SIZE: f32 = 420.0;
/// How long the keycap takes to pop in once it appears.
const ENTER: Duration = Duration::from_millis(360);
/// Bursting as the panel shows, and springing away when the key is let go early.
pub const BURST: Duration = Duration::from_millis(300);
pub const RELEASE: Duration = Duration::from_millis(420);
/// The first part of the hold shows nothing: ⌘C, ⌘V and the like are pressed and gone well
/// within it, and must not flash anything.
pub const MAX_GRACE: Duration = Duration::from_millis(250);
/// The full keycap holds still this long before bursting: the panel's first frame lands in that
/// pause (a hit-stop), not in the middle of the burst, where it would show as a stutter.
pub const BURST_DELAY: Duration = Duration::from_millis(100);
/// The burst star outlives the keycap a little, and the aura and sparkles fade faster.
const BOOM: Duration = Duration::from_millis(420);
const AURA_FADE: Duration = Duration::from_millis(220);

/// Keycap size and the distances around it, in points.
const CAP: f32 = 73.0;
const CAP_RADIUS: f32 = 18.0;
const BORDER: f32 = 2.3;
const DEPTH: f32 = 7.6;
const TRAVEL: f32 = 6.0;
const AURA: f32 = 243.0;
const AURA_VARIANTS: usize = 6;
/// The aura flickers to its next shape this often.
const AURA_FLICKER_MS: u128 = 110;

const INK: u32 = 0x1b1b2f;
const FACE: u32 = 0xfffdf7;
const FACE_SHADE: u32 = 0xe6e0f5;
const SIDE: u32 = 0xc3badb;

/// How long nothing is shown for a hold of `delay`: never more than a third of it, so short
/// holds still get to see the keycap fill.
pub fn grace(delay: Duration) -> Duration {
    MAX_GRACE.min(delay / 3)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Filling up: empty at `shown_at`, full at `full_at`.
    Charging { shown_at: Instant, full_at: Instant },
    /// The panel is showing.
    Burst { at: Instant },
    /// The key was let go early, with the keycap filled to `p`.
    Release { at: Instant, p: f32 },
}

/// Everything one frame of the animation needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// How full the keycap is, 0–1.
    pub p: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    /// Vertical offset of the keycap, negative is up.
    pub lift: f32,
    /// Opacity of the whole picture. Applied to the window, not to each layer: gpui fades layers
    /// one by one, which would show the keycap's side and glyph through its face.
    pub opacity: f32,
    /// Strength of the aura and sparkles, 0–1 (they fade first when the hold ends).
    pub aura: f32,
    pub shake: bool,
    /// Progress of the burst star, while bursting.
    pub boom: Option<f32>,
    /// Nothing moves any more.
    pub done: bool,
}

fn progress(now: Instant, from: Instant, length: Duration) -> f32 {
    (now.saturating_duration_since(from).as_secs_f32() / length.as_secs_f32()).clamp(0.0, 1.0)
}

/// Overshoots a little before settling at 1, for the pop-in.
fn ease_out_back(t: f32) -> f32 {
    let c1 = 2.4;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Close to the mockup's `cubic-bezier(.2, .8, .3, 1)` and `cubic-bezier(.1, .7, .3, 1)`.
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// CSS `ease-out`.
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(2)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Phase {
    pub fn frame(&self, now: Instant) -> Frame {
        match *self {
            Phase::Charging { shown_at, full_at } => {
                let p = progress(now, shown_at, full_at.saturating_duration_since(shown_at));
                // Pops in from a fifth of its size, overshooting a little, fading in as it goes.
                let pop = ease_out_back(progress(now, shown_at, ENTER));
                Frame {
                    p,
                    scale_x: lerp(0.2, 1.0, pop),
                    scale_y: lerp(0.2, 1.0, pop),
                    lift: 0.0,
                    opacity: pop.clamp(0.0, 1.0),
                    aura: 1.0,
                    shake: p > 0.85,
                    boom: None,
                    // Still filling, or about to burst: keep drawing.
                    done: false,
                }
            }
            Phase::Burst { at } => {
                // Swells to 1.12 in the first quarter, then to 1.6, fading out all along.
                let t = progress(now, at, BURST);
                let scale = if t < 0.25 {
                    lerp(1.0, 1.12, ease_out_cubic(t / 0.25))
                } else {
                    lerp(1.12, 1.6, ease_out_cubic((t - 0.25) / 0.75))
                };
                Frame {
                    p: 1.0,
                    scale_x: scale,
                    scale_y: scale,
                    lift: 0.0,
                    opacity: 1.0 - ease_out_cubic(t),
                    aura: 1.0 - progress(now, at, AURA_FADE),
                    shake: false,
                    boom: Some(progress(now, at, BOOM)),
                    done: t >= 1.0,
                }
            }
            Phase::Release { at, p } => {
                // Squash, stretch up, then shrink to 0.15, fading out all along.
                let t = progress(now, at, RELEASE);
                let (scale_x, scale_y, lift) = if t < 0.25 {
                    let k = ease_out(t / 0.25);
                    (lerp(1.0, 1.2, k), lerp(1.0, 0.78, k), 0.0)
                } else if t < 0.55 {
                    let k = ease_out((t - 0.25) / 0.3);
                    (lerp(1.2, 0.86, k), lerp(0.78, 1.14, k), lerp(0.0, -10.0, k))
                } else {
                    let k = ease_out((t - 0.55) / 0.45);
                    (
                        lerp(0.86, 0.15, k),
                        lerp(1.14, 0.15, k),
                        lerp(-10.0, 0.0, k),
                    )
                };
                Frame {
                    p,
                    scale_x,
                    scale_y,
                    lift,
                    opacity: 1.0 - ease_out(t),
                    aura: 1.0 - progress(now, at, AURA_FADE),
                    shake: false,
                    boom: None,
                    done: t >= 1.0,
                }
            }
        }
    }
}

/// Pictures that depend on the accent colour, made again when it changes.
struct Art {
    accent: Hsla,
    auras: Vec<Arc<Image>>,
    wave: Arc<Image>,
    boom: Arc<Image>,
}

pub struct ChargeView {
    phase: Option<Phase>,
    /// Sets the opacity of the window (see `Frame::opacity`), and the value last set.
    set_alpha: fn(&Window, f32),
    alpha: f32,
    accent: Hsla,
    art: Option<Art>,
    glyph: Arc<Image>,
    glyph_lit: Arc<Image>,
    star: Arc<Image>,
    star_gold: Arc<Image>,
}

impl ChargeView {
    pub fn new(set_alpha: fn(&Window, f32)) -> Self {
        Self {
            phase: None,
            set_alpha,
            alpha: -1.0,
            accent: rgb(DEFAULT_ACCENT).into(),
            art: None,
            glyph: svg_image(glyph_svg(None, INK, 52.0)),
            glyph_lit: svg_image(glyph_svg(Some(INK), 0xffffff, 46.0)),
            star: svg_image(star_svg(0xffffff)),
            star_gold: svg_image(star_svg(0xffd84d)),
        }
    }

    pub fn phase(&self) -> Option<Phase> {
        self.phase
    }

    /// Starts filling: empty now, full at `full_at`.
    pub fn charge(&mut self, accent: Hsla, full_at: Instant, cx: &mut Context<Self>) {
        self.accent = accent;
        self.phase = Some(Phase::Charging {
            shown_at: Instant::now(),
            full_at,
        });
        cx.notify();
    }

    pub fn burst(&mut self, cx: &mut Context<Self>) {
        self.phase = Some(Phase::Burst {
            at: Instant::now() + BURST_DELAY,
        });
        cx.notify();
    }

    pub fn release(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let p = self.phase.map(|phase| phase.frame(now).p).unwrap_or(0.0);
        self.phase = Some(Phase::Release { at: now, p });
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.phase = None;
        self.alpha = -1.0;
        cx.notify();
    }

    fn art(&mut self) -> &Art {
        if self
            .art
            .as_ref()
            .is_none_or(|art| art.accent != self.accent)
        {
            let accent = hex(self.accent);
            let light = hex(Hsla {
                l: (self.accent.l + 0.22).min(0.85),
                ..self.accent
            });
            self.art = Some(Art {
                accent: self.accent,
                auras: (0..AURA_VARIANTS)
                    .map(|seed| svg_image(aura_svg(seed as u32, &accent, &light)))
                    .collect(),
                wave: svg_image(wave_svg(&light, &accent)),
                boom: svg_image(boom_svg(&light)),
            });
        }
        self.art.as_ref().expect("just made")
    }
}

impl Render for ChargeView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let root = div().size(px(WINDOW_SIZE)).relative();
        let Some(phase) = self.phase else {
            return root;
        };
        let now = Instant::now();
        let frame = phase.frame(now);
        if !frame.done {
            window.request_animation_frame();
        }
        let alpha = frame.opacity.clamp(0.0, 1.0);
        if (alpha - self.alpha).abs() > 0.002 {
            (self.set_alpha)(window, alpha);
            self.alpha = alpha;
        }
        // The burst star and the aura fade on their own schedule; the window alpha fades them too,
        // so divide it back out (they are single pictures: per-layer opacity is right for them).
        let own = |wanted: f32| {
            if alpha <= 0.001 {
                0.0
            } else {
                (wanted / alpha).clamp(0.0, 1.0)
            }
        };
        let elapsed_ms = match phase {
            Phase::Charging { shown_at, .. } => now.saturating_duration_since(shown_at).as_millis(),
            Phase::Burst { at } | Phase::Release { at, .. } => {
                now.saturating_duration_since(at).as_millis()
            }
        };
        let accent = self.accent;
        let (glyph, glyph_lit, star, star_gold) = (
            self.glyph.clone(),
            self.glyph_lit.clone(),
            self.star.clone(),
            self.star_gold.clone(),
        );
        let art = self.art();
        let aura = art.auras[(elapsed_ms / AURA_FLICKER_MS) as usize % art.auras.len()].clone();
        let (wave, boom) = (art.wave.clone(), art.boom.clone());

        let center = WINDOW_SIZE / 2.0;
        let mut root = root;

        // Aura behind everything, growing as the keycap fills.
        let aura_size = AURA * (0.5 + frame.p * 0.35) * frame.scale_x.min(1.0);
        root = root.child(
            img(aura)
                .absolute()
                .left(px(center - aura_size / 2.0))
                .top(px(center - aura_size / 2.0))
                .size(px(aura_size))
                .opacity(own(frame.p * 0.55 * frame.aura)),
        );
        if let Some(t) = frame.boom {
            let size = 152.0 * lerp(0.4, 2.6, ease_out_cubic(t));
            root = root.child(
                img(boom)
                    .absolute()
                    .left(px(center - size / 2.0))
                    .top(px(center - size / 2.0))
                    .size(px(size))
                    .opacity(own(0.7 * (1.0 - ease_out_cubic(t)))),
            );
        }

        // The keycap: the side (its thickness) under the face, both outlined in ink.
        let (w, h) = (CAP * frame.scale_x, CAP * frame.scale_y);
        let radius = CAP_RADIUS * frame.scale_x.min(frame.scale_y);
        let jitter = if frame.shake {
            if (elapsed_ms / 40) % 2 == 0 {
                0.8
            } else {
                -0.8
            }
        } else {
            0.0
        };
        let left = center - w / 2.0 + jitter;
        let top = center - h / 2.0 + frame.lift + frame.p * TRAVEL * frame.scale_y;
        let depth = ((1.0 - frame.p) * DEPTH + 1.5) * frame.scale_y;
        let inner_w = w - BORDER * 2.0;
        let inner_h = h - BORDER * 2.0;
        let liquid_h = (inner_h * frame.p * 1.1).min(inner_h);
        let glyph_size = 40.0 * frame.scale_x.min(frame.scale_y);
        let glyph_left = (inner_w - glyph_size) / 2.0;
        let glyph_top = (inner_h - glyph_size) / 2.0;
        let wave_offset = (elapsed_ms % 600) as f32 / 600.0 * 20.0 * frame.scale_x;

        // Shrunk to nothing at the end of a burst or release: nothing left to draw.
        let visible = frame.scale_x > 0.02 && frame.scale_y > 0.02 && frame.opacity > 0.0;
        if visible {
            root = root
                .child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top + depth))
                        .w(px(w))
                        .h(px(h))
                        .rounded(px(radius))
                        .bg(rgb(SIDE))
                        .border(px(BORDER))
                        .border_color(rgb(INK)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(w))
                        .h(px(h))
                        .rounded(px(radius))
                        .border(px(BORDER))
                        .border_color(rgb(INK))
                        .bg(rgb(FACE))
                        .overflow_hidden()
                        // Hard cel-shading band at the bottom of the face.
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(inner_h * 0.3))
                                .bg(rgb(FACE_SHADE)),
                        )
                        .child(
                            img(glyph)
                                .absolute()
                                .left(px(glyph_left))
                                .top(px(glyph_top))
                                .size(px(glyph_size)),
                        )
                        // The liquid, with the white ⌘ inside it lined up with the ink one.
                        .when(liquid_h > 0.0, |face| {
                            face.child(
                                img(wave)
                                    .absolute()
                                    .left(px(-wave_offset))
                                    .top(px(inner_h - liquid_h - 5.0 * frame.scale_y))
                                    .w(px(inner_w + 40.0))
                                    .h(px(8.0 * frame.scale_y)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .h(px(liquid_h))
                                    .overflow_hidden()
                                    .bg(accent)
                                    .child(
                                        img(glyph_lit)
                                            .absolute()
                                            .left(px(glyph_left))
                                            .top(px(glyph_top - (inner_h - liquid_h)))
                                            .size(px(glyph_size)),
                                    ),
                            )
                        })
                        // Gloss.
                        .child(
                            div()
                                .absolute()
                                .left(px(8.0 * frame.scale_x))
                                .top(px(6.0 * frame.scale_y))
                                .w(px(15.0 * frame.scale_x))
                                .h(px(5.0 * frame.scale_y))
                                .rounded(px(3.))
                                .bg(gpui_kit::white().opacity(0.9)),
                        ),
                );
        }

        // Two sparkles twinkling by the keycap.
        let spark = (frame.p * 1.6 - 0.4).clamp(0.0, 0.8) * frame.aura;
        if spark > 0.0 {
            let twinkle = |phase: f32| {
                let t = ((elapsed_ms as f32 / 500.0 + phase) * std::f32::consts::PI).sin();
                0.85 + 0.25 * t
            };
            for (image, dx, dy, size, phase) in [
                (star_gold, 40.0, -48.0, 15.0, 0.0),
                (star, -48.0, -26.0, 11.0, 0.5),
            ] {
                let size = size * twinkle(phase);
                root = root.child(
                    img(image)
                        .absolute()
                        .left(px(center + dx - size / 2.0))
                        .top(px(center + dy - size / 2.0))
                        .size(px(size))
                        .opacity(own(spark)),
                );
            }
        }
        root
    }
}

// ---- Pictures ----

/// gpui rasterises an SVG image once, at the width and height it declares, and scales that
/// bitmap; so each picture declares about twice the largest size it is drawn at, to stay crisp on
/// Retina screens.
fn svg_image(svg: String) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
}

fn hex(color: Hsla) -> String {
    let c = Rgba::from(color);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

fn hex_u32(color: u32) -> String {
    format!("#{color:06x}")
}

/// The ⌘ glyph drawn with strokes (same shape as the app icon). With `outline`, a wider stroke in
/// that colour goes under it.
fn glyph_svg(outline: Option<u32>, color: u32, width: f32) -> String {
    const D: &str = "M -84 -146 V 146 M 84 -146 V 146 M -146 -84 H 146 M -146 84 H 146 \
        M 84 146 A 62 62 0 1 0 146 84 M -84 146 A 62 62 0 1 1 -146 84 \
        M 84 -146 A 62 62 0 1 1 146 -84 M -84 -146 A 62 62 0 1 0 -146 -84";
    let path = |stroke: &str, w: f32| {
        format!(
            r#"<path d="{D}" fill="none" stroke="{stroke}" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"/>"#
        )
    };
    let under = outline
        .map(|c| path(&hex_u32(c), width + 42.0))
        .unwrap_or_default();
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="-240 -240 480 480">{under}{}</svg>"#,
        path(&hex_u32(color), width)
    )
}

fn star_svg(fill: u32) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="-11 -11 22 22"><path d="M0-9L2.2-2.2 9 0 2.2 2.2 0 9-2.2 2.2-9 0-2.2-2.2z" fill="{}" stroke="#1b1b2f" stroke-width="1.6" stroke-linejoin="round"/></svg>"##,
        hex_u32(fill)
    )
}

/// A small deterministic generator, so each aura variant keeps its shape.
fn next(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

fn spikes(seed: &mut u32, count: usize, inner: f32, outer: f32, jitter: f32) -> String {
    (0..count * 2)
        .map(|i| {
            let a =
                i as f32 / (count * 2) as f32 * std::f32::consts::TAU + (next(seed) - 0.5) * 0.12;
            let r = if i % 2 == 1 {
                inner
            } else {
                outer * (1.0 - jitter + next(seed) * jitter * 1.6)
            };
            format!("{:.1},{:.1}", a.cos() * r, a.sin() * r)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn aura_svg(variant: u32, accent: &str, light: &str) -> String {
    let mut seed = variant.wrapping_mul(7919).wrapping_add(17);
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="480" height="480" viewBox="-160 -160 320 320"><polygon points="{}" fill="{accent}" stroke="#1b1b2f" stroke-width="2.5" stroke-linejoin="round"/><polygon points="{}" fill="{light}"/></svg>"##,
        spikes(&mut seed, 14, 66.0, 100.0, 0.15),
        spikes(&mut seed, 14, 60.0, 84.0, 0.15),
    )
}

fn boom_svg(fill: &str) -> String {
    let mut seed = 99;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="800" viewBox="-100 -100 200 200"><polygon points="{}" fill="{fill}" stroke="#1b1b2f" stroke-width="2" stroke-linejoin="round"/></svg>"##,
        spikes(&mut seed, 14, 46.0, 92.0, 0.15)
    )
}

/// The liquid's wavy surface: a light crest over the accent body, two periods wide so it can
/// slide sideways by one period.
fn wave_svg(light: &str, accent: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="20" viewBox="0 0 120 10" preserveAspectRatio="none"><path d="M0 4 Q5 0 10 4 T20 4 T30 4 T40 4 T50 4 T60 4 T70 4 T80 4 T90 4 T100 4 T110 4 T120 4 V10 H0Z" fill="{light}"/><path d="M0 7 Q5 3 10 7 T20 7 T30 7 T40 7 T50 7 T60 7 T70 7 T80 7 T90 7 T100 7 T110 7 T120 7 V10 H0Z" fill="{accent}"/></svg>"#
    )
}

#[cfg(test)]
mod tests {
    // `use gpui_kit::*` above brings in gpui's test macro, which would shadow the built-in one.
    use core::prelude::v1::test;

    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn grace_is_at_most_a_third_of_the_hold() {
        assert_eq!(grace(ms(800)), ms(250));
        assert_eq!(grace(ms(300)), ms(100));
    }

    #[test]
    fn charging_fills_up_by_the_time_the_hold_completes() {
        let t0 = Instant::now();
        let phase = Phase::Charging {
            shown_at: t0,
            full_at: t0 + ms(550),
        };
        assert_eq!(phase.frame(t0).p, 0.0);
        assert!((phase.frame(t0 + ms(275)).p - 0.5).abs() < 0.01);
        assert_eq!(phase.frame(t0 + ms(550)).p, 1.0);
        // It keeps drawing until the controller turns it into a burst or a release.
        assert!(!phase.frame(t0 + ms(5000)).done);
    }

    #[test]
    fn keycap_pops_in_and_only_shakes_near_the_end() {
        let t0 = Instant::now();
        let phase = Phase::Charging {
            shown_at: t0,
            full_at: t0 + ms(550),
        };
        let start = phase.frame(t0);
        assert!(start.scale_x < 0.25 && start.opacity == 0.0);
        let entered = phase.frame(t0 + ENTER);
        assert!((entered.scale_x - 1.0).abs() < 0.01);
        assert!(!phase.frame(t0 + ms(400)).shake);
        assert!(phase.frame(t0 + ms(520)).shake);
    }

    #[test]
    fn burst_grows_fades_and_finishes() {
        let t0 = Instant::now();
        let phase = Phase::Burst { at: t0 };
        let mid = phase.frame(t0 + BURST / 2);
        assert!(mid.scale_x > 1.0 && mid.boom.is_some() && !mid.done);
        let end = phase.frame(t0 + BURST);
        assert!(end.done && end.opacity <= 0.0 && end.scale_x > 1.5);
    }

    #[test]
    fn release_squashes_then_shrinks_away_keeping_the_fill() {
        let t0 = Instant::now();
        let phase = Phase::Release { at: t0, p: 0.4 };
        let squash = phase.frame(t0 + RELEASE / 4);
        assert!(squash.scale_x > 1.1 && squash.scale_y < 0.85);
        assert_eq!(squash.p, 0.4);
        let end = phase.frame(t0 + RELEASE);
        assert!(end.done && end.opacity <= 0.0 && (end.scale_x - 0.15).abs() < 0.01);
    }

    #[test]
    fn pictures_are_valid_svg_documents() {
        for svg in [
            glyph_svg(Some(INK), 0xffffff, 46.0),
            star_svg(0xffd84d),
            aura_svg(3, "#3b82f6", "#8ab6fa"),
            boom_svg("#8ab6fa"),
            wave_svg("#8ab6fa", "#3b82f6"),
        ] {
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{svg}");
            assert!(!svg.contains("NaN"), "{svg}");
        }
    }

    #[test]
    fn aura_variants_differ() {
        assert_ne!(aura_svg(0, "#000", "#fff"), aura_svg(1, "#000", "#fff"));
    }
}
