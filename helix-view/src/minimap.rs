//! Shared minimap geometry and source coordinates. No terminal or I/O dependencies.
use std::ops::Range;

use serde::{Deserialize, Deserializer, Serialize};

use crate::graphics::Rect;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MinimapRender {
    #[default]
    Braille,
    Blocks,
}

impl MinimapRender {
    pub fn pixel_height(self) -> usize {
        match self {
            Self::Braille => 4,
            Self::Blocks => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct MinimapConfig {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_width")]
    pub width: u16,
    pub render: MinimapRender,
    #[serde(deserialize_with = "deserialize_min_width")]
    pub min_editor_width: u16,
    pub diagnostics: bool,
    pub git_diff: bool,
}

impl Default for MinimapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            width: 16,
            render: MinimapRender::Braille,
            min_editor_width: 60,
            diagnostics: true,
            git_diff: true,
        }
    }
}

fn deserialize_width<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    let width = u16::deserialize(d)?;
    if !(8..=40).contains(&width) {
        return Err(serde::de::Error::custom(
            "minimap.width must be between 8 and 40",
        ));
    }
    Ok(width)
}

fn deserialize_min_width<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    let width = u16::deserialize(d)?;
    if width < 20 {
        return Err(serde::de::Error::custom(
            "minimap.min-editor-width must be at least 20",
        ));
    }
    Ok(width)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinimapLayout {
    pub text: Rect,
    pub map: Option<Rect>,
    pub content: Option<Rect>,
}

impl MinimapLayout {
    pub fn new(area: Rect, gutter: u16, config: &MinimapConfig) -> Self {
        let mut text = area.clip_left(gutter).clip_bottom(1);
        let map = (config.enabled
            && (8..=40).contains(&config.width)
            && text.height >= 4
            && text.width as u32 >= config.width as u32 + config.min_editor_width as u32)
            .then(|| {
                text.width -= config.width;
                Rect::new(text.right(), text.y, config.width, text.height)
            });
        Self {
            text,
            map,
            content: map.map(|r| r.clip_left(1).clip_right(2)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMap {
    lines: usize,
    pixels: usize,
    scale: usize,
}

impl LineMap {
    pub fn new(lines: usize, height: u16, render: MinimapRender) -> Self {
        let lines = lines.max(1);
        let scale = render.pixel_height();
        Self {
            lines,
            pixels: lines.min(height.max(1) as usize * scale),
            scale,
        }
    }

    pub fn pixel(self, line: usize) -> usize {
        ((line.min(self.lines - 1) as u128 * self.pixels as u128) / self.lines as u128) as usize
    }

    pub fn row(self, line: usize) -> u16 {
        (self.pixel(line) / self.scale) as u16
    }

    pub fn pixel_lines(self, pixel: usize) -> Range<usize> {
        if pixel >= self.pixels {
            return self.lines - 1..self.lines;
        }
        let bound =
            |p: usize| (p as u128 * self.lines as u128).div_ceil(self.pixels as u128) as usize;
        bound(pixel)..bound(pixel + 1)
    }

    pub fn row_lines(self, row: u16) -> Range<usize> {
        let start = row as usize * self.scale;
        self.pixel_lines(start).start
            ..self
                .pixel_lines((start + self.scale - 1).min(self.pixels - 1))
                .end
    }

    pub fn target_line(self, row: u16) -> usize {
        let range = self.row_lines(row);
        range.start + (range.len() - 1) / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Rect;

    #[test]
    fn minimap_config_validates_and_defaults() {
        let c: crate::editor::Config = toml::from_str("").unwrap();
        assert!(!c.minimap.enabled);
        assert_eq!(c.minimap.width, 16);
        assert_eq!(c.minimap.min_editor_width, 60);
        assert_eq!(c.minimap.render, MinimapRender::Braille);
        assert!(c.minimap.diagnostics && c.minimap.git_diff);
        let c: crate::editor::Config =
            toml::from_str("[minimap]\nenabled=true\nrender='blocks'").unwrap();
        assert!(c.minimap.enabled);
        assert_eq!(c.minimap.render, MinimapRender::Blocks);
        for field in [
            "width=0",
            "width=41",
            "min-editor-width=19",
            "render='pixels'",
            "unknown=true",
        ] {
            assert!(
                toml::from_str::<crate::editor::Config>(&format!("[minimap]\n{field}")).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn minimap_layout_reserves_only_when_it_fits() {
        let mut c = MinimapConfig::default();
        let area = Rect::new(5, 3, 100, 20);
        assert_eq!(
            MinimapLayout::new(area, 4, &c).text,
            Rect::new(9, 3, 96, 19)
        );
        c.enabled = true;
        let layout = MinimapLayout::new(area, 4, &c);
        assert_eq!(layout.text, Rect::new(9, 3, 80, 19));
        assert_eq!(layout.map, Some(Rect::new(89, 3, 16, 19)));
        assert_eq!(layout.content.unwrap().width, 13);
        assert!(MinimapLayout::new(Rect::new(0, 0, 79, 20), 4, &c)
            .map
            .is_none());
        assert!(MinimapLayout::new(Rect::new(0, 0, 100, 4), 4, &c)
            .map
            .is_none());
        assert!(MinimapLayout::new(Rect::new(0, 0, 80, 20), 4, &c)
            .map
            .is_some());
    }

    #[test]
    fn minimap_line_map_roundtrips_and_clamps() {
        for render in [MinimapRender::Braille, MinimapRender::Blocks] {
            for n in [0usize, 1, 3, 40, 41, 399, 10_000, 50_001] {
                let m = LineMap::new(n, 20, render);
                assert_eq!(m.row(0), 0);
                assert!(m.row(n.saturating_sub(1)) < 20);
                for line in 0..n {
                    assert!(m.row_lines(m.row(line)).contains(&line));
                }
                assert_eq!(m.target_line(u16::MAX), n.saturating_sub(1));
            }
        }
        assert_eq!(LineMap::new(3, 20, MinimapRender::Braille).row(2), 0);
    }
}
