//! #9038: every adjustment layer showed the same half-filled circle in the Layers panel, so a
//! Levels layer could not be told from a Curves one. Each kind now shows its own glyph, the same
//! one as its Adjustments panel button.

use egui::vec2;
use egui_kittest::Harness;
use serde_json::json;

use crate::PhotocraftApp;

/// Every kind the Adjustments panel creates (`layer.newAdjustmentLayer.<kind>`).
const KINDS: [&str; 16] = [
    "brightnessContrast",
    "levels",
    "curves",
    "exposure",
    "vibrance",
    "hueSaturation",
    "colorBalance",
    "blackWhite",
    "photoFilter",
    "channelMixer",
    "invert",
    "posterize",
    "threshold",
    "gradientMap",
    "selectiveColor",
    "colorLookup",
];

/// Each adjustment kind maps to its own embedded icon, so no two kinds share a thumbnail glyph,
/// and none falls back to the generic adjustment glyph (#9038).
#[test]
fn every_adjustment_kind_has_its_own_embedded_icon() {
    let icons: Vec<&str> = KINDS.iter().map(|k| super::adjustment_icon(k)).collect();
    for (kind, icon) in KINDS.iter().zip(&icons) {
        assert!(crate::icon_data::ICONS.iter().any(|(name, _)| name == icon), "{kind}: icon {icon} is embedded");
        assert_ne!(*icon, super::adjustment_icon("unsupported"), "{kind}: not the generic glyph");
    }
    let mut unique = icons.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), KINDS.len(), "one glyph per kind: {icons:?}");
}

/// A Layers panel over a document holding one adjustment layer of each of `kinds`, rendered on
/// the offscreen adapter. `None` when this machine has no graphics adapter.
fn harness(kinds: &[&str]) -> Option<Harness<'static, PhotocraftApp>> {
    let mut session = photocraft_engine::Session::new();
    session.execute("prefs.set", json!({"path": "interface.language", "value": "en"})).unwrap();
    session.execute("file.new", json!({"width": 64, "height": 64, "background": "white"})).unwrap();
    for kind in kinds {
        session.execute(&format!("layer.newAdjustmentLayer.{kind}"), json!({})).unwrap();
    }
    // The Background row is active, so no adjustment row is highlighted.
    let doc = &session.active().unwrap().doc;
    let bg = doc.walk().into_iter().find(|(_, _, l)| matches!(l.content, photocraft_doc::LayerContent::Raster(_))).map(|(_, _, l)| l.id).unwrap();
    session.execute("layer.select", json!({"layer": bg.0})).unwrap();
    let app = PhotocraftApp::new(session, Default::default());
    // `wgpu()` panics when no adapter is found; only that step may be skipped.
    let builder =
        std::panic::catch_unwind(|| Harness::builder().with_size(vec2(420.0, 220.0 + 48.0 * kinds.len() as f32)).with_pixels_per_point(1.0).wgpu()).ok()?;
    let mut h = builder.build_ui_state(
        |ui, app: &mut PhotocraftApp| {
            if !ui.ctx().fonts(|fonts| fonts.families().contains(&egui::FontFamily::Name("medium".into()))) {
                return;
            }
            crate::i18n::with_language(crate::i18n::Lang::EN, || {
                app.ppp = ui.ctx().pixels_per_point();
                super::layers(app, ui);
            });
        },
        app,
    );
    PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::default());
    h.run_steps(8);
    Some(h)
}

/// The rendered pixels (read through `pixel`) inside the thumbnail of the Layers panel row for
/// `layer`, inset past the rounded corners and frame so only the glyph and its tile are compared.
fn thumb_pixels(h: &Harness<'static, PhotocraftApp>, pixel: impl Fn(u32, u32) -> [u8; 4], layer: u64) -> Vec<[u8; 4]> {
    let rows = crate::layer_row_ui::recorded(&h.ctx);
    let thumb = rows.iter().find(|r| r.layer == layer).and_then(|r| r.thumbnail).expect("adjustment row has a thumbnail").shrink(4.0);
    let mut px = Vec::new();
    for y in thumb.top().ceil() as u32..thumb.bottom().floor() as u32 {
        for x in thumb.left().ceil() as u32..thumb.right().floor() as u32 {
            px.push(pixel(x, y));
        }
    }
    px
}

/// Levels, Curves and Hue/Saturation layers render three different thumbnails in the Layers
/// panel; before #9038 they were pixel-identical half-filled circles.
#[test]
fn adjustment_layer_thumbnails_differ_by_kind() {
    let kinds = ["levels", "curves", "hueSaturation"];
    let Some(mut h) = harness(&kinds) else {
        eprintln!("skipping: no offscreen graphics adapter");
        return;
    };
    let image = h.render().unwrap();
    let ids: Vec<u64> = h
        .state()
        .session
        .active()
        .unwrap()
        .doc
        .walk()
        .into_iter()
        .filter(|(_, _, l)| matches!(l.content, photocraft_doc::LayerContent::Adjustment(_)))
        .map(|(_, _, l)| l.id.0)
        .collect();
    assert_eq!(ids.len(), kinds.len());
    if let Some(dir) = std::env::var_os("PHOTOCRAFT_THUMB_OUTPUT") {
        image.save(std::path::Path::new(&dir).join("adjustment-thumbs.png")).unwrap();
    }
    let thumbs: Vec<_> = ids.iter().map(|id| thumb_pixels(&h, |x, y| image.get_pixel(x, y).0, *id)).collect();
    for t in &thumbs {
        let first = t[0];
        assert!(t.iter().any(|p| *p != first), "the thumbnail draws a glyph, not a blank tile");
    }
    for i in 0..thumbs.len() {
        for j in i + 1..thumbs.len() {
            assert_eq!(thumbs[i].len(), thumbs[j].len(), "thumbnails share a size");
            // Rows can sit at different sub-pixel offsets, so the same glyph may differ by a few
            // anti-aliased edge pixels; a different glyph changes a large share of the tile.
            let changed = thumbs[i].iter().zip(&thumbs[j]).filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 64)).count();
            eprintln!("rows {i} and {j}: {changed} of {} pixels differ", thumbs[i].len());
            assert!(changed * 20 > thumbs[i].len(), "rows {i} and {j} must not share a thumbnail glyph ({changed} pixels differ)");
        }
    }
}
