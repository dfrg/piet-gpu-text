use piet_parley::swash;
use swash::scale::{ScaleContext, Scaler};
use swash::zeno::{Vector, Verb};
use swash::{FontRef, GlyphId};

use piet::kurbo::{Point};

use piet_parley::ParleyTextLayout;

use piet_gpu_types::scene::{CubicSeg, Element, FillColor, LineSeg, QuadSeg, Transform};

use crate::render_ctx::{self, FillMode};
use crate::PietGpuRenderContext;

// This is very much a hack to get things working.
// On Windows, can set this to "c:\\Windows\\Fonts\\seguiemj.ttf" to get color emoji
const FONT_DATA: &[u8] = include_bytes!("../third-party/Roboto-Regular.ttf");

#[derive(Default)]
pub struct PathEncoder {
    elements: Vec<Element>,
    n_segs: usize,
    // If this is zero, then it's a text glyph and should be followed by a fill
    n_colr_layers: usize,
}

fn make_path(font: &FontRef, scaler: &mut Scaler, glyph_id: GlyphId) -> PathEncoder {
    /*
    let mut encoder = PathEncoder::default();
    self.face.outline_glyph(glyph_id, &mut encoder);
    encoder
    */
    // Should the scale context be in the font? In the RenderCtx?
    let mut encoder = PathEncoder::default();
    if scaler.has_color_outlines() {
        if let Some(outline) = scaler.scale_color_outline(glyph_id) {
            // TODO: be more sophisticated choosing a palette
            let palette = font.color_palettes().next().unwrap();
            let mut i = 0;
            while let Some(layer) = outline.get(i) {
                if let Some(color_ix) = layer.color_index() {
                    let color = palette.get(color_ix);
                    encoder.append_outline(layer.verbs(), layer.points());
                    encoder.append_solid_fill(color);
                }
                i += 1;
            }
            return encoder;
        }
    }
    if let Some(outline) = scaler.scale_outline(glyph_id) {
        encoder.append_outline(outline.verbs(), outline.points());
    }
    encoder
}

pub(crate) fn draw_text(ctx: &mut PietGpuRenderContext, layout: &ParleyTextLayout, pos: Point) {
    ctx.set_fill_mode(FillMode::Nonzero);
    let mut scale_ctx = ScaleContext::new();
    let tpos = render_ctx::to_f32_2(pos);
    for line in layout.layout.lines() {
        let mut last_x = 0.0;
        let mut last_y = 0.0;
        ctx.encode_transform(Transform {
            mat: [1.0, 0.0, 0.0, -1.0],
            translate: tpos,
        });
        for glyph_run in line.glyph_runs() {
            let run = glyph_run.run();
            let color = &glyph_run.style().brush.0;
            let font = run.font();
            let font = font.as_ref();
            let mut first = true;
            let mut scaler = scale_ctx.builder(font).size(run.font_size()).build();
            for glyph in glyph_run.positioned_glyphs() {  
                let delta_x = glyph.x - last_x;
                let delta_y = glyph.y - last_y;
                let transform = Transform {
                    mat: [1.0, 0.0, 0.0, 1.0],
                    translate: [delta_x, -delta_y],
                };
                last_x = glyph.x;
                last_y = glyph.y;
                if first {
                    if let Some(deco) = glyph_run.style().underline.as_ref() {
                        let offset = deco.offset.unwrap_or(run.metrics().underline_offset);
                        let size = deco.size.unwrap_or(run.metrics().underline_size);
                        ctx.encode_transform(Transform {
                            mat: [1.0, 0.0, 0.0, 1.0],
                            translate: [delta_x, -(delta_y - offset)],
                        });
                        let width = glyph_run.advance();
                        let mut path = PathEncoder::default();
                        path.elements.push(Element::Line(LineSeg { p0: [0.0; 2], p1: [width, 0.0]}));
                        path.elements.push(Element::Line(LineSeg { p0: [width, 0.0], p1: [width, -size]}));
                        path.elements.push(Element::Line(LineSeg { p0: [width, -size], p1: [0.0, -size]}));
                        path.elements.push(Element::Line(LineSeg { p0: [0.0, -size], p1: [0.0; 2]}));
                        path.n_segs += 4;
                        ctx.append_path_encoder(&path);
                        ctx.fill_glyph(deco.brush.0.as_rgba_u32());
                        ctx.encode_transform(Transform {
                            mat: [1.0, 0.0, 0.0, 1.0],
                            translate: [-delta_x, delta_y - offset],
                        });
                    }
                    if let Some(deco) = glyph_run.style().strikethrough.as_ref() {
                        let offset = deco.offset.unwrap_or(run.metrics().strikethrough_offset);
                        let size = deco.size.unwrap_or(run.metrics().strikethrough_size);
                        ctx.encode_transform(Transform {
                            mat: [1.0, 0.0, 0.0, 1.0],
                            translate: [delta_x, -(delta_y - offset)],
                        });
                        let width = glyph_run.advance();
                        let mut path = PathEncoder::default();
                        path.elements.push(Element::Line(LineSeg { p0: [0.0; 2], p1: [width, 0.0]}));
                        path.elements.push(Element::Line(LineSeg { p0: [width, 0.0], p1: [width, -size]}));
                        path.elements.push(Element::Line(LineSeg { p0: [width, -size], p1: [0.0, -size]}));
                        path.elements.push(Element::Line(LineSeg { p0: [0.0, -size], p1: [0.0; 2]}));
                        path.n_segs += 4;
                        ctx.append_path_encoder(&path);
                        ctx.fill_glyph(deco.brush.0.as_rgba_u32());
                        ctx.encode_transform(Transform {
                            mat: [1.0, 0.0, 0.0, 1.0],
                            translate: [-delta_x, delta_y - offset],
                        });
                    }
                }
                first = false;                
                //println!("{:?}, {:?}", transform.mat, transform.translate);
                ctx.encode_transform(transform);
                let path = make_path(&font, &mut scaler, glyph.id);
                ctx.append_path_encoder(&path);
                if path.n_colr_layers == 0 {                    
                    ctx.fill_glyph(color.as_rgba_u32());
                    // ctx.fill_glyph(0xff_ff_ff_ff);
                } else {
                    ctx.bump_n_paths(path.n_colr_layers);
                }
            }
        }
        ctx.encode_transform(Transform {
            mat: [1.0, 0.0, 0.0, -1.0],
            translate: [-(tpos[0] + last_x), tpos[1] + last_y],
        });
    }
}

impl PathEncoder {
    pub(crate) fn elements(&self) -> &[Element] {
        &self.elements
    }

    pub(crate) fn n_segs(&self) -> usize {
        self.n_segs
    }

    fn append_outline(&mut self, verbs: &[Verb], points: &[Vector]) {
        let elements = &mut self.elements;
        let old_len = elements.len();
        let mut i = 0;
        let mut start_pt = [0.0f32; 2];
        let mut last_pt = [0.0f32; 2];
        for verb in verbs {
            match verb {
                Verb::MoveTo => {
                    start_pt = convert_swash_point(points[i]);
                    last_pt = start_pt;
                    i += 1;
                }
                Verb::LineTo => {
                    let p1 = convert_swash_point(points[i]);
                    elements.push(Element::Line(LineSeg { p0: last_pt, p1 }));
                    last_pt = p1;
                    i += 1;
                }
                Verb::QuadTo => {
                    let p1 = convert_swash_point(points[i]);
                    let p2 = convert_swash_point(points[i + 1]);
                    elements.push(Element::Quad(QuadSeg {
                        p0: last_pt,
                        p1,
                        p2,
                    }));
                    last_pt = p2;
                    i += 2;
                }
                Verb::CurveTo => {
                    let p1 = convert_swash_point(points[i]);
                    let p2 = convert_swash_point(points[i + 1]);
                    let p3 = convert_swash_point(points[i + 2]);
                    elements.push(Element::Cubic(CubicSeg {
                        p0: last_pt,
                        p1,
                        p2,
                        p3,
                    }));
                    last_pt = p3;
                    i += 3;
                }
                Verb::Close => {
                    if start_pt != last_pt {
                        elements.push(Element::Line(LineSeg {
                            p0: last_pt,
                            p1: start_pt,
                        }));
                    }
                }
            }
        }
        self.n_segs += elements.len() - old_len;
    }

    fn append_solid_fill(&mut self, color: [u8; 4]) {
        let rgba_color = u32::from_be_bytes(color);
        self.elements
            .push(Element::FillColor(FillColor { rgba_color }));
        self.n_colr_layers += 1;
    }
}

fn convert_swash_point(v: Vector) -> [f32; 2] {
    [v.x, v.y]
}
