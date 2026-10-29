use std::fmt;

use serde::{Deserialize, Serialize};

/// A PCB layer, named as in Altium Designer. Serialized as its display name (`"Top Layer"`,
/// `"Mid-Layer 2"`, `"Mechanical 1"` …) so project files and agent payloads stay readable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Layer {
    TopCopper,
    /// Inner signal layer 1..=30.
    Inner(u8),
    BottomCopper,
    TopOverlay,
    BottomOverlay,
    TopPaste,
    BottomPaste,
    TopSolder,
    BottomSolder,
    /// Mechanical layer 1..=32.
    Mechanical(u8),
    KeepOut,
    /// Through-hole objects (pads and vias) that exist on every copper layer.
    MultiLayer,
    DrillGuide,
    DrillDrawing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayerKind {
    Signal,
    Overlay,
    Paste,
    Solder,
    Mechanical,
    KeepOut,
    Multi,
    Drill,
}

impl Layer {
    pub fn kind(self) -> LayerKind {
        match self {
            Layer::TopCopper | Layer::Inner(_) | Layer::BottomCopper => LayerKind::Signal,
            Layer::TopOverlay | Layer::BottomOverlay => LayerKind::Overlay,
            Layer::TopPaste | Layer::BottomPaste => LayerKind::Paste,
            Layer::TopSolder | Layer::BottomSolder => LayerKind::Solder,
            Layer::Mechanical(_) => LayerKind::Mechanical,
            Layer::KeepOut => LayerKind::KeepOut,
            Layer::MultiLayer => LayerKind::Multi,
            Layer::DrillGuide | Layer::DrillDrawing => LayerKind::Drill,
        }
    }

    pub fn is_copper(self) -> bool {
        self.kind() == LayerKind::Signal
    }

    /// The layer on the other side of the board (copper, overlay, mask and paste swap; others
    /// are unchanged). Used when a footprint is flipped to the bottom.
    pub fn flipped(self) -> Layer {
        match self {
            Layer::TopCopper => Layer::BottomCopper,
            Layer::BottomCopper => Layer::TopCopper,
            Layer::TopOverlay => Layer::BottomOverlay,
            Layer::BottomOverlay => Layer::TopOverlay,
            Layer::TopPaste => Layer::BottomPaste,
            Layer::BottomPaste => Layer::TopPaste,
            Layer::TopSolder => Layer::BottomSolder,
            Layer::BottomSolder => Layer::TopSolder,
            other => other,
        }
    }

    /// Short name used on Altium's layer tabs where space is tight.
    pub fn short_name(self) -> String {
        match self {
            Layer::TopCopper => "TL".into(),
            Layer::Inner(n) => format!("M{n}"),
            Layer::BottomCopper => "BL".into(),
            Layer::TopOverlay => "TO".into(),
            Layer::BottomOverlay => "BO".into(),
            Layer::TopPaste => "TP".into(),
            Layer::BottomPaste => "BP".into(),
            Layer::TopSolder => "TS".into(),
            Layer::BottomSolder => "BS".into(),
            Layer::Mechanical(n) => format!("M{n}"),
            Layer::KeepOut => "KO".into(),
            Layer::MultiLayer => "MU".into(),
            Layer::DrillGuide => "DG".into(),
            Layer::DrillDrawing => "DD".into(),
        }
    }

    /// Copper layers from top to bottom for a board with `count` copper layers (2 or more).
    pub fn copper_stack(count: u8) -> Vec<Layer> {
        let count = count.clamp(1, 32);
        let mut layers = vec![Layer::TopCopper];
        layers.extend((1..count.saturating_sub(1)).map(Layer::Inner));
        if count > 1 {
            layers.push(Layer::BottomCopper);
        }
        layers
    }

    /// Every layer a board with `copper` copper layers shows, in Altium's layer-tab order.
    pub fn display_order(copper: u8) -> Vec<Layer> {
        let mut layers = Layer::copper_stack(copper);
        layers.extend([
            Layer::Mechanical(1),
            Layer::Mechanical(13),
            Layer::Mechanical(15),
            Layer::TopOverlay,
            Layer::BottomOverlay,
            Layer::TopPaste,
            Layer::BottomPaste,
            Layer::TopSolder,
            Layer::BottomSolder,
            Layer::DrillGuide,
            Layer::KeepOut,
            Layer::DrillDrawing,
            Layer::MultiLayer,
        ]);
        layers
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Layer::TopCopper => f.write_str("Top Layer"),
            Layer::Inner(n) => write!(f, "Mid-Layer {n}"),
            Layer::BottomCopper => f.write_str("Bottom Layer"),
            Layer::TopOverlay => f.write_str("Top Overlay"),
            Layer::BottomOverlay => f.write_str("Bottom Overlay"),
            Layer::TopPaste => f.write_str("Top Paste"),
            Layer::BottomPaste => f.write_str("Bottom Paste"),
            Layer::TopSolder => f.write_str("Top Solder"),
            Layer::BottomSolder => f.write_str("Bottom Solder"),
            Layer::Mechanical(n) => write!(f, "Mechanical {n}"),
            Layer::KeepOut => f.write_str("Keep-Out Layer"),
            Layer::MultiLayer => f.write_str("Multi-Layer"),
            Layer::DrillGuide => f.write_str("Drill Guide"),
            Layer::DrillDrawing => f.write_str("Drill Drawing"),
        }
    }
}

impl From<Layer> for String {
    fn from(layer: Layer) -> String {
        layer.to_string()
    }
}

impl TryFrom<String> for Layer {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
        name.parse()
    }
}

impl std::str::FromStr for Layer {
    type Err = String;
    fn from_str(name: &str) -> Result<Self, String> {
        let n = name.trim().to_ascii_lowercase();
        let fixed = match n.as_str() {
            "top layer" | "top" | "toplayer" | "f.cu" => Some(Layer::TopCopper),
            "bottom layer" | "bottom" | "bottomlayer" | "b.cu" => Some(Layer::BottomCopper),
            "top overlay" | "top silkscreen" | "f.silks" => Some(Layer::TopOverlay),
            "bottom overlay" | "bottom silkscreen" | "b.silks" => Some(Layer::BottomOverlay),
            "top paste" | "f.paste" => Some(Layer::TopPaste),
            "bottom paste" | "b.paste" => Some(Layer::BottomPaste),
            "top solder" | "top solder mask" | "f.mask" => Some(Layer::TopSolder),
            "bottom solder" | "bottom solder mask" | "b.mask" => Some(Layer::BottomSolder),
            "keep-out layer" | "keepout" | "keep-out" => Some(Layer::KeepOut),
            "multi-layer" | "multilayer" => Some(Layer::MultiLayer),
            "drill guide" => Some(Layer::DrillGuide),
            "drill drawing" => Some(Layer::DrillDrawing),
            _ => None,
        };
        if let Some(layer) = fixed {
            return Ok(layer);
        }
        let numbered = |prefix: &str, max: u8| {
            n.strip_prefix(prefix).and_then(|rest| rest.trim().parse::<u8>().ok()).filter(|v| (1..=max).contains(v))
        };
        if let Some(v) = numbered("mid-layer", 30).or_else(|| numbered("inner", 30)) {
            return Ok(Layer::Inner(v));
        }
        if let Some(v) = numbered("mechanical", 32) {
            return Ok(Layer::Mechanical(v));
        }
        Err(format!("unknown layer '{name}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for layer in Layer::display_order(4) {
            assert_eq!(layer.to_string().parse::<Layer>(), Ok(layer));
        }
        assert!("Mid-Layer 31".parse::<Layer>().is_err());
        assert_eq!(serde_json::to_string(&Layer::Inner(2)).unwrap(), "\"Mid-Layer 2\"");
    }

    #[test]
    fn copper_stack_order() {
        assert_eq!(Layer::copper_stack(2), vec![Layer::TopCopper, Layer::BottomCopper]);
        assert_eq!(Layer::copper_stack(4), vec![Layer::TopCopper, Layer::Inner(1), Layer::Inner(2), Layer::BottomCopper]);
        assert_eq!(Layer::TopOverlay.flipped(), Layer::BottomOverlay);
    }
}
