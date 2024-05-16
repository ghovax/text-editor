use std::ops::Range;

use fontdb::{Family, Stretch, Style, Weight};
use rangemap::RangeMap;

use crate::color::Color;

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
    pub fn new() -> Self {
        Self {
            color: None,
            family: Family::SansSerif,
            stretch: Stretch::Normal,
            style: Style::Normal,
            weight: Weight::NORMAL,
            cache_key_flags: CacheKeyFlags::empty(),
        }
    }
}

/// List of text attributes to apply to a line.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttributesList {
    defaults: Attributes,
    pub spans: RangeMap<usize, Attributes>,
}

impl AttributesList {
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
    pub fn get_span(&self, index: usize) -> Attributes {
        self.spans.get(&index).unwrap_or(&self.defaults)
    }

    /// Split attributes list at an offset.
    pub fn split_off(&mut self, index: usize) -> Option<Self> {
        let mut updated_attributes_list = Self::new(self.defaults.as_attrs());
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

        for (key, resize) in ranges_to_remove {
            let (range, attributes) = self
                .spans
                .get_key_value(&key.start)
                .map(|range_to_remove| (range_to_remove.0.clone(), range_to_remove.1.clone()))?;
            self.spans.remove(key);

            if resize {
                updated_attributes_list
                    .spans
                    .insert(0..range.end - index, attributes.clone());
                self.spans.insert(range.start..index, attributes);
            } else {
                updated_attributes_list
                    .spans
                    .insert(range.start - index..range.end - index, attributes);
            }
        }

        updated_attributes_list
    }
}
