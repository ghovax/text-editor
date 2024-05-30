use std::ops::Range;

pub use fontdb::{Family, Stretch, Style, Weight};
use rangemap::RangeMap;
use serde::ser::SerializeSeq as _;

use crate::{color::Color, glyph_cache::CacheKeyFlags};

/// Text attributes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Attributes {
    pub color: Option<Color>,
    pub family: fontdb::Family<'static>,
    pub stretch: fontdb::Stretch,
    pub style: fontdb::Style,
    pub weight: fontdb::Weight,
    pub cache_key_flags: CacheKeyFlags,
}

impl Attributes {
    /// Create a new set of attributes with sane defaults.
    ///
    /// This defaults to a regular Serif font.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            color: None,
            family: Family::Serif,
            stretch: Stretch::Normal,
            style: Style::Normal,
            weight: Weight::BOLD,
            cache_key_flags: CacheKeyFlags::empty(),
        }
    }

    /// Check if these attributes match the ones of the font.
    pub fn matches(&self, face: &fontdb::FaceInfo) -> bool {
        // TODO(ghovax): Is there a smarter way of including emojis?
        face.post_script_name.contains("Emoji") || (face.style == self.style && face.stretch == self.stretch)
    }

    /// Check if this set of attributes can be shaped with another.
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.family == other.family
            && self.stretch == other.stretch
            && self.style == other.style
            && self.weight == other.weight
    }

    pub fn italic(&self) -> Self {
        Self {
            style: Style::Italic,
            ..self.clone()
        }
    }

    pub fn bold(&self) -> Self {
        Self {
            weight: Weight::MEDIUM,
            ..self.clone()
        }
    }
}

/// List of text attributes to apply to a line.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttributesList {
    pub default_attributes: Attributes,
    pub spans: RangeMap<usize, Attributes>,
}

impl AttributesList {
    /// Create a new attributes list with a set of default [`Attributes`].
    pub fn new(default_attributes: Attributes) -> Self {
        Self {
            default_attributes,
            spans: RangeMap::new(),
        }
    }

    /// Add an attribute span, removes any previous matching parts of spans.
    pub fn add_span(&mut self, range: Range<usize>, attributes: Attributes) {
        if range.start == range.end {
            return;
        }

        self.spans.insert(range, attributes);
    }

    /// Get the attribute span for an index.
    ///
    /// This returns a span that contains the index.
    pub fn get_span(&self, index: usize) -> &Attributes {
        self.spans.get(&index).unwrap_or(&self.default_attributes)
    }

    /// Split attributes list at an offset.
    pub fn split_off(&mut self, index: usize) -> Option<Self> {
        let mut updated_attributes_list = Self::new(self.default_attributes);
        let mut ranges_to_remove = Vec::new();

        // Get the keys we need to remove or fix
        for span in self.spans.iter() {
            if span.0.end <= index {
                continue;
            } else if span.0.start >= index {
                ranges_to_remove.push((span.0.clone(), false));
            } else {
                ranges_to_remove.push((span.0.clone(), true));
            }
        }

        for (range_to_remove, to_resize) in ranges_to_remove {
            let (selected_range, attributes) = self
                .spans
                .get_key_value(&range_to_remove.start)
                .map(|range_to_remove| (range_to_remove.0.clone(), *range_to_remove.1))?;
            self.spans.remove(range_to_remove);

            if to_resize {
                updated_attributes_list
                    .spans
                    .insert(0..selected_range.end - index, attributes);
                self.spans.insert(selected_range.start..index, attributes);
            } else {
                updated_attributes_list
                    .spans
                    .insert(selected_range.start - index..selected_range.end - index, attributes);
            }
        }

        Some(updated_attributes_list)
    }
}

impl serde::Serialize for Attributes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Basically for each or the selected properties of the font, we serialize the value
        // as a string and append it onto a vector. Then we serialize the vector.
        // So for example if I have annot italic style and a medium weight, the serialized
        // vector would be ["italic", "bold"], and then we would serialize that vector.
        let mut attributes_strings: Vec<String> = Vec::new();
        if self.style == Style::Italic {
            attributes_strings.push("italic".to_string());
        }

        if self.weight == Weight::MEDIUM {
            attributes_strings.push("bold".to_string());
        }

        if attributes_strings.is_empty() {
            attributes_strings.push("roman".to_string());
        }

        // Serialize the vector as a vector containing the strings
        let mut sequence = serializer.serialize_seq(Some(attributes_strings.len()))?;
        for attribute in &attributes_strings {
            sequence.serialize_element(attribute)?;
        }

        sequence.end()
    }
}

impl<'de> serde::Deserialize<'de> for Attributes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialize the sequence of strings
        let attributes_strings: Vec<String> = serde::Deserialize::deserialize(deserializer)?;

        // Process the deserialized strings
        let mut attributes = Attributes::new();
        for attribute in attributes_strings {
            match attribute.as_str() {
                "italic" => attributes.style = Style::Italic,
                "bold" => attributes.weight = Weight::MEDIUM,
                "roman" => attributes.style = Style::Normal,
                _ => {
                    return Err(serde::de::Error::custom(format!(
                        "Unknown attribute string: {}",
                        attribute
                    )))
                }
            }
        }

        Ok(attributes)
    }
}

#[test]
fn tester_attributes_serialize() {
    let mut attributes = Attributes::new().italic().bold();

    let serialized = serde_json::to_string(&attributes).unwrap();
    assert_eq!(serialized, r#"["italic","bold"]"#);
}

#[test]
fn tester_attributes_deserialize() {
    let attributes = serde_json::from_str::<Attributes>(r#"["italic","bold"]"#).unwrap();

    assert_eq!(attributes.style, Style::Italic);
    assert_eq!(attributes.weight, Weight::MEDIUM);
}
